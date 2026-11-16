//! cr-engine — W5.1 BT 引擎门面。
//!
//! 边界（[ADR-002](../../../docs/02-architecture/adr/ADR-002-bt-engine.md) 选项 A + [ADR-015]）：
//! 业务 crate 只依赖本文件的 `Engine` Trait / `RqbitEngine`，不直接 `use librqbit`。
//! 依赖分层见 [ADR-015](../../../docs/02-architecture/adr/ADR-015-dependency-layering.md)：
//! 协议与信令 crate 保持 std-only，只有本 crate 与 `coldrevived` 允许引入 librqbit + tokio。
//!
//! 设计取舍：`coldrevived` 是 std 线程模型（`thread::spawn` + `Arc<Mutex<..>>`），
//! 而 librqbit 是 async。这里把 Tokio 运行时**关在本 crate 内部**：`RqbitEngine` 持有一个
//! 多线程 Runtime，对外全部是同步方法，调用方不需要是 async。每次调用只 `block_on` 一小段
//! 有超时的 future，下载本身由运行时后台推进，不占用调用线程。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use librqbit::api::TorrentIdOrHash;
use librqbit::{
    AddTorrent, AddTorrentOptions, ConnectionOptions, ListenerMode, ListenerOptions, Session,
    SessionOptions, TorrentStatsState,
};

/// 引擎内部任务号。与 `coldrevived` 的 aria2 `gid` 是两套编号，映射关系由调用方保存。
pub type TaskId = u64;

/// 单个任务的引擎状态。
///
/// `Cold` = 已实例化但未传输（paused / 还没开始），`Waking` = 正在初始化或建连，
/// `Hot` = 正在传输，`Failed` = 引擎报错（错误原文在 `EngineProgress::error`），
/// `Stopped` = 已从引擎移除。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState {
    Stopped,
    Cold,
    Waking,
    Hot,
    Failed,
}

impl EngineState {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineState::Stopped => "stopped",
            EngineState::Cold => "cold",
            EngineState::Waking => "waking",
            EngineState::Hot => "hot",
            EngineState::Failed => "failed",
        }
    }
}

impl std::fmt::Display for EngineState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 一次采样的真实计数器。**全部字段来自引擎自身观测**，没有估算值。
#[derive(Debug, Clone)]
pub struct EngineProgress {
    pub state: EngineState,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub uploaded_bytes: u64,
    pub peers: usize,
    pub finished: bool,
    /// 引擎自报的下行速率（Mbps，librqbit 原始单位）。不**换算**成字节/秒：
    /// 换算规则若猜错就会把速度报成假的，宁可按引擎原样透出。
    pub download_mbps: f64,
    pub error: Option<String>,
}

/// 加入一个种子所需的全部输入。
pub struct TorrentSpec<'a> {
    /// 展示名（aria2 兼容层用），可以为空。
    pub name: &'a str,
    /// `.torrent` 原始字节（必须是完整 bencode，由调用方负责取回）。
    pub bytes: &'a [u8],
    /// 覆盖会话默认下载目录；`None` 用 `EngineOptions::download_dir`。
    pub output_dir: Option<PathBuf>,
    /// 已知 peer（跨主机时由调用方给出）；空表示只用 tracker/DHT。
    pub initial_peers: Vec<SocketAddr>,
    /// `true` = 实例化后保持暂停（冷态），用于按需唤醒的 COLD 语义。
    pub paused: bool,
    /// 已存在同名文件时是否允许覆盖（做种/续传必须为 `true`）。
    pub overwrite: bool,
    /// 忽略种子内嵌 tracker（本地取证环境需要）。
    pub disable_trackers: bool,
}

impl<'a> TorrentSpec<'a> {
    pub fn new(name: &'a str, bytes: &'a [u8]) -> Self {
        Self {
            name,
            bytes,
            output_dir: None,
            initial_peers: Vec::new(),
            paused: false,
            overwrite: true,
            disable_trackers: false,
        }
    }
}

/// 会话级参数。
#[derive(Debug, Clone)]
pub struct EngineOptions {
    pub download_dir: PathBuf,
    /// `None` = **不监听任何入站端口**（只做下载方）。做种必须显式给出地址。
    pub listen: Option<SocketAddr>,
    /// 默认 `false`：不加入公共 DHT。要开必须由调用方显式承担公网报文。
    pub enable_dht: bool,
    /// 默认 `true`：用种子内嵌的 tracker。
    pub enable_trackers: bool,
    pub ipv4_only: bool,
    pub upload_bps: Option<u32>,
    pub download_bps: Option<u32>,
    /// 初始化（读盘 + 校验已有分片）的等待上限，超时如实报错。
    pub init_timeout: Duration,
}

impl EngineOptions {
    pub fn new<P: AsRef<Path>>(download_dir: P) -> Self {
        Self {
            download_dir: download_dir.as_ref().to_path_buf(),
            listen: None,
            enable_dht: false,
            enable_trackers: true,
            ipv4_only: true,
            upload_bps: None,
            download_bps: None,
            init_timeout: Duration::from_secs(60),
        }
    }
}

struct Task {
    handle: Arc<librqbit::ManagedTorrent>,
    info_hash: String,
    name: String,
}

/// 业务侧唯一可见的引擎接口。
///
/// 与 W5.1 骨架相比，`&mut self` 的全局 pause/resume 换成了**按任务**的形态：
/// 一个守护进程要同时持有多个种子（下载中的、做种中的、冷态的），
/// 全局开关无法表达 LRU 活跃池。
pub trait Engine {
    fn add_torrent(&mut self, spec: &TorrentSpec<'_>) -> Result<TaskId, String>;
    fn activate(&mut self, task: TaskId) -> Result<(), String>;
    fn pause(&mut self, task: TaskId) -> Result<(), String>;
    fn resume(&mut self, task: TaskId) -> Result<(), String>;
    fn state(&self, task: TaskId) -> EngineState;
    fn progress(&self, task: TaskId) -> Option<EngineProgress>;
    fn remove(&mut self, task: TaskId, delete_files: bool) -> Result<(), String>;
    fn shutdown(&mut self);
}

/// librqbit 9.1.0 实现。
pub struct RqbitEngine {
    rt: tokio::runtime::Runtime,
    session: Arc<Session>,
    tasks: Mutex<HashMap<TaskId, Task>>,
    next_id: AtomicU64,
    download_dir: PathBuf,
}

impl RqbitEngine {
    pub fn new(opts: EngineOptions) -> Result<Self, String> {
        std::fs::create_dir_all(&opts.download_dir)
            .map_err(|e| format!("创建下载目录失败 {}: {e}", opts.download_dir.display()))?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| format!("构建 Tokio 运行时失败: {e}"))?;

        let mut so = SessionOptions {
            dht: None,
            disable_trackers: !opts.enable_trackers,
            disable_local_service_discovery: true,
            listen: None,
            // rqbit 构造连接器时要求至少启用一种传输；这里恒定启用 TCP 出站，
            // 入站是否可达由 `listen` 决定，两者不可混为一谈。
            connect: Some(ConnectionOptions {
                enable_tcp: true,
                ..Default::default()
            }),
            ipv4_only: opts.ipv4_only,
            client_name_and_version: Some(format!("ColdRevive/{}", env!("CARGO_PKG_VERSION"))),
            ..Default::default()
        };
        if let Some(addr) = opts.listen {
            so.listen = Some(ListenerOptions {
                mode: ListenerMode::TcpOnly,
                listen_addr: addr,
                enable_upnp_port_forwarding: false,
                ipv4_only: opts.ipv4_only,
                ..Default::default()
            });
        }
        if opts.enable_dht {
            so.dht = Some(librqbit::DhtSessionConfig::default());
        }
        if let Some(u) = opts.upload_bps {
            so.ratelimits.upload_bps = std::num::NonZeroU32::new(u);
        }
        if let Some(d) = opts.download_bps {
            so.ratelimits.download_bps = std::num::NonZeroU32::new(d);
        }

        let dir = opts.download_dir.clone();
        let session = rt
            .block_on(async { Session::new_with_opts(dir, so).await })
            .map_err(|e| format!("创建 BT 会话失败: {e}"))?;
        Ok(Self {
            rt,
            session,
            tasks: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            download_dir: opts.download_dir,
        })
    }

    pub fn download_dir(&self) -> &Path {
        &self.download_dir
    }

    /// 本会话实际监听地址；`None` 表示没有入站端口。
    pub fn listen_addr(&self) -> Option<SocketAddr> {
        self.session.listen_addr()
    }

    pub fn info_hash(&self, task: TaskId) -> Option<String> {
        self.tasks.lock().unwrap().get(&task).map(|t| t.info_hash.clone())
    }

    pub fn add_torrent_with_timeout(&mut self, spec: &TorrentSpec<'_>, init_timeout: Duration) -> Result<TaskId, String> {
        let out_dir = spec.output_dir.clone().unwrap_or_else(|| self.download_dir.clone());
        std::fs::create_dir_all(&out_dir)
            .map_err(|e| format!("创建任务目录失败 {}: {e}", out_dir.display()))?;

        let add_opts = AddTorrentOptions {
            paused: spec.paused,
            overwrite: spec.overwrite,
            disable_trackers: spec.disable_trackers,
            output_folder: Some(out_dir.to_string_lossy().into_owned()),
            initial_peers: if spec.initial_peers.is_empty() {
                None
            } else {
                Some(spec.initial_peers.clone())
            },
            ..Default::default()
        };
        let session = self.session.clone();
        let bytes = bytes::Bytes::copy_from_slice(spec.bytes);
        let fut = async move {
            let resp = session
                .add_torrent(AddTorrent::TorrentFileBytes(bytes), Some(add_opts))
                .await
                .map_err(|e| format!("add_torrent: {e}"))?;
            let handle = resp
                .into_handle()
                .ok_or_else(|| "add_torrent 只返回了列表（list_only），没有可管理句柄".to_string())?;
            handle
                .wait_until_initialized()
                .await
                .map_err(|e| format!("初始化任务失败: {e}"))?;
            Ok::<_, String>(handle)
        };
        let handle = self
            .rt
            .block_on(async { tokio::time::timeout(init_timeout, fut).await })
            .map_err(|_| format!("初始化超时（{}s）", init_timeout.as_secs()))??;

        let info_hash = handle.info_hash().as_string();
        let name = handle.name().unwrap_or_default();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.tasks.lock().unwrap().insert(
            id,
            Task { handle, info_hash, name },
        );
        Ok(id)
    }

    /// 种子内嵌的展示名（来自元数据，不是调用方给的 URI）。
    pub fn name(&self, task: TaskId) -> Option<String> {
        self.tasks.lock().unwrap().get(&task).map(|t| t.name.clone())
    }

    /// 采样一次；任务不在引擎里返回 `None`（调用方据此如实报 error，而不是编状态）。
    pub fn sample(&self, task: TaskId) -> Option<EngineProgress> {
        let h = { self.tasks.lock().unwrap().get(&task).map(|t| t.handle.clone()) }?;
        let s = h.stats();
        let state = match s.state {
            TorrentStatsState::Live => EngineState::Hot,
            TorrentStatsState::Paused => EngineState::Cold,
            TorrentStatsState::Initializing { .. } => EngineState::Waking,
            TorrentStatsState::Error => EngineState::Failed,
        };
        let peers = s
            .live
            .as_ref()
            .map(|l| l.snapshot.peer_stats.live as usize)
            .unwrap_or(0);
        let download_mbps = s.live.as_ref().map(|l| l.download_speed.mbps).unwrap_or(0.0);
        Some(EngineProgress {
            state,
            downloaded_bytes: s.progress_bytes,
            total_bytes: s.total_bytes,
            uploaded_bytes: s.uploaded_bytes,
            peers,
            finished: s.finished,
            download_mbps,
            error: s.error.clone(),
        })
    }

    pub fn pause(&self, task: TaskId) -> Result<(), String> {
        let h = { self.tasks.lock().unwrap().get(&task).map(|t| t.handle.clone()) }
            .ok_or_else(|| format!("任务 {task} 不存在"))?;
        self.rt
            .block_on(async { self.session.pause(&h).await })
            .map_err(|e| format!("pause: {e}"))
    }

    pub fn resume(&self, task: TaskId) -> Result<(), String> {
        let h = { self.tasks.lock().unwrap().get(&task).map(|t| t.handle.clone()) }
            .ok_or_else(|| format!("任务 {task} 不存在"))?;
        self.rt
            .block_on(async { self.session.unpause(&h).await })
            .map_err(|e| format!("resume: {e}"))
    }

    pub fn remove(&mut self, task: TaskId, delete_files: bool) -> Result<(), String> {
        let t = self
            .tasks
            .lock()
            .unwrap()
            .remove(&task)
            .ok_or_else(|| format!("任务 {task} 不存在"))?;
        let id = TorrentIdOrHash::Id(t.handle.id());
        self.rt
            .block_on(async { self.session.delete(id, delete_files).await })
            .map_err(|e| format!("delete: {e}"))
    }

    /// 停会话并回收运行时。重复调用安全（第二次调用时任务表已空）。
    pub fn shutdown(&mut self) {
        self.tasks.lock().unwrap().clear();
        self.rt.block_on(async { self.session.stop().await });
    }
}

impl Engine for RqbitEngine {
    fn add_torrent(&mut self, spec: &TorrentSpec<'_>) -> Result<TaskId, String> {
        RqbitEngine::add_torrent_with_timeout(self, spec, Duration::from_secs(60))
    }

    fn activate(&mut self, task: TaskId) -> Result<(), String> {
        // 冷态任务被唤醒：从暂停转运行。引擎不检查当前状态，重复 unpause 由 librqbit 自己幂等处理。
        self.resume(task)
    }

    fn pause(&mut self, task: TaskId) -> Result<(), String> {
        RqbitEngine::pause(self, task)
    }

    fn resume(&mut self, task: TaskId) -> Result<(), String> {
        RqbitEngine::resume(self, task)
    }

    fn state(&self, task: TaskId) -> EngineState {
        self.sample(task).map(|p| p.state).unwrap_or(EngineState::Stopped)
    }

    fn progress(&self, task: TaskId) -> Option<EngineProgress> {
        self.sample(task)
    }

    fn remove(&mut self, task: TaskId, delete_files: bool) -> Result<(), String> {
        RqbitEngine::remove(self, task, delete_files)
    }

    fn shutdown(&mut self) {
        RqbitEngine::shutdown(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "cr-engine-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn state_names_are_stable() {
        // 名字进 aria2 兼容层与报告，不能随手改。
        assert_eq!(EngineState::Cold.as_str(), "cold");
        assert_eq!(EngineState::Hot.to_string(), "hot");
    }

    #[test]
    fn engine_starts_without_listening() {
        let dir = tmp_dir("nolsn");
        let mut e = RqbitEngine::new(EngineOptions::new(&dir)).expect("engine must start");
        assert!(e.listen_addr().is_none(), "默认不得监听任何入站端口");
        assert_eq!(e.state(999), EngineState::Stopped);
        assert!(e.progress(999).is_none());
        e.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn garbage_torrent_is_rejected() {
        let dir = tmp_dir("garbage");
        let mut e = RqbitEngine::new(EngineOptions::new(&dir)).expect("engine must start");
        let spec = TorrentSpec::new("bad", b"this is not a torrent");
        let err = Engine::add_torrent(&mut e, &spec).expect_err("垃圾字节必须失败");
        assert!(!err.is_empty(), "错误必须带原因，不能是空串");
        e.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pause_on_unknown_task_errors() {
        let dir = tmp_dir("unknown");
        let mut e = RqbitEngine::new(EngineOptions::new(&dir)).expect("engine must start");
        assert!(RqbitEngine::pause(&e, 42).is_err());
        assert!(RqbitEngine::resume(&e, 42).is_err());
        e.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }
}

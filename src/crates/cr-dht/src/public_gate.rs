//! 公共 DHT 宣告闸门（DOC-35 决定 3 选 B）。
//!
//! 背景：`announce_peer` 是**对外、不可撤回**的副作用——记录一旦扩散到公共 DHT，
//! 撤回只是「以后不发了」，已经拿到记录的人不受影响。因此所有者授权的是
//! 「机制可以存在」，不是「默认开」：
//!
//! 1. **默认关闭**：`GateConfig::enabled = false` 时任何 key 都不放行；
//! 2. **显式开启 + 限速**：每分钟窗口上限 + 每日总量上限，两道都过才放行；
//! 3. **运行时可一键关闭**：`kill()` 立即生效，`revive()` 需要重新显式调用；
//! 4. **审计留痕**：每次放行与每次拒绝都留一条（时间、infohash、原因），有界环形缓冲。
//!
//! 本模块**不**决定「谁是公共节点」——那由配置的 bootstrap 列表决定；这里只管
//! 「允不允许发、发了多少」。也不产出任何公网可达性结论。

use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// 审计环形缓冲容量（每条 ~80 B，够回答「刚才到底发了什么」）。
pub const AUDIT_CAP: usize = 16;

/// 闸门参数。默认值即「关着」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateConfig {
    pub enabled: bool,
    /// 每分钟最多放行多少个 key（固定窗口，不是令牌桶；文档里按固定窗口写）。
    pub max_per_min: u64,
    /// 每日（UTC）最多放行多少个 key。0 表示不设日上限。
    pub daily_cap: u64,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self { enabled: false, max_per_min: 4, daily_cap: 200 }
    }
}

/// 拒绝原因。**每一种都要能写进日志**，不能笼统写「被拒绝」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// 配置未开启公共宣告。
    Disabled,
    /// 运行时被一键关闭。
    Killed,
    /// 本分钟窗口已满。
    MinuteRateLimit,
    /// 当日总量已达上限。
    DailyCap,
}

impl Refusal {
    pub fn label(self) -> &'static str {
        match self {
            Refusal::Disabled => "disabled",
            Refusal::Killed => "killed",
            Refusal::MinuteRateLimit => "minute_rate_limit",
            Refusal::DailyCap => "daily_cap",
        }
    }
}

/// 一条审计记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// Unix 秒。
    pub at: u64,
    /// 被放行 / 被拒绝的 infohash（小写 hex，40 字符）。
    pub info_hash: String,
    /// `None` = 放行；`Some(reason)` = 拒绝。
    pub refused: Option<&'static str>,
}

/// 只读快照，供 `/health` 与 `/cli dht announce-status` 使用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateSnapshot {
    pub enabled: bool,
    pub killed: bool,
    pub max_per_min: u64,
    pub daily_cap: u64,
    /// 本分钟窗口已用配额。
    pub used_this_minute: u64,
    /// 当日已用配额。
    pub used_today: u64,
    /// 进程启动以来放行总数。
    pub sent_total: u64,
    /// 进程启动以来拒绝总数（按原因分项也给了）。
    pub refused_total: u64,
    pub refused_disabled: u64,
    pub refused_killed: u64,
    pub refused_minute: u64,
    pub refused_daily: u64,
    pub last_refusal: Option<&'static str>,
    pub audit: Vec<AuditEntry>,
}

#[derive(Debug)]
struct State {
    minute: u64,
    used_minute: u64,
    day: u64,
    used_day: u64,
    killed: bool,
    sent_total: u64,
    refused_total: u64,
    refused_disabled: u64,
    refused_killed: u64,
    refused_minute: u64,
    refused_daily: u64,
    last_refusal: Option<&'static str>,
    audit: VecDeque<AuditEntry>,
}

/// 公共宣告闸门。线程安全：宣告线程写、`/health` 与 `/cli` 读。
pub struct PublicAnnounceGate {
    cfg: GateConfig,
    inner: Mutex<State>,
}

impl PublicAnnounceGate {
    pub fn new(cfg: GateConfig) -> Self {
        Self {
            cfg,
            inner: Mutex::new(State {
                minute: 0,
                used_minute: 0,
                day: 0,
                used_day: 0,
                killed: false,
                sent_total: 0,
                refused_total: 0,
                refused_disabled: 0,
                refused_killed: 0,
                refused_minute: 0,
                refused_daily: 0,
                last_refusal: None,
                audit: VecDeque::new(),
            }),
        }
    }

    pub fn config(&self) -> GateConfig {
        self.cfg
    }

    /// 试放行一个 key：成功则**消耗**配额并留审计；失败则如实记原因，不消耗配额。
    pub fn try_acquire(&self, info_hash: &[u8; 20], now: SystemTime) -> Result<(), Refusal> {
        let secs = now.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let mut st = self.inner.lock().unwrap();
        // 时间窗口滚动：分钟与 UTC 日分别重置。
        let minute = secs / 60;
        if st.minute != minute {
            st.minute = minute;
            st.used_minute = 0;
        }
        let day = secs / 86_400;
        if st.day != day {
            st.day = day;
            st.used_day = 0;
        }

        let refusal = if !self.cfg.enabled {
            Some(Refusal::Disabled)
        } else if st.killed {
            Some(Refusal::Killed)
        } else if st.used_minute >= self.cfg.max_per_min {
            Some(Refusal::MinuteRateLimit)
        } else if self.cfg.daily_cap > 0 && st.used_day >= self.cfg.daily_cap {
            Some(Refusal::DailyCap)
        } else {
            None
        };

        let hex = hex20(info_hash);
        match refusal {
            None => {
                st.used_minute += 1;
                st.used_day += 1;
                st.sent_total += 1;
                push_audit(&mut st.audit, AuditEntry { at: secs, info_hash: hex, refused: None });
                Ok(())
            }
            Some(r) => {
                st.refused_total += 1;
                match r {
                    Refusal::Disabled => st.refused_disabled += 1,
                    Refusal::Killed => st.refused_killed += 1,
                    Refusal::MinuteRateLimit => st.refused_minute += 1,
                    Refusal::DailyCap => st.refused_daily += 1,
                }
                st.last_refusal = Some(r.label());
                push_audit(
                    &mut st.audit,
                    AuditEntry { at: secs, info_hash: hex, refused: Some(r.label()) },
                );
                Err(r)
            }
        }
    }

    /// 运行时一键关闭。幂等；返回是否发生了状态变化。
    pub fn kill(&self) -> bool {
        let mut st = self.inner.lock().unwrap();
        if st.killed {
            return false;
        }
        st.killed = true;
        true
    }

    /// 重新放行。**只有配置本身已开启时才允许**——否则「revive 成功但实际仍拒绝」会误导运维。
    pub fn revive(&self) -> Result<(), Refusal> {
        if !self.cfg.enabled {
            return Err(Refusal::Disabled);
        }
        let mut st = self.inner.lock().unwrap();
        st.killed = false;
        Ok(())
    }

    pub fn killed(&self) -> bool {
        self.inner.lock().unwrap().killed
    }

    pub fn snapshot(&self) -> GateSnapshot {
        let st = self.inner.lock().unwrap();
        GateSnapshot {
            enabled: self.cfg.enabled,
            killed: st.killed,
            max_per_min: self.cfg.max_per_min,
            daily_cap: self.cfg.daily_cap,
            used_this_minute: st.used_minute,
            used_today: st.used_day,
            sent_total: st.sent_total,
            refused_total: st.refused_total,
            refused_disabled: st.refused_disabled,
            refused_killed: st.refused_killed,
            refused_minute: st.refused_minute,
            refused_daily: st.refused_daily,
            last_refusal: st.last_refusal,
            audit: st.audit.iter().cloned().collect(),
        }
    }
}

fn push_audit(buf: &mut VecDeque<AuditEntry>, e: AuditEntry) {
    if buf.len() >= AUDIT_CAP {
        buf.pop_front();
    }
    buf.push_back(e);
}

pub fn hex20(b: &[u8; 20]) -> String {
    let mut s = String::with_capacity(40);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

/// 判定一个对端地址是否是**公共**地址——只有公共地址才需要过闸门。
///
/// 标准库的 `Ipv4Addr::is_global` 至今 unstable，所以这里显式列举「非公共」的段：
/// 回环、私有（10/8、172.16/12、192.168/16）、链路本地（169.254/16）、
/// 运营商级 NAT（100.64/10）、未指定、组播/广播。**判错方向是保守的**：
/// 拿不准就当公共（宁可多拦一次，也不要把内网测试流量误判成公共流量而放行）。
pub fn is_public_addr(addr: SocketAddr) -> bool {
    match addr.ip() {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => !(v6.is_loopback() || v6.is_unspecified() || v6.is_multicast()),
    }
}

fn is_public_v4(v4: Ipv4Addr) -> bool {
    if v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_broadcast() {
        return false;
    }
    if v4.is_multicast() || v4.is_unspecified() {
        return false;
    }
    // 100.64.0.0/10：CGNAT。用户往往在 CGNAT 后面，把它当公网会误伤自建测试节点。
    let o = v4.octets();
    if o[0] == 100 && (64..=127).contains(&o[1]) {
        return false;
    }
    // 0.0.0.0/8 与 192.0.0.0/24（协议保留），同样不算公共目标。
    if o[0] == 0 {
        return false;
    }
    if o[0] == 192 && o[1] == 0 && o[2] == 0 {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn ih(n: u8) -> [u8; 20] {
        [n; 20]
    }

    #[test]
    fn default_is_closed_and_says_why() {
        let g = PublicAnnounceGate::new(GateConfig::default());
        assert_eq!(g.try_acquire(&ih(1), at(1_000)), Err(Refusal::Disabled));
        let s = g.snapshot();
        assert!(!s.enabled);
        assert_eq!(s.sent_total, 0, "拒绝不许记成发出");
        assert_eq!(s.refused_disabled, 1);
        assert_eq!(s.last_refusal, Some("disabled"));
        assert_eq!(s.audit.len(), 1);
        assert_eq!(s.audit[0].refused, Some("disabled"));
    }

    #[test]
    fn minute_window_limits_and_rolls_over() {
        let g = PublicAnnounceGate::new(GateConfig { enabled: true, max_per_min: 2, daily_cap: 0 });
        assert!(g.try_acquire(&ih(1), at(120)).is_ok());
        assert!(g.try_acquire(&ih(2), at(121)).is_ok());
        assert_eq!(g.try_acquire(&ih(3), at(122)), Err(Refusal::MinuteRateLimit));
        // 下一分钟窗口重置：能继续发，但日计数不变。
        assert!(g.try_acquire(&ih(4), at(180)).is_ok());
        let s = g.snapshot();
        assert_eq!(s.sent_total, 3);
        assert_eq!(s.used_today, 3, "日计数跨分钟不清零");
        assert_eq!(s.used_this_minute, 1);
        assert_eq!(s.refused_minute, 1);
    }

    #[test]
    fn daily_cap_survives_minute_rollover() {
        let g = PublicAnnounceGate::new(GateConfig { enabled: true, max_per_min: 10, daily_cap: 2 });
        assert!(g.try_acquire(&ih(1), at(60)).is_ok());
        assert!(g.try_acquire(&ih(2), at(61)).is_ok());
        // 换一分钟、换一小时都不该复活配额；只有跨 UTC 日才重置。
        assert_eq!(g.try_acquire(&ih(3), at(3_600)), Err(Refusal::DailyCap));
        assert!(g.try_acquire(&ih(4), at(86_400 + 5)).is_ok(), "跨日必须重置");
        assert_eq!(g.snapshot().refused_daily, 1);
    }

    #[test]
    fn kill_switch_is_immediate_and_revive_needs_authorization() {
        let g = PublicAnnounceGate::new(GateConfig { enabled: true, max_per_min: 10, daily_cap: 0 });
        assert!(g.kill());
        assert!(!g.kill(), "重复关闭不该报变化");
        assert_eq!(g.try_acquire(&ih(1), at(60)), Err(Refusal::Killed));
        assert!(g.revive().is_ok());
        assert!(g.try_acquire(&ih(2), at(61)).is_ok());
        assert_eq!(g.snapshot().refused_killed, 1);

        // 配置未开启时 revive 必须失败，不能给出「已恢复」的假象。
        let closed = PublicAnnounceGate::new(GateConfig::default());
        closed.kill();
        assert_eq!(closed.revive(), Err(Refusal::Disabled));
        assert!(closed.killed(), "revive 失败后必须仍然是关着的");
    }

    #[test]
    fn public_addr_classifier_refuses_to_call_private_things_public() {
        use std::net::SocketAddr;
        let pub_ip: SocketAddr = "1.2.3.4:6881".parse().unwrap();
        assert!(is_public_addr(pub_ip));
        assert!(is_public_addr("119.29.150.254:7100".parse().unwrap()));
        for a in [
            "127.0.0.1:6881",
            "10.207.0.2:7101",
            "172.16.5.5:6881",
            "192.168.1.221:6881",
            "169.254.1.1:6881",
            "100.64.0.7:6881",
            "0.0.0.0:6881",
            "224.0.0.1:6881",
            "[::1]:6881",
        ] {
            let sa: SocketAddr = a.parse().unwrap();
            assert!(!is_public_addr(sa), "{a} 不该被当作公共地址");
        }
    }

    #[test]
    fn audit_ring_is_bounded_and_keeps_the_tail() {
        let g = PublicAnnounceGate::new(GateConfig { enabled: true, max_per_min: 100, daily_cap: 0 });
        for i in 0..(AUDIT_CAP as u64 + 5) {
            let _ = g.try_acquire(&ih(i as u8), at(60 + i));
        }
        let s = g.snapshot();
        assert_eq!(s.audit.len(), AUDIT_CAP, "审计必须是有界缓冲");
        assert_eq!(s.audit.last().unwrap().info_hash, hex20(&ih((AUDIT_CAP + 4) as u8)),
            "保留尾部，不能留最旧的");
        assert_eq!(s.sent_total, AUDIT_CAP as u64 + 5, "总数不受缓冲影响");
    }
}

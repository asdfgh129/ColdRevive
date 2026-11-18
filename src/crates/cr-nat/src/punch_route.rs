//! 打洞优先 / 中继兜底的**双端协商判据**（ADR-022）。
//!
//! 这不是「我发出 SYN 就算打通」——那正是本项目反复踩过的假阳性（SYN_SENT 时
//! `peer_addr()` 就返回目标）。这里要求三件事同时成立才允许走直连：
//!
//! 1. 本侧在已建立 socket 上做完 nonce 握手，确认连到的是**这次会话的对端**；
//! 2. 对端报告它自己也完成了同样的握手（第一轮 `Local`）；
//! 3. 双方都报告「我接受直连」（第二轮 `Commit`），且两轮的 nonce 一致、来源不是自己。
//!
//! 为什么需要第二轮：只有第一轮时会出现「A 验证成功、B 验证失败 → A 走直连、
//! B 留中继」的双向黑洞。第二轮把各自的**决定**也交换一次，任何一方不接受，
//! 双方都落回中继。这是两阶段提交，不是性能指标。
//!
//! 本模块是纯判据 + 定长线格式，std-only，不碰 socket；握手字节怎么来回由调用方负责。
//! 任何未知字段、错序、nonce 不符、自身回显都** fail-closed 到 Relay**，绝不静默放行。

/// 一条裁决的线格式：`16B 来源 id + 16B 会话 nonce + 1B 阶段 + 1B 是否成立`。
pub const VERDICT_LEN: usize = 16 + 16 + 1 + 1;
/// 握手帧：对端必须原样回显本端的 `side || nonce` 才算验明身份。
pub const HANDSHAKE_LEN: usize = 16 + 16;

const STAGE_LOCAL: u8 = 1;
const STAGE_COMMIT: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Local,
    Commit,
}

impl Stage {
    fn code(self) -> u8 {
        match self {
            Stage::Local => STAGE_LOCAL,
            Stage::Commit => STAGE_COMMIT,
        }
    }
    fn from_code(c: u8) -> Option<Self> {
        match c {
            STAGE_LOCAL => Some(Stage::Local),
            STAGE_COMMIT => Some(Stage::Commit),
            _ => None,
        }
    }
}

/// 最终路由。只有 `Direct` 允许把 punched socket 交给引擎；
/// `Relay` 表示回落到中继；`Pending` 表示协商还没走完，不得据此发起任何数据面动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Direct,
    Relay,
    Pending,
}

impl Route {
    pub fn label(self) -> &'static str {
        match self {
            Route::Direct => "punched-direct",
            Route::Relay => "relay-fallback",
            Route::Pending => "pending",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegotiateError {
    /// 帧长不对，不猜内容。
    BadLength,
    /// 阶段字节未知（旧版或篡改），不按任一阶段解释。
    UnknownStage,
    /// nonce 不属于这次会话：可能是上一轮残留或重放。
    NonceMismatch,
    /// 来源 id 等于本端 id：自己回显自己，不能当对端确认。
    SelfEcho,
    /// Commit 先于 Local 到达：拿不到「对端是否看见我的 Local」这个前提，
    /// 只能当无效信息处理；随后超时会自然落到 Relay。
    OutOfOrder,
}

/// 一条可上线的裁决。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub side: [u8; 16],
    pub nonce: [u8; 16],
    pub stage: Stage,
    pub ok: bool,
}

impl Verdict {
    pub fn encode(&self) -> [u8; VERDICT_LEN] {
        let mut b = [0u8; VERDICT_LEN];
        b[..16].copy_from_slice(&self.side);
        b[16..32].copy_from_slice(&self.nonce);
        b[32] = self.stage.code();
        b[33] = if self.ok { 1 } else { 0 };
        b
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, NegotiateError> {
        if bytes.len() != VERDICT_LEN {
            return Err(NegotiateError::BadLength);
        }
        let mut side = [0u8; 16];
        let mut nonce = [0u8; 16];
        side.copy_from_slice(&bytes[..16]);
        nonce.copy_from_slice(&bytes[16..32]);
        let stage = Stage::from_code(bytes[32]).ok_or(NegotiateError::UnknownStage)?;
        // 只认 0/1：0x02 这类「看起来像 true」的值必须拒绝，不能按非零点当作成立。
        let ok = match bytes[33] {
            0 => false,
            1 => true,
            _ => return Err(NegotiateError::BadLength),
        };
        Ok(Verdict { side, nonce, stage, ok })
    }
}

/// 握手帧：把 `side || nonce` 发给对端，期待它原样回显另一侧的 id 与同一 nonce。
pub fn handshake_frame(side: &[u8; 16], nonce: &[u8; 16]) -> [u8; HANDSHAKE_LEN] {
    let mut b = [0u8; HANDSHAKE_LEN];
    b[..16].copy_from_slice(side);
    b[16..].copy_from_slice(nonce);
    b
}

/// 校验对端握手帧：`nonce` 必须一致，`side` 必须不是本端（防自己回显自己）。
pub fn validate_handshake(
    frame: &[u8],
    local_side: &[u8; 16],
    nonce: &[u8; 16],
) -> Result<[u8; 16], NegotiateError> {
    if frame.len() != HANDSHAKE_LEN {
        return Err(NegotiateError::BadLength);
    }
    let mut peer_side = [0u8; 16];
    peer_side.copy_from_slice(&frame[..16]);
    if &frame[16..] != nonce {
        return Err(NegotiateError::NonceMismatch);
    }
    if &peer_side == local_side {
        return Err(NegotiateError::SelfEcho);
    }
    Ok(peer_side)
}

/// 双端协商状态机。
///
/// 典型用法（双方对称执行）：
/// ```text
/// let mut n = RouteNegotiator::new(local_id, nonce, local_handshake_ok);
/// // 第一轮：交换 n.verdict()（Stage::Local）
/// n.observe(peer_local_verdict)?;
/// // 第二轮：交换 n.verdict()（Stage::Commit）
/// n.observe(peer_commit_verdict)?;
/// let route = n.route();  // Direct 或 Relay，永不 Pending（除非未交换完）
/// ```
#[derive(Debug, Clone)]
pub struct RouteNegotiator {
    local_side: [u8; 16],
    nonce: [u8; 16],
    local_ok: bool,
    peer_local_ok: Option<bool>,
    peer_commit_ok: Option<bool>,
}

impl RouteNegotiator {
    pub fn new(local_side: [u8; 16], nonce: [u8; 16], local_ok: bool) -> Self {
        Self {
            local_side,
            nonce,
            local_ok,
            peer_local_ok: None,
            peer_commit_ok: None,
        }
    }

    /// 本端此刻应收到的那一轮裁决。Local 阶段报本侧握手结果；
    /// 收到对端 Local 后才进入 Commit，报本端**是否接受直连**。
    pub fn verdict(&self) -> Verdict {
        let (stage, ok) = if self.peer_local_ok.is_none() {
            (Stage::Local, self.local_ok)
        } else {
            (Stage::Commit, self.tentative_direct())
        };
        Verdict {
            side: self.local_side,
            nonce: self.nonce,
            stage,
            ok,
        }
    }

    /// 是否还需要再交换一轮 Commit。`route()` 为 Pending 且本轮 Local 已到时为真。
    pub fn needs_commit(&self) -> bool {
        self.peer_local_ok.is_some() && self.peer_commit_ok.is_none()
    }

    /// 本端是否倾向直连：这是第二轮要报出去的值，不是最终决定。
    fn tentative_direct(&self) -> bool {
        self.local_ok && self.peer_local_ok == Some(true)
    }

    /// 收下对端裁决。校验全 failing-closed；同一阶段重复到达按幂等处理（同值接受，
    /// 矛盾值拒绝，防止把过期帧当新证据）。
    pub fn observe(&mut self, v: Verdict) -> Result<(), NegotiateError> {
        if &v.nonce != &self.nonce {
            return Err(NegotiateError::NonceMismatch);
        }
        if &v.side == &self.local_side {
            return Err(NegotiateError::SelfEcho);
        }
        match v.stage {
            Stage::Local => {
                if self.peer_commit_ok.is_some() {
                    // 已经进到 Commit 又来 Local：过期帧，忽略而不是覆盖结论。
                    return Err(NegotiateError::OutOfOrder);
                }
                match self.peer_local_ok {
                    Some(prev) if prev != v.ok => Err(NegotiateError::OutOfOrder),
                    _ => {
                        self.peer_local_ok = Some(v.ok);
                        Ok(())
                    }
                }
            }
            Stage::Commit => {
                // Commit 必须建立在「已见到对端 Local」之上，否则拿不到双方确认的前提。
                if self.peer_local_ok.is_none() {
                    return Err(NegotiateError::OutOfOrder);
                }
                match self.peer_commit_ok {
                    Some(prev) if prev != v.ok => Err(NegotiateError::OutOfOrder),
                    _ => {
                        self.peer_commit_ok = Some(v.ok);
                        Ok(())
                    }
                }
            }
        }
    }

    /// 当前路由。任一端没完成握手、任一端不接受、任何错序/错 nonce → Relay。
    /// 只有两轮都到齐且双方都 ok 才是 Direct。
    pub fn route(&self) -> Route {
        let Some(peer_commit) = self.peer_commit_ok else {
            return Route::Pending;
        };
        if self.tentative_direct() && peer_commit {
            Route::Direct
        } else {
            Route::Relay
        }
    }

    /// 协商是否已收敛（可以据此做数据面动作）。
    pub fn settled(&self) -> bool {
        self.route() != Route::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(b: u8) -> [u8; 16] {
        [b; 16]
    }

    fn pair(local_ok: bool, peer_ok: bool) -> (RouteNegotiator, RouteNegotiator) {
        let nonce = [7u8; 16];
        let mut a = RouteNegotiator::new(id(1), nonce, local_ok);
        let mut b = RouteNegotiator::new(id(2), nonce, peer_ok);
        // 第一轮：交换 Local
        let av = a.verdict();
        let bv = b.verdict();
        assert_eq!(av.stage, Stage::Local);
        assert_eq!(bv.stage, Stage::Local);
        a.observe(bv).unwrap();
        b.observe(av).unwrap();
        // 第二轮：交换 Commit
        let av2 = a.verdict();
        let bv2 = b.verdict();
        assert_eq!(av2.stage, Stage::Commit);
        assert_eq!(bv2.stage, Stage::Commit);
        a.observe(bv2).unwrap();
        b.observe(av2).unwrap();
        (a, b)
    }

    #[test]
    fn both_verified_converge_to_direct() {
        let (a, b) = pair(true, true);
        assert_eq!(a.route(), Route::Direct);
        assert_eq!(b.route(), Route::Direct);
        assert!(a.settled() && b.settled());
    }

    #[test]
    fn one_sided_success_falls_back_to_relay_on_both_sides() {
        // 这是防双向黑洞的核心断言：A 握手成功、B 失败，两端必须都落 Relay。
        let (a, b) = pair(true, false);
        assert_eq!(a.route(), Route::Relay, "本端成功但 Peer 失败，不得独自走直连");
        assert_eq!(b.route(), Route::Relay, "本端失败必须明确拒绝直连");
    }

    #[test]
    fn mirror_case_also_falls_back() {
        let (a, b) = pair(false, true);
        assert_eq!(a.route(), Route::Relay);
        assert_eq!(b.route(), Route::Relay);
    }

    #[test]
    fn both_failed_is_relay() {
        let (a, b) = pair(false, false);
        assert_eq!(a.route(), Route::Relay);
        assert_eq!(b.route(), Route::Relay);
    }

    #[test]
    fn pending_never_authorizes_direct() {
        let n = RouteNegotiator::new(id(1), [3u8; 16], true);
        assert_eq!(n.route(), Route::Pending);
        assert!(!n.settled());
        let peer_local = Verdict { side: id(2), nonce: [3u8; 16], stage: Stage::Local, ok: true };
        let mut n = n;
        n.observe(peer_local).unwrap();
        assert_eq!(n.route(), Route::Pending, "只收到第一轮 Local 时不得切直连");
        assert!(n.needs_commit());
    }

    #[test]
    fn peer_rejecting_commit_forces_relay() {
        let nonce = [9u8; 16];
        let mut a = RouteNegotiator::new(id(1), nonce, true);
        a.observe(Verdict { side: id(2), nonce, stage: Stage::Local, ok: true }).unwrap();
        // 对端第一轮说「我验证成功」，但第二轮拒绝直连（例如随后发现链路不稳）。
        a.observe(Verdict { side: id(2), nonce, stage: Stage::Commit, ok: false }).unwrap();
        assert_eq!(a.route(), Route::Relay);
    }

    #[test]
    fn nonce_mismatch_is_rejected_not_treated_as_failure_flag() {
        let mut a = RouteNegotiator::new(id(1), [1u8; 16], true);
        let stale = Verdict { side: id(2), nonce: [2u8; 16], stage: Stage::Local, ok: true };
        assert_eq!(a.observe(stale), Err(NegotiateError::NonceMismatch));
        assert_eq!(a.route(), Route::Pending, "错 nonce 不得产生任何结论");
    }

    #[test]
    fn self_echo_is_rejected() {
        let mut a = RouteNegotiator::new(id(1), [1u8; 16], true);
        let echo = Verdict { side: id(1), nonce: [1u8; 16], stage: Stage::Local, ok: true };
        assert_eq!(a.observe(echo), Err(NegotiateError::SelfEcho));
    }

    #[test]
    fn commit_before_local_is_out_of_order() {
        let mut a = RouteNegotiator::new(id(1), [1u8; 16], true);
        let early = Verdict { side: id(2), nonce: [1u8; 16], stage: Stage::Commit, ok: true };
        assert_eq!(a.observe(early), Err(NegotiateError::OutOfOrder));
        assert_eq!(a.route(), Route::Pending);
    }

    #[test]
    fn verdict_wire_roundtrip_and_strict_decode() {
        let v = Verdict { side: id(4), nonce: [5u8; 16], stage: Stage::Commit, ok: true };
        assert_eq!(Verdict::decode(&v.encode()), Ok(v));
        assert_eq!(Verdict::decode(&[0u8; VERDICT_LEN]), Err(NegotiateError::UnknownStage));
        let mut bad = v.encode();
        bad[33] = 2; // 非 0/1 的 ok 字节
        assert_eq!(Verdict::decode(&bad), Err(NegotiateError::BadLength));
        assert_eq!(Verdict::decode(&bad[..VERDICT_LEN - 1]), Err(NegotiateError::BadLength));
    }

    #[test]
    fn handshake_validates_nonce_and_distinct_peer() {
        let frame = handshake_frame(&id(2), &[8u8; 16]);
        assert_eq!(validate_handshake(&frame, &id(1), &[8u8; 16]), Ok(id(2)));
        assert_eq!(
            validate_handshake(&frame, &id(1), &[9u8; 16]),
            Err(NegotiateError::NonceMismatch)
        );
        // 本端 id 回显：自己不能证明自己是「对端」。
        let echo = handshake_frame(&id(1), &[8u8; 16]);
        assert_eq!(validate_handshake(&echo, &id(1), &[8u8; 16]), Err(NegotiateError::SelfEcho));
    }

    #[test]
    fn route_labels_are_machine_readable() {
        assert_eq!(Route::Direct.label(), "punched-direct");
        assert_eq!(Route::Relay.label(), "relay-fallback");
        assert_eq!(Route::Pending.label(), "pending");
    }
}

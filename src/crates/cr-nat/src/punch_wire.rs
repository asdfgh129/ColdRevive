//! ADR-022 wire 层：把 `punch_route` 的裁决真正放到**中继帧**上交换。
//!
//! 为什么单独一层：`punch_route` 只有纯状态机（可单测），但要证明它「能在真实
//! 中继上收敛」必须有可执行的线格式与收发循环。放在 `cr-nat` 而不是守护进程，
//! 是为了让集成测试能直接起一个真的 `RelayServer` 跑双端，而不是在二进制里
//! 靠肉眼读日志。
//!
//! 三个写死的边界：
//! 1. **只做判据与交接**：是否打通由 `tcp_punch` 的 `poll+SO_ERROR` 决定；这里
//!    只把「本端握手结果 + 对端两轮裁决」汇总成 `Route`。
//! 2. **中继没有 BT 数据面**：`Route::Relay` 表示不登记 punched socket，保持引擎
//!    原有出向行为；它**不意味着**取片走中继。
//! 3. **线格式不新增帧类型**：`RELAY` 帧负载 = 一条 `Verdict::encode()`（定长 34B），
//!    目标 id 在帧头；服务端转发时把**来源**放进 `DELIVERED.frame.node`，`payload`
//!    是原样的裁决。绝不能在负载里再拼一份来源 id——那会和 `frame.node` 重复，
//!    解出来的 `side/nonce/stage` 全错位，而错位后 `observe()` 会以 `NonceMismatch`
//!    静默回落，看起来像「对端不支持」，极难查。

use crate::punch_route::{
    handshake_frame, validate_handshake, NegotiateError, RouteNegotiator, Verdict, HANDSHAKE_LEN,
    VERDICT_LEN,
};
use crate::relay::{ephemeral_session_id, FrameKind, RelayConn, RelayFrame, NODE_ID_LEN};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// 一次协商的会话 id：由 `ephemeral_session_id` 生成，避免同一节点连续两次
/// 唤醒复用 nonce（复用会让上一轮的迟到帧被当成这一轮的确认）。
pub fn session_nonce() -> [u8; NODE_ID_LEN] {
    ephemeral_session_id(&[0u8; NODE_ID_LEN])
}

/// 本地握手：在一条**已由 `tcp_punch` 判定 established 的 socket** 上互换身份帧。
///
/// 返回 `Ok(peer_id)` 只表示「连到的确实是持有该 node id 的对端」，**不表示**
/// 数据面已可用，也不表示对端也认了这次协商——那是第二轮裁决的事。
pub fn verify_peer(
    stream: &mut TcpStream,
    local_id: &[u8; NODE_ID_LEN],
    nonce: &[u8; NODE_ID_LEN],
    timeout: Duration,
) -> Result<[u8; NODE_ID_LEN], String> {
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("set_read_timeout: {e}"))?;
    stream
        .write_all(&handshake_frame(local_id, nonce))
        .map_err(|e| format!("写握手帧: {e}"))?;
    stream.flush().map_err(|e| format!("flush: {e}"))?;
    let mut frame = [0u8; HANDSHAKE_LEN];
    stream
        .read_exact(&mut frame)
        .map_err(|e| format!("读握手帧: {e}"))?;
    validate_handshake(&frame, local_id, nonce).map_err(|e| match e {
        NegotiateError::NonceMismatch => "握手 nonce 不符（可能是对端残留会话）".to_string(),
        NegotiateError::SelfEcho => "握手回显本端 id（不能自证对端）".to_string(),
        _ => format!("握手非法: {e:?}"),
    })
}

/// 从 `DELIVERED` 的 `payload` 取出定长裁决。
///
/// 服务端布局是 `node = source`、`payload = 原始消息`，所以这里**只**认恰好
/// `VERDICT_LEN` 字节；带前缀或其它长度一律视为非法，不猜。
pub fn decode_verdict_payload(payload: &[u8]) -> Option<Verdict> {
    if payload.len() != VERDICT_LEN {
        return None;
    }
    Verdict::decode(payload).ok()
}

/// 收一条 `DELIVERED`（跳过其它控制帧）。
fn recv_delivered(client: &mut RelayConn) -> Result<RelayFrame, String> {
    loop {
        let frame = client
            .recv_frame()
            .map_err(|e| format!("收裁决失败（超时按 Relay 处理）: {e}"))?;
        if frame.kind == FrameKind::Delivered {
            return Ok(frame);
        }
    }
}

/// 双端协商的**发起侧**：两轮裁决在同一中继连接上交换，返回对端两轮裁决。
///
/// 调用方拿到两个裁决后再喂给 `RouteNegotiator`（或直接用 `negotiate_route`），
/// 以保证「谁算路由」只有一份实现。
pub fn exchange_verdicts(
    client: &mut RelayConn,
    local_id: &[u8; NODE_ID_LEN],
    peer_id: &[u8; NODE_ID_LEN],
    local_ok: bool,
    nonce: &[u8; NODE_ID_LEN],
    timeout: Duration,
) -> Result<(Verdict, Verdict), String> {
    let mut neg = RouteNegotiator::new(*local_id, *nonce, local_ok);
    let mut rounds = Vec::with_capacity(2);
    for _ in 0..2 {
        client
            .send_relay(peer_id, &neg.verdict().encode())
            .map_err(|e| format!("发裁决失败: {e}"))?;
        client
            .set_read_timeout(Some(timeout))
            .map_err(|e| format!("set_read_timeout: {e}"))?;
        // 同一连接上可能先到别的控制帧（订阅线程只认 WAKEUP_*，不会抢这里的裁决）。
        // 只接受来自本次对端的 DELIVERED；别人的帧直接跳过，不能拿来凑轮数。
        let verdict = loop {
            let frame = recv_delivered(client)?;
            if &frame.node != peer_id {
                continue;
            }
            let Some(v) = decode_verdict_payload(&frame.payload) else {
                continue;
            };
            break v;
        };
        if let Err(e) = neg.observe(verdict) {
            return Err(format!("对端裁决非法: {e:?}"));
        }
        rounds.push(verdict);
    }
    Ok((rounds[0], rounds[1]))
}

/// 双端协商的**应答侧**：在同一条中继连接上回两轮裁决，并返回观察到的对端两轮裁决。
///
/// 会话 nonce 由发起方给出，本端不接受自定义；第一条 `DELIVERED` 的 `frame.node`
/// 必须与裁决自报的 `side` 一致，否则无法确认「是谁在跟我协商」。
pub fn answer_verdicts(
    client: &mut RelayConn,
    local_id: &[u8; NODE_ID_LEN],
    local_ok: bool,
    timeout: Duration,
) -> Result<(Verdict, Verdict), String> {
    client
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("set_read_timeout: {e}"))?;
    let first = recv_delivered(client)?;
    let v0 =
        decode_verdict_payload(&first.payload).ok_or_else(|| "第一轮裁决负载非法".to_string())?;
    if first.node != v0.side {
        return Err("DELIVERED 来源 id 与裁决 side 不符".into());
    }
    let initiator = v0.side;

    let mut local = RouteNegotiator::new(*local_id, v0.nonce, local_ok);
    let reply0 = local.verdict();
    client
        .send_relay(&initiator, &reply0.encode())
        .map_err(|e| format!("回第一轮: {e}"))?;

    let second = recv_delivered(client)?;
    if second.node != initiator {
        return Err("第二轮来自非协商对端".into());
    }
    let v1 =
        decode_verdict_payload(&second.payload).ok_or_else(|| "第二轮裁决负载非法".to_string())?;
    local
        .observe(v0)
        .map_err(|e| format!("对端第一轮非法: {e:?}"))?;
    local
        .observe(v1)
        .map_err(|e| format!("对端第二轮非法: {e:?}"))?;
    client
        .send_relay(&initiator, &local.verdict().encode())
        .map_err(|e| format!("回第二轮: {e}"))?;
    Ok((v0, v1))
}

/// 把「本端握手结果 + 两轮对端裁决」折算成最终路由。路由判定只有这一份实现，
/// 避免调用方各自 `observe` 后得出不同结论（一端 Direct 一端 Relay = 双向黑洞）。
pub fn negotiate_route(
    local_id: &[u8; NODE_ID_LEN],
    nonce: &[u8; NODE_ID_LEN],
    local_ok: bool,
    peer_rounds: (Verdict, Verdict),
) -> Result<crate::punch_route::Route, String> {
    let mut neg = RouteNegotiator::new(*local_id, *nonce, local_ok);
    neg.observe(peer_rounds.0)
        .map_err(|e| format!("对端第一轮非法: {e:?}"))?;
    neg.observe(peer_rounds.1)
        .map_err(|e| format!("对端第二轮非法: {e:?}"))?;
    Ok(neg.route())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::punch_route::Route;
    use crate::relay::RelayClient;
    use crate::relay_server::RelayServer;

    fn id(label: u8) -> [u8; NODE_ID_LEN] {
        let mut v = [0u8; NODE_ID_LEN];
        v[0] = label;
        v[NODE_ID_LEN - 1] = label;
        v
    }

    /// 起一个真中继，两端各自 register，然后并发跑发起/应答。
    /// 返回发起侧算出的路由；并在内部断言两端收敛到同一路由。
    fn run_pair(local_ok: bool, peer_ok: bool, nonce: [u8; NODE_ID_LEN]) -> Result<Route, String> {
        let server = RelayServer::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let handle = server.spawn();
        let addr = format!("127.0.0.1:{}", port);

        let id_a = id(b'A');
        let id_b = id(b'B');
        let timeout = Duration::from_millis(1500);

        // 应答侧先连好并 register，否则发起方的第一帧会在服务端被 dropped。
        let mut b = RelayClient::new(addr.clone()).connect(timeout).unwrap();
        b.register(&id_b).unwrap();

        let b_thread = std::thread::spawn(move || answer_verdicts(&mut b, &id_b, peer_ok, timeout));

        let mut a = RelayClient::new(addr).connect(timeout).unwrap();
        a.register(&id_a).unwrap();

        let a_rounds = exchange_verdicts(&mut a, &id_a, &id_b, local_ok, &nonce, timeout);
        let b_rounds = b_thread.join().unwrap();
        handle.stop();

        // 一端成功一端失败时，两端必须都算成 Relay；否则就是双向黑洞。
        let a_route = a_rounds.and_then(|r| negotiate_route(&id_a, &nonce, local_ok, r));
        let b_route = b_rounds.and_then(|r| negotiate_route(&id_b, &nonce, peer_ok, r));
        match (&a_route, &b_route) {
            (Ok(x), Ok(y)) => {
                assert_eq!(x, y, "两端必须收敛到同一路由，否则就是双向黑洞");
                Ok(*x)
            }
            // 任一侧失败就是失败，不能靠另一侧「看起来成功」掩盖。
            (Err(e), _) => Err(e.clone()),
            (_, Err(e)) => Err(e.clone()),
        }
    }

    #[test]
    fn both_sides_verified_converge_to_direct_over_real_relay() {
        let route = run_pair(true, true, [5u8; NODE_ID_LEN]).unwrap();
        assert_eq!(route, Route::Direct);
    }

    #[test]
    fn one_sided_success_must_fall_back_to_relay_on_both_sides() {
        // 关键反黑洞用例：A 打通了、B 没打通，不能一端 Direct 一端 Relay。
        let route = run_pair(true, false, [6u8; NODE_ID_LEN]).unwrap();
        assert_eq!(route, Route::Relay);
    }

    #[test]
    fn neither_side_verified_is_relay_not_error() {
        let route = run_pair(false, false, [7u8; NODE_ID_LEN]).unwrap();
        assert_eq!(route, Route::Relay);
    }

    #[test]
    fn payload_with_node_prefix_is_rejected() {
        // 回归：早前实现把来源 id 拼进 payload，解出来全错位。
        let v = Verdict {
            side: id(b'A'),
            nonce: [1u8; NODE_ID_LEN],
            stage: crate::punch_route::Stage::Local,
            ok: true,
        };
        let mut prefixed = id(b'A').to_vec();
        prefixed.extend_from_slice(&v.encode());
        assert!(decode_verdict_payload(&prefixed).is_none());
        assert!(decode_verdict_payload(&v.encode()).is_some());
    }

    #[test]
    fn session_nonce_differs_across_calls() {
        // 同一个节点连续两次协商不能拿到相同 nonce，否则上一轮的迟到帧会被当成本轮确认。
        let a = session_nonce();
        let b = session_nonce();
        assert_ne!(a, b, "两次协商的 nonce 相同（重放风险）");
        assert!(a.iter().any(|x| *x != 0));
    }
}

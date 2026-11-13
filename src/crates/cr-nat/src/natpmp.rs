//! NAT-PMP 客户端（RFC 6886），std-only 实现。
//!
//! 用于家庭/Apple 路由器场景的端口映射。比 UPnP 更轻量、更可预测。
//! 实现：外部地址查询（opcode 0）+ UDP/TCP 端口映射（opcode 1/2）。

use std::net::{Ipv4Addr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

pub const NATPMP_PORT: u16 = 5351;

#[derive(Debug, Clone)]
pub struct NatPmpResult {
    pub available: bool,
    pub external_ip: Option<String>,
    pub epoch_sec: Option<u32>,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct NatPmpMapping {
    pub proto: Proto,
    pub internal_port: u16,
    pub external_port: u16,
    pub lifetime_sec: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    Udp,
    Tcp,
}

pub struct NatPmpClient {
    pub gateway: String,
}

/// 解析网关地址：若显式带端口则尊重，否则追加默认 5351。
fn gateway_addr(gateway: &str) -> Result<std::net::SocketAddr, String> {
    if let Some(pos) = gateway.rfind(':') {
        let maybe_port = &gateway[pos + 1..];
        if !maybe_port.is_empty() && maybe_port.chars().all(|c| c.is_ascii_digit()) && gateway.matches(':').count() == 1 {
            return gateway
                .to_socket_addrs()
                .map_err(|e| format!("dns: {}", e))?
                .next()
                .ok_or_else(|| "gateway 无解析结果".to_string());
        }
    }
    let target = format!("{}:{}", gateway, NATPMP_PORT);
    target
        .to_socket_addrs()
        .map_err(|e| format!("dns: {}", e))?
        .next()
        .ok_or_else(|| "gateway 无解析结果".to_string())
}

impl NatPmpClient {
    pub fn new(gateway: String) -> Self {
        Self { gateway }
    }

    /// 查询网关分配的公网地址。返回 (external_ip, epoch)。
    pub fn external_address(&self, timeout: Duration) -> NatPmpResult {
        let mut out = NatPmpResult {
            available: false,
            external_ip: None,
            epoch_sec: None,
            error: String::new(),
        };
        let s = match UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => s,
            Err(e) => {
                out.error = format!("bind: {}", e);
                return out;
            }
        };
        let addr = match gateway_addr(&self.gateway) {
            Ok(a) => a,
            Err(e) => {
                out.error = e;
                return out;
            }
        };
        let _ = s.set_read_timeout(Some(timeout));
        if s.send_to(&[0u8, 0u8], addr).is_err() {
            out.error = "sendto 失败".into();
            return out;
        }
        let mut buf = [0u8; 64];
        match s.recv_from(&mut buf) {
            Ok((n, _)) => {
                if n < 12 {
                    out.error = "响应过短".into();
                    return out;
                }
                let ver = buf[0];
                let op = buf[1];
                let result = u16::from_be_bytes([buf[2], buf[3]]);
                let epoch = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
                let ext = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
                if ver != 0 || op != 128 || result != 0 {
                    out.error = format!("NAT-PMP 错误: ver={} op={} result={}", ver, op, result);
                    return out;
                }
                out.available = true;
                out.external_ip = Some(Ipv4Addr::from(ext).to_string());
                out.epoch_sec = Some(epoch);
            }
            Err(_) => {
                out.error = "timeout (NAT-PMP 未启用或被过滤)".into();
            }
        }
        out
    }

    /// 请求一个端口映射。返回协商后的映射（外部端口可能被网关改写）。
    pub fn map(
        &self,
        proto: Proto,
        internal_port: u16,
        external_port: u16,
        lifetime_sec: u32,
        timeout: Duration,
    ) -> Result<NatPmpMapping, String> {
        let s = UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| format!("bind: {}", e))?;
        let addr = gateway_addr(&self.gateway)?;
        let op_code: u8 = match proto {
            Proto::Udp => 1,
            Proto::Tcp => 2,
        };
        let mut req = vec![0u8, op_code, 0, 0];
        req.extend_from_slice(&internal_port.to_be_bytes());
        req.extend_from_slice(&external_port.to_be_bytes());
        req.extend_from_slice(&lifetime_sec.to_be_bytes());
        let _ = s.set_read_timeout(Some(timeout));
        s.send_to(&req, addr).map_err(|e| format!("sendto: {}", e))?;
        let mut buf = [0u8; 64];
        let (n, _) = s.recv_from(&mut buf).map_err(|_| "timeout 等待映射响应".to_string())?;
        if n < 16 {
            return Err("映射响应过短".into());
        }
        let resp_op = buf[1];
        let result = u16::from_be_bytes([buf[2], buf[3]]);
        // NAT-PMP 响应 opcode = 请求 opcode | 0x80
        if resp_op != (op_code + 128) || result != 0 {
            return Err(format!("映射失败: op={} result={}", resp_op, result));
        }
        let in_port = u16::from_be_bytes([buf[8], buf[9]]);
        let ex_port = u16::from_be_bytes([buf[10], buf[11]]);
        let life = u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]);
        Ok(NatPmpMapping {
            proto,
            internal_port: in_port,
            external_port: ex_port,
            lifetime_sec: life,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket;
    use std::thread;

    #[test]
    fn external_address_against_mock() {
        let srv = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = srv.local_addr().unwrap().port();
        thread::spawn(move || {
            let mut buf = [0u8; 64];
            if let Ok((n, peer)) = srv.recv_from(&mut buf) {
                if n >= 2 && buf[0] == 0 && buf[1] == 0 {
                    let mut resp = vec![0u8, 128, 0, 0];
                    resp.extend_from_slice(&1234u32.to_be_bytes()); // epoch
                    resp.extend_from_slice(&0x01020304u32.to_be_bytes()); // 1.2.3.4
                    let _ = srv.send_to(&resp, peer);
                }
            }
        });
        let c = NatPmpClient::new(format!("127.0.0.1:{}", port));
        let r = c.external_address(Duration::from_secs(2));
        assert!(r.available, "mock NAT-PMP 应可用: {}", r.error);
        assert_eq!(r.external_ip.as_deref(), Some("1.2.3.4"));
        assert_eq!(r.epoch_sec, Some(1234));
    }

    #[test]
    fn map_against_mock() {
        let srv = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = srv.local_addr().unwrap().port();
        thread::spawn(move || {
            let mut buf = [0u8; 64];
            if let Ok((n, peer)) = srv.recv_from(&mut buf) {
                if n >= 12 {
                    // 请求：ver,op,0,0,in,ex,life(4)
                    let in_port = u16::from_be_bytes([buf[4], buf[5]]);
                    let ex_port = u16::from_be_bytes([buf[6], buf[7]]);
                    let mut resp = vec![0u8, 129, 0, 0];
                    resp.extend_from_slice(&1234u32.to_be_bytes()); // epoch
                    resp.extend_from_slice(&in_port.to_be_bytes());
                    resp.extend_from_slice(&ex_port.to_be_bytes()); // 网关接受请求的外部端口
                    resp.extend_from_slice(&3600u32.to_be_bytes());
                    let _ = srv.send_to(&resp, peer);
                }
            }
        });
        let c = NatPmpClient::new(format!("127.0.0.1:{}", port));
        let m = c
            .map(Proto::Udp, 17801, 45000, 3600, Duration::from_secs(2))
            .expect("map 应成功");
        assert_eq!(m.internal_port, 17801);
        assert_eq!(m.external_port, 45000);
        assert_eq!(m.lifetime_sec, 3600);
    }

    #[test]
    fn timeout_without_server() {
        // 连一个不存在的网关，应超时而非崩溃
        let c = NatPmpClient::new("127.0.0.1:65351".into());
        let r = c.external_address(Duration::from_millis(300));
        assert!(!r.available);
    }
}

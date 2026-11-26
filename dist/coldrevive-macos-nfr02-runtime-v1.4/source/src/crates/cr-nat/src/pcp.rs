//! PCP 客户端（RFC 6887），std-only 最小实现。
//!
//! 仅实现 MAP opcode（UDP/TCP），用于支持同时具备 PCP 的路由器（如较新的 ISP CPE）。
//! 与 NAT-PMP 接口对齐，便于上层统一调用。Client IP 字段留零（服务器以源地址为准）。

use std::net::{Ipv4Addr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

pub const PCP_PORT: u16 = 5351;
const OP_MAP: u8 = 1;
#[allow(dead_code)]
const PROTO_UDP: u8 = 17;
#[allow(dead_code)]
const PROTO_TCP: u8 = 6;

#[derive(Debug, Clone)]
pub struct PcpResult {
    pub available: bool,
    pub external_ip: Option<String>,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct PcpMapping {
    pub proto: u8,
    pub internal_port: u16,
    pub external_port: u16,
    pub lifetime_sec: u32,
    pub ext_ip: Option<String>,
}

pub struct PcpClient {
    pub gateway: String,
}

/// 解析网关地址：若显式带端口则尊重，否则追加默认 5351。
fn gateway_addr(gateway: &str) -> Result<std::net::SocketAddr, String> {
    if let Some(pos) = gateway.rfind(':') {
        let maybe_port = &gateway[pos + 1..];
        if !maybe_port.is_empty()
            && maybe_port.chars().all(|c| c.is_ascii_digit())
            && gateway.matches(':').count() == 1
        {
            return gateway
                .to_socket_addrs()
                .map_err(|e| format!("dns: {}", e))?
                .next()
                .ok_or_else(|| "gateway 无解析结果".to_string());
        }
    }
    let target = format!("{}:{}", gateway, PCP_PORT);
    target
        .to_socket_addrs()
        .map_err(|e| format!("dns: {}", e))?
        .next()
        .ok_or_else(|| "gateway 无解析结果".to_string())
}

impl PcpClient {
    pub fn new(gateway: String) -> Self {
        Self { gateway }
    }

    /// 发一个 MAP 请求并解析响应。Client IP 留零。
    pub fn map(
        &self,
        proto: u8,
        internal_port: u16,
        external_port: u16,
        lifetime_sec: u32,
        timeout: Duration,
    ) -> Result<PcpMapping, String> {
        let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("bind: {}", e))?;
        let addr = gateway_addr(&self.gateway)?;

        let mut req = Vec::with_capacity(56);
        req.push(2); // PCP 协议版本
        req.push(OP_MAP);
        req.extend_from_slice(&0u32.to_be_bytes()); // Reserved + Requested Lifetime 占位（后填）
        // 先填 lifetime 到 header 的后 4 字节
        req.truncate(4);
        req.extend_from_slice(&lifetime_sec.to_be_bytes()); // Requested Lifetime
        req.extend_from_slice(&[0u8; 16]); // Client IP (128 bit)
        // MAP 特定部分
        let mut nonce = [0u8; 12];
        for (i, b) in nonce.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(3);
        }
        req.extend_from_slice(&nonce); // Mapping Nonce
        req.push(proto); // Protocol
        req.extend_from_slice(&[0u8; 3]); // Reserved
        req.extend_from_slice(&internal_port.to_be_bytes());
        req.extend_from_slice(&external_port.to_be_bytes()); // Assigned External Port（请求时建议值）
        req.extend_from_slice(&[0u8; 16]); // Assigned External IP

        let _ = s.set_read_timeout(Some(timeout));
        s.send_to(&req, addr).map_err(|e| format!("sendto: {}", e))?;
        let mut buf = [0u8; 128];
        let (n, _) = s.recv_from(&mut buf).map_err(|_| "timeout 等待 PCP 响应".to_string())?;
        if n < 56 {
            return Err(format!("PCP 响应过短: {} 字节", n));
        }
        let result = u16::from_be_bytes([buf[2], buf[3]]);
        if result != 0 {
            return Err(format!("PCP MAP 失败: result={}", result));
        }
        // 响应布局：version(1@0) op(1@1) result(2@2) lifetime(4@4) reserved(16@8)
        //           nonce(12@24) proto(1@36) reserved(3@37) internal(2@40) external(2@42) extIP(16@44)
        let life = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let in_port = u16::from_be_bytes([buf[40], buf[41]]);
        let ex_port = u16::from_be_bytes([buf[42], buf[43]]);
        let ext_ip = Ipv4Addr::from(u32::from_be_bytes([buf[44], buf[45], buf[46], buf[47]]));
        Ok(PcpMapping {
            proto,
            internal_port: in_port,
            external_port: ex_port,
            lifetime_sec: life,
            // 透传外部 IP 供上层使用
            ext_ip: Some(ext_ip.to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket;
    use std::thread;

    #[test]
    fn map_against_mock() {
        let srv = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = srv.local_addr().unwrap().port();
        thread::spawn(move || {
            let mut buf = [0u8; 128];
            if let Ok((n, peer)) = srv.recv_from(&mut buf) {
                // 请求布局：header(24) + nonce(12 @24) + proto(1 @36) + reserved(3) +
                //           internal(2 @40) + external(2 @42) + ext IP(16 @44)
                if n >= 56 && buf[1] == OP_MAP {
                    let life = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
                    let in_port = u16::from_be_bytes([buf[40], buf[41]]);
                    let ex_port = u16::from_be_bytes([buf[42], buf[43]]);
                    let proto = buf[36];
                    let mut resp = Vec::new();
                    resp.push(2); // version
                    resp.push(OP_MAP); // response opcode
                    resp.extend_from_slice(&0u16.to_be_bytes()); // result
                    resp.extend_from_slice(&life.to_be_bytes());
                    resp.extend_from_slice(&[0u8; 16]); // reserved (client IP echoed 区域)
                    resp.extend_from_slice(&buf[24..36]); // echo nonce
                    resp.push(proto);
                    resp.extend_from_slice(&[0u8; 3]);
                    resp.extend_from_slice(&in_port.to_be_bytes());
                    resp.extend_from_slice(&ex_port.to_be_bytes());
                    resp.extend_from_slice(&0x0a000001u32.to_be_bytes()); // 10.0.0.1
                    resp.extend_from_slice(&[0u8; 12]);
                    let _ = srv.send_to(&resp, peer);
                }
            }
        });
        let c = PcpClient::new(format!("127.0.0.1:{}", port));
        let m = c
            .map(PROTO_UDP, 17801, 45000, 3600, Duration::from_secs(2))
            .expect("pcp map 应成功");
        assert_eq!(m.internal_port, 17801);
        assert_eq!(m.external_port, 45000);
        assert_eq!(m.lifetime_sec, 3600);
        assert_eq!(m.ext_ip.as_deref(), Some("10.0.0.1"));
    }
}

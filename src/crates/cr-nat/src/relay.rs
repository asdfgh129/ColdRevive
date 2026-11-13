//! 中继传输（LowID 回退），std-only。
//!
//! 当所有端口映射（UPnP/NAT-PMP/PCP）与预测打洞都失败时，退化到经可信中继服务器转发
//! 唤醒/对等消息。本模块实现**客户端侧**的帧协议与连接管理；中继服务端为部署侧组件，
//! 不在本 crate 内实现（仅提供 mock 用于单测）。
//!
//! 帧格式（大端）：1 字节 kind + 16 字节 node id + 4 字节长度 + 负载。
//!   kind=0x01 REGISTER  负载 = 16 字节节点 id
//!   kind=0x02 RELAY     负载 = 16 字节目标 id + 任意消息
//!   kind=0x03 DELIVERED 负载 = 16 字节来源 id + 任意消息
//!   kind=0x04 ACK       负载为空

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub const NODE_ID_LEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Register,
    Relay,
    Delivered,
    Ack,
    Unknown(u8),
}

impl FrameKind {
    fn code(&self) -> u8 {
        match self {
            FrameKind::Register => 0x01,
            FrameKind::Relay => 0x02,
            FrameKind::Delivered => 0x03,
            FrameKind::Ack => 0x04,
            FrameKind::Unknown(c) => *c,
        }
    }
    fn from_code(c: u8) -> FrameKind {
        match c {
            0x01 => FrameKind::Register,
            0x02 => FrameKind::Relay,
            0x03 => FrameKind::Delivered,
            0x04 => FrameKind::Ack,
            other => FrameKind::Unknown(other),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RelayFrame {
    pub kind: FrameKind,
    pub node: [u8; NODE_ID_LEN],
    pub payload: Vec<u8>,
}

impl RelayFrame {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(1 + NODE_ID_LEN + 4 + self.payload.len());
        v.push(self.kind.code());
        v.extend_from_slice(&self.node);
        v.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        v.extend_from_slice(&self.payload);
        v
    }

    pub fn decode(data: &[u8]) -> Option<RelayFrame> {
        if data.len() < 1 + NODE_ID_LEN + 4 {
            return None;
        }
        let kind = FrameKind::from_code(data[0]);
        let mut node = [0u8; NODE_ID_LEN];
        node.copy_from_slice(&data[1..1 + NODE_ID_LEN]);
        let len = u32::from_be_bytes([
            data[1 + NODE_ID_LEN],
            data[2 + NODE_ID_LEN],
            data[3 + NODE_ID_LEN],
            data[4 + NODE_ID_LEN],
        ]) as usize;
        let start = 1 + NODE_ID_LEN + 4;
        if data.len() < start + len {
            return None;
        }
        Some(RelayFrame {
            kind,
            node,
            payload: data[start..start + len].to_vec(),
        })
    }
}

pub struct RelayClient {
    pub addr: String,
}

impl RelayClient {
    pub fn new(addr: String) -> Self {
        Self { addr }
    }

    pub fn connect(&self, timeout: Duration) -> Result<RelayConn, String> {
        let addr = self
            .addr
            .to_socket_addrs()
            .map_err(|e| format!("dns: {}", e))?
            .next()
            .ok_or_else(|| "relay 地址无解析结果".to_string())?;
        let stream = TcpStream::connect(addr).map_err(|e| format!("connect: {}", e))?;
        stream.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(timeout)).map_err(|e| e.to_string())?;
        Ok(RelayConn { stream })
    }
}

pub struct RelayConn {
    stream: TcpStream,
}

impl RelayConn {
    pub fn register(&mut self, node_id: &[u8; NODE_ID_LEN]) -> Result<(), String> {
        let frame = RelayFrame {
            kind: FrameKind::Register,
            node: *node_id,
            payload: node_id.to_vec(),
        };
        self.stream
            .write_all(&frame.encode())
            .map_err(|e| e.to_string())?;
        self.stream.flush().map_err(|e| e.to_string())?;
        // 等待 ACK
        let resp = self.recv_frame()?;
        if resp.kind != FrameKind::Ack {
            return Err("relay 未回 ACK".into());
        }
        Ok(())
    }

    pub fn send_relay(&mut self, target: &[u8; NODE_ID_LEN], message: &[u8]) -> Result<(), String> {
        let mut payload = Vec::with_capacity(NODE_ID_LEN + message.len());
        payload.extend_from_slice(target);
        payload.extend_from_slice(message);
        let frame = RelayFrame {
            kind: FrameKind::Relay,
            node: *target,
            payload,
        };
        self.stream
            .write_all(&frame.encode())
            .map_err(|e| e.to_string())?;
        self.stream.flush().map_err(|e| e.to_string())
    }

    pub fn recv_frame(&mut self) -> Result<RelayFrame, String> {
        let mut head = [0u8; 1 + NODE_ID_LEN + 4];
        self.stream
            .read_exact(&mut head)
            .map_err(|e| format!("读帧头失败: {}", e))?;
        let len = u32::from_be_bytes([
            head[1 + NODE_ID_LEN],
            head[2 + NODE_ID_LEN],
            head[3 + NODE_ID_LEN],
            head[4 + NODE_ID_LEN],
        ]) as usize;
        let mut payload = vec![0u8; len];
        if len > 0 {
            self.stream
                .read_exact(&mut payload)
                .map_err(|e| format!("读帧体失败: {}", e))?;
        }
        RelayFrame::decode(&{
            let mut v = head.to_vec();
            v.extend_from_slice(&payload);
            v
        })
        .ok_or_else(|| "帧解析失败".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// 在调用方线程绑定监听，把 listener 移入工作线程，返回端口，避免竞态。
    fn spawn_mock_relay() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // 读完整 REGISTER 帧（头 + 16 字节 payload），回 ACK
                let mut head = [0u8; 1 + NODE_ID_LEN + 4];
                if stream.read_exact(&mut head).is_ok() {
                    let plen = u32::from_be_bytes([
                        head[1 + NODE_ID_LEN],
                        head[2 + NODE_ID_LEN],
                        head[3 + NODE_ID_LEN],
                        head[4 + NODE_ID_LEN],
                    ]) as usize;
                    let mut payload = vec![0u8; plen];
                    if plen == 0 || stream.read_exact(&mut payload).is_ok() {
                        let ack = RelayFrame {
                            kind: FrameKind::Ack,
                            node: [0u8; NODE_ID_LEN],
                            payload: Vec::new(),
                        };
                        let _ = stream.write_all(&ack.encode());
                        let _ = stream.flush();
                    }
                }
                // 读完整 RELAY 帧，回 DELIVERED（来源=目标，模拟回环投递）
                let mut head2 = [0u8; 1 + NODE_ID_LEN + 4];
                if stream.read_exact(&mut head2).is_ok() {
                    let plen = u32::from_be_bytes([
                        head2[1 + NODE_ID_LEN],
                        head2[2 + NODE_ID_LEN],
                        head2[3 + NODE_ID_LEN],
                        head2[4 + NODE_ID_LEN],
                    ]) as usize;
                    let mut payload = vec![0u8; plen];
                    if plen == 0 || stream.read_exact(&mut payload).is_ok() {
                        let src: [u8; NODE_ID_LEN] = payload
                            .get(..NODE_ID_LEN)
                            .and_then(|s| s.try_into().ok())
                            .unwrap_or([0u8; NODE_ID_LEN]);
                        let msg = if payload.len() > NODE_ID_LEN {
                            payload[NODE_ID_LEN..].to_vec()
                        } else {
                            Vec::new()
                        };
                        let delivered = RelayFrame {
                            kind: FrameKind::Delivered,
                            node: src,
                            payload: msg,
                        };
                        let _ = stream.write_all(&delivered.encode());
                        let _ = stream.flush();
                    }
                }
            }
        });
        port
    }

    #[test]
    fn frame_roundtrip() {
        let f = RelayFrame {
            kind: FrameKind::Relay,
            node: [7u8; NODE_ID_LEN],
            payload: b"hello".to_vec(),
        };
        let enc = f.encode();
        let dec = RelayFrame::decode(&enc).unwrap();
        assert_eq!(dec.kind, FrameKind::Relay);
        assert_eq!(dec.node, [7u8; NODE_ID_LEN]);
        assert_eq!(dec.payload, b"hello");
    }

    #[test]
    fn register_and_relay_against_mock() {
        let port = spawn_mock_relay();
        let client = RelayClient::new(format!("127.0.0.1:{}", port));
        let mut conn = client.connect(Duration::from_secs(2)).unwrap();
        let id = [1u8; NODE_ID_LEN];
        conn.register(&id).expect("register 应成功");
        let target = [2u8; NODE_ID_LEN];
        conn.send_relay(&target, b"wakeup").expect("send_relay 应成功");
        let delivered = conn.recv_frame().unwrap();
        assert_eq!(delivered.kind, FrameKind::Delivered);
        assert_eq!(&delivered.payload, b"wakeup");
    }
}

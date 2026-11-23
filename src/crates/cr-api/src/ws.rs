//! RFC6455 握手与帧编解码（std-only，服务端推送子集）。
//!
//! 只用 `cr_hash::sha1` 做 `Sec-WebSocket-Accept`，base64 自己实现；不引第三方 websocket
//! crate。帧方向明确：服务端发出的帧**不掩码**，客户端来的帧**必须掩码**（RFC6455 §5.1）。

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 标准 base64 编码（无换行）。WebSocket accept 只有 20 字节，不需要流式实现。
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        out.push(B64[((n >> 18) & 63) as usize] as char);
        out.push(B64[((n >> 12) & 63) as usize] as char);
        out.push(B64[((n >> 6) & 63) as usize] as char);
        out.push(B64[(n & 63) as usize] as char);
        i += 3;
    }
    let rem = data.len() - i;
    if rem == 1 {
        let n = (data[i] as u32) << 16;
        out.push(B64[((n >> 18) & 63) as usize] as char);
        out.push(B64[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        out.push(B64[((n >> 18) & 63) as usize] as char);
        out.push(B64[((n >> 12) & 63) as usize] as char);
        out.push(B64[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

/// `Sec-WebSocket-Key` → `Sec-WebSocket-Accept`（RFC6455 §4.2.2）。
pub fn accept_key(key: &str) -> String {
    let mut seed = key.as_bytes().to_vec();
    seed.extend_from_slice(WS_GUID.as_bytes());
    base64_encode(&cr_hash::sha1(&seed))
}

/// 生成 101 Switching Protocols 响应。非法/缺失 key 时返回错误，绝不回一个假的 accept。
pub fn handshake_response(key: Option<&str>) -> Result<String, &'static str> {
    let key = key.filter(|k| !k.trim().is_empty()).ok_or("缺少 Sec-WebSocket-Key")?;
    Ok(format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        accept_key(key.trim())
    ))
}

/// 从原始请求头（含请求行）里取 `Sec-WebSocket-Key`，大小写不敏感。
pub fn key_from_request(raw: &str) -> Option<&str> {
    raw.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("Sec-WebSocket-Key") {
            Some(value.trim())
        } else {
            None
        }
    })
}

/// 编码一帧服务端帧。`mask=false`（RFC6455 要求服务端帧不掩码）。
pub fn encode_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 10);
    out.push(0x80 | (opcode & 0x0f));
    let len = payload.len();
    if len < 126 {
        out.push(len as u8);
    } else if len <= u16::MAX as usize {
        out.push(126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
    out.extend_from_slice(payload);
    out
}

pub fn encode_text(payload: &[u8]) -> Vec<u8> { encode_frame(0x1, payload) }
pub fn encode_ping(payload: &[u8]) -> Vec<u8> { encode_frame(0x9, payload) }
pub fn encode_pong(payload: &[u8]) -> Vec<u8> { encode_frame(0xA, payload) }
pub fn encode_close() -> Vec<u8> { encode_frame(0x8, &[]) }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientFrame {
    pub fin: bool,
    pub opcode: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    Incomplete,
    UnmaskedClientFrame,
    TooLarge,
}

/// 解析一帧**客户端**帧。返回 `(帧, 已消费字节数)`；缓冲不足时 `Incomplete`。
///
/// 守护进程的 /ws 只需要识别 close / ping，文本帧忽略即可（服务端只推送状态）。
pub fn decode_client_frame(buf: &[u8]) -> Result<(ClientFrame, usize), FrameError> {
    if buf.len() < 2 { return Err(FrameError::Incomplete); }
    let fin = buf[0] & 0x80 != 0;
    let opcode = buf[0] & 0x0f;
    let masked = buf[1] & 0x80 != 0;
    if !masked { return Err(FrameError::UnmaskedClientFrame); }
    let mut len = (buf[1] & 0x7f) as u64;
    let mut head = 2usize;
    if len == 126 {
        if buf.len() < 4 { return Err(FrameError::Incomplete); }
        len = u16::from_be_bytes([buf[2], buf[3]]) as u64;
        head = 4;
    } else if len == 127 {
        if buf.len() < 10 { return Err(FrameError::Incomplete); }
        let mut b = [0u8; 8];
        b.copy_from_slice(&buf[2..10]);
        len = u64::from_be_bytes(b);
        head = 10;
    }
    let len_usize = usize::try_from(len).map_err(|_| FrameError::TooLarge)?;
    const MAX_FRAME: usize = 64 * 1024;
    if len_usize > MAX_FRAME { return Err(FrameError::TooLarge); }
    if buf.len() < head + 4 + len_usize { return Err(FrameError::Incomplete); }
    let mask = [buf[head], buf[head + 1], buf[head + 2], buf[head + 3]];
    let mut payload = Vec::with_capacity(len_usize);
    for (i, b) in buf[head + 4..head + 4 + len_usize].iter().enumerate() {
        payload.push(b ^ mask[i % 4]);
    }
    Ok((ClientFrame { fin, opcode, payload }, head + 4 + len_usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6455_accept_example() {
        // RFC6455 §4.2.2 的官方样例。
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn base64_padding() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn handshake_uses_101_and_never_fakes_accept() {
        let r = handshake_response(Some("dGhlIHNhbXBsZSBub25jZQ==")).unwrap();
        assert!(r.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));
        assert!(r.contains("Upgrade: websocket"));
        assert!(r.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
        assert!(handshake_response(None).is_err());
        assert!(handshake_response(Some("  ")).is_err());
    }

    #[test]
    fn key_from_request_is_case_insensitive() {
        let raw = "GET /ws HTTP/1.1\r\nHost: x\r\nsec-websocket-key: abc==\r\n\r\n";
        assert_eq!(key_from_request(raw), Some("abc=="));
    }

    #[test]
    fn text_frame_layout_and_no_server_mask() {
        assert_eq!(encode_text(b"hi"), vec![0x81, 0x02, b'h', b'i']);
        let long = vec![b'x'; 126];
        let f = encode_text(&long);
        assert_eq!(&f[..4], &[0x81, 126, 0, 126]);
        assert_eq!(f.len(), 4 + 126);
        let huge = vec![b'y'; 65536];
        let f = encode_text(&huge);
        assert_eq!(f[0], 0x81);
        assert_eq!(f[1], 127);
        assert_eq!(&f[2..10], &(65536u64).to_be_bytes());
        // 服务端帧第二字节最高位必须为 0。
        assert_eq!(f[1] & 0x80, 0);
    }

    #[test]
    fn masked_client_text_frame_roundtrip() {
        // payload "hi" = 0x68 0x69, mask 01 02 03 04 → 0x69 0x6b
        let wire = [0x81, 0x82, 1, 2, 3, 4, 0x69, 0x6b];
        let (f, used) = decode_client_frame(&wire).unwrap();
        assert_eq!(used, wire.len());
        assert_eq!(f.opcode, 1);
        assert!(f.fin);
        assert_eq!(f.payload, b"hi");
        assert_eq!(decode_client_frame(&wire[..6]), Err(FrameError::Incomplete));
    }

    #[test]
    fn unmasked_client_frame_is_rejected() {
        assert_eq!(decode_client_frame(&[0x81, 0x02, b'h', b'i']),
                   Err(FrameError::UnmaskedClientFrame));
    }
}

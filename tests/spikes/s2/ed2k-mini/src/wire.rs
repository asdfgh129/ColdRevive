//! Minimal standard eD2k client-to-client TCP wire codec (ADR-011 L2 spike).
//!
//! Every constant and payload below is transcribed from aMule `master`
//! @ 6a1105634d81242a581a0f36a7e00379b12a83f3 (same pinned baseline as
//! `kad-mini`). Source paths are given inline. This is a PROTOCOL PROBE, not a
//! production client: no compression, no obfuscation, no credit system, no
//! source exchange, no AICH, no >4 GiB (I64) parts, no server connection.
//!
//! NOTE ON TAGS: the eD2k TCP handshake reads tags with `CTag::CTag(CFileDataIO&)`
//! (`src/Tag.cpp`), which accepts BOTH the compact `[type|0x80][name:1]` form
//! and the `[type][u16 name_len][name]` form. `CTag::WriteTagToFile` ->
//! `CFileDataIO::WriteTag` emits the latter. That is exactly the same framing
//! the KAD `ReadTag` path requires, so we reuse `kad::Tag` for writing and add
//! a tolerant reader here. Do NOT use `WriteNewEd2kTag`'s compact form on the
//! KAD path -- that was the v2.4.26 bug (PLAN §19).

#![allow(dead_code)]

use std::io::{Read, Write};

use crate::kad::{Buf, Cur, Tag, TagValue};

// src/include/protocol/Protocols.h
pub const PROT_EDONKEY: u8 = 0xE3;
pub const PROT_EMULE: u8 = 0xC5;
pub const PROT_PACKED: u8 = 0xD4;

// src/include/protocol/ed2k/Client2Client/TCP.h
#[allow(dead_code)]
pub mod op {
    pub const HELLO: u8 = 0x01;
    pub const SENDINGPART: u8 = 0x46;
    pub const REQUESTPARTS: u8 = 0x47;
    pub const FILEREQANSNOFIL: u8 = 0x48;
    pub const END_OF_DOWNLOAD: u8 = 0x49;
    pub const ASKSHAREDFILES: u8 = 0x4A;
    pub const ASKSHAREDFILESANSWER: u8 = 0x4B;
    pub const ASKSHAREDDENIEDANS: u8 = 0x61;
    pub const HELLOANSWER: u8 = 0x4C;
    pub const MESSAGE: u8 = 0x4E;
    pub const SETREQFILEID: u8 = 0x4F;
    pub const FILESTATUS: u8 = 0x50;
    pub const HASHSETREQUEST: u8 = 0x51;
    pub const HASHSETANSWER: u8 = 0x52;
    pub const STARTUPLOADREQ: u8 = 0x54;
    pub const ACCEPTUPLOADREQ: u8 = 0x55;
    pub const CANCELTRANSFER: u8 = 0x56;
    pub const OUTOFPARTREQS: u8 = 0x57;
    pub const REQUESTFILENAME: u8 = 0x58;
    pub const REQFILENAMEANSWER: u8 = 0x59;
    pub const QUEUERANK: u8 = 0x5C; // legacy, prot 0xE3, uint32 rank
    // Extended-protocol opcodes (prot 0xC5)
    pub const COMPRESSEDPART: u8 = 0x40;
    pub const QUEUERANKING: u8 = 0x60; // 12-byte payload, see SendRankingInfo
    pub const SENDINGPART_I64: u8 = 0xA2;
    pub const REQUESTPARTS_I64: u8 = 0xA3;
}

// src/include/tags/ClientTags.h
#[allow(dead_code)]
pub mod ct {
    pub const NAME: u8 = 0x01;
    pub const PORT: u8 = 0x0F;
    pub const VERSION: u8 = 0x11;
    pub const EMULECOMPAT_OPTIONS: u8 = 0xEF;
    pub const EMULE_UDPPORTS: u8 = 0xF9;
    pub const EMULE_MISCOPTIONS1: u8 = 0xFA;
    pub const EMULE_VERSION: u8 = 0xFB;
    pub const EMULE_MISCOPTIONS2: u8 = 0xFE;
}

// src/include/protocol/ed2k/Constants.h
pub const BLOCKSIZE: u32 = 184_320;
pub const PARTSIZE: usize = crate::kad::ED2K_PART_SIZE; // 9_728_000

pub fn opcode_name(opcode: u8) -> &'static str {
    match opcode {
        op::HELLO => "OP_HELLO",
        op::SENDINGPART => "OP_SENDINGPART",
        op::REQUESTPARTS => "OP_REQUESTPARTS",
        op::FILEREQANSNOFIL => "OP_FILEREQANSNOFIL",
        op::END_OF_DOWNLOAD => "OP_END_OF_DOWNLOAD",
        op::HELLOANSWER => "OP_HELLOANSWER",
        op::SETREQFILEID => "OP_SETREQFILEID",
        op::ASKSHAREDFILES => "OP_ASKSHAREDFILES",
        op::ASKSHAREDFILESANSWER => "OP_ASKSHAREDFILESANSWER",
        op::ASKSHAREDDENIEDANS => "OP_ASKSHAREDDENIEDANS",
        op::FILESTATUS => "OP_FILESTATUS",
        op::HASHSETREQUEST => "OP_HASHSETREQUEST",
        op::HASHSETANSWER => "OP_HASHSETANSWER",
        op::STARTUPLOADREQ => "OP_STARTUPLOADREQ",
        op::ACCEPTUPLOADREQ => "OP_ACCEPTUPLOADREQ",
        op::CANCELTRANSFER => "OP_CANCELTRANSFER",
        op::OUTOFPARTREQS => "OP_OUTOFPARTREQS",
        op::REQUESTFILENAME => "OP_REQUESTFILENAME",
        op::REQFILENAMEANSWER => "OP_REQFILENAMEANSWER",
        op::QUEUERANK => "OP_QUEUERANK",
        op::QUEUERANKING => "OP_QUEUERANKING",
        op::COMPRESSEDPART => "OP_COMPRESSEDPART",
        _ => "UNKNOWN",
    }
}

// --------------------------------------------------------------- framing --
//
// src/OtherStructs.h `Header_Struct` (packed):
//     <protocol u8><packetlength u32 LE><opcode u8><payload...>
// `CPacket::CPacket(rawHeader, buf)`: size = ENDIAN_SWAP_32(packetlength) - 1,
// i.e. packetlength = 1 (opcode) + payload length. `GetPacketSizeFromHeader`
// rejects size < 1 or >= 0x7ffffff0.
pub fn frame(protocol: u8, opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + payload.len());
    out.push(protocol);
    out.extend_from_slice(&((payload.len() + 1) as u32).to_le_bytes());
    out.push(opcode);
    out.extend_from_slice(payload);
    out
}

#[derive(Clone, Debug)]
pub struct Packet {
    pub protocol: u8,
    pub opcode: u8,
    pub payload: Vec<u8>,
}

pub fn parse_frame(data: &[u8]) -> Result<Packet, String> {
    if data.len() < 6 {
        return Err("eD2k frame shorter than 6-byte header".into());
    }
    let len = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
    if len < 1 || len >= 0x7fff_fff0 {
        return Err(format!("invalid eD2k packet length {len}"));
    }
    let want = len - 1;
    if data.len() - 6 < want {
        return Err(format!("truncated packet: need {want}, have {}", data.len() - 6));
    }
    Ok(Packet { protocol: data[0], opcode: data[5], payload: data[6..6 + want].to_vec() })
}

pub fn send_packet<S: Write>(stream: &mut S, protocol: u8, opcode: u8, payload: &[u8]) -> Result<(), String> {
    let f = frame(protocol, opcode, payload);
    stream.write_all(&f).map_err(|e| format!("write eD2k frame: {e}"))?;
    stream.flush().map_err(|e| format!("flush eD2k frame: {e}"))
}

pub fn read_packet<S: Read>(stream: &mut S) -> Result<Packet, String> {
    let mut pending = Vec::new();
    read_packet_pending(stream, &mut pending)
}

/// 带读超时的连接必须用这个版本：`pending` 在超时后保留半帧字节，下一次
/// 调用接着读。`read_exact` 会在超时前消费掉半帧却把这些字节丢掉，之后
/// 永远解出错误的长度（实测 aMule >PARTSIZE 下载报
/// `invalid eD2k packet length 2655250573`）。
fn fill_pending<S: Read>(stream: &mut S, pending: &mut Vec<u8>, want: usize, label: &'static str) -> Result<(), String> {
    while pending.len() < want {
        let mut buf = [0u8; 8192];
        let n = match stream.read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("read eD2k {label}: {e}")),
        };
        if n == 0 {
            return Err(format!("read eD2k {label}: connection closed"));
        }
        pending.extend_from_slice(&buf[..n]);
    }
    Ok(())
}

pub fn read_packet_pending<S: Read>(stream: &mut S, pending: &mut Vec<u8>) -> Result<Packet, String> {
    fill_pending(stream, pending, 6, "header")?;
    let len = u32::from_le_bytes([pending[1], pending[2], pending[3], pending[4]]) as usize;
    if len < 1 || len >= 0x7fff_fff0 || len > 16 * 1024 * 1024 {
        return Err(format!("invalid eD2k packet length {len}"));
    }
    fill_pending(stream, pending, len + 5, "body")?;
    let frame: Vec<u8> = pending.drain(..len + 5).collect();
    Ok(Packet { protocol: frame[0], opcode: frame[5], payload: frame[6..].to_vec() })
}

// ------------------------------------------------------------------- tags --
//
// `CTag::CTag(CFileDataIO&, bool)` in src/Tag.cpp:
//     type = ReadUInt8(); if (type & 0x80) { type &= 0x7f; name = ReadUInt8(); }
//     else { len = ReadUInt16(); len==1 ? name=ReadUInt8() : name=ReadOnlyString(len) }
//     then switch(type) -- STR1..STR16 (0x11..0x20) carry the string length in
//     the type byte itself and are ACCEPTED here (unlike KAD's ReadTag).
pub fn read_meta_tag(c: &mut Cur<'_>) -> Result<Tag, String> {
    let raw_type = c.u8()?;
    let compact = raw_type & 0x80 != 0;
    let ty = raw_type & 0x7f;
    let name = if compact {
        vec![c.u8()?]
    } else {
        let n = c.u16()? as usize;
        c.raw(n)?.to_vec()
    };
    let value = match ty {
        0x01 => {
            let mut h = [0u8; 16];
            h.copy_from_slice(c.raw(16)?);
            TagValue::Hash16(h)
        }
        0x02 => {
            let n = c.u16()? as usize;
            TagValue::Str(String::from_utf8_lossy(c.raw(n)?).into_owned())
        }
        0x03 => TagValue::U32(c.u32()?),
        0x04 => TagValue::U32(c.u32()?), // FLOAT32 bits, opaque
        0x05 => TagValue::U8(c.u8()?),  // BOOL
        0x07 => {
            let n = c.u32()? as usize;
            TagValue::Raw(0x07, c.raw(n)?.to_vec())
        }
        0x08 => TagValue::U16(c.u16()?),
        0x09 => TagValue::U8(c.u8()?),
        0x0A => {
            // CFileDataIO::WriteBsob / ReadBsob use a ONE-byte length, unlike
            // BLOB's uint32. (CTag::CTag cannot read BSOB at all, but a
            // correctly-sized skip keeps us in sync with peers that send it.)
            let n = c.u8()? as usize;
            TagValue::Raw(0x0A, c.raw(n)?.to_vec())
        }
        0x0B => TagValue::U64(c.u64()?),
        0x11..=0x22 => {
            let n = (ty - 0x11 + 1) as usize;
            TagValue::Str(String::from_utf8_lossy(c.raw(n)?).into_owned())
        }
        other => return Err(format!("unsupported eD2k meta tag type 0x{other:02x}")),
    };
    Ok(Tag { name, value })
}

pub fn tag_as_u32(t: &Tag) -> Option<u32> {
    match t.value {
        TagValue::U8(v) => Some(v as u32),
        TagValue::U16(v) => Some(v as u32),
        TagValue::U32(v) => Some(v),
        _ => None,
    }
}

pub fn find_tag<'a>(tags: &'a [Tag], name: u8) -> Option<&'a Tag> {
    tags.iter().find(|t| t.name.len() == 1 && t.name[0] == name)
}

pub fn tag_name_display(name: &[u8]) -> String {
    let mut out = String::new();
    for &b in name {
        if (0x20..0x7f).contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{b:02x}"));
        }
    }
    out
}

// ------------------------------------------------------------------ hello --
//
// src/BaseClient.cpp `SendHelloPacket` / `SendHelloTypePacket` / `SendHelloAnswer`
// and `ProcessHelloPacket` / `ProcessHelloAnswer` / `ProcessHelloTypePacket`.
//   OP_HELLO       payload: <0x10><userhash 16><clientID u32><port u16>
//                           <tagcount u32><tags...><serverIP u32><serverPort u16>
//   OP_HELLOANSWER payload: same WITHOUT the leading 0x10 hash-size byte.
#[derive(Clone, Debug)]
pub struct Hello {
    pub user_hash: [u8; 16],
    pub client_id: u32,
    pub tcp_port: u16,
    pub tags: Vec<Tag>,
    pub server_ip: u32,
    pub server_port: u16,
}

fn hello_body(h: &Hello, with_hash_size: bool) -> Vec<u8> {
    let mut b = Buf::new();
    if with_hash_size {
        b.u8(16);
    }
    b.id(&h.user_hash);
    b.u32(h.client_id);
    b.u16(h.tcp_port);
    b.u32(h.tags.len() as u32); // u32 here, unlike KAD's u8 tag lists
    for t in &h.tags {
        t.encode(&mut b).expect("tag encode is infallible for these types");
    }
    b.u32(h.server_ip);
    b.u16(h.server_port);
    b.0
}

pub fn hello_frame(h: &Hello) -> Vec<u8> {
    frame(PROT_EDONKEY, op::HELLO, &hello_body(h, true))
}

pub fn hello_answer_frame(h: &Hello) -> Vec<u8> {
    frame(PROT_EDONKEY, op::HELLOANSWER, &hello_body(h, false))
}

pub fn parse_hello(payload: &[u8], with_hash_size: bool) -> Result<Hello, String> {
    let mut c = Cur::new(payload);
    if with_hash_size && c.u8()? != 16 {
        return Err("invalid hello: userhash size != 16".into());
    }
    let user_hash = c.id()?;
    let client_id = c.u32()?;
    let tcp_port = c.u16()?;
    let count = c.u32()? as usize;
    if count > 1024 {
        return Err(format!("implausible hello tag count {count}"));
    }
    let mut tags = Vec::with_capacity(count);
    for _ in 0..count {
        tags.push(read_meta_tag(&mut c)?);
    }
    let server_ip = c.u32()?;
    let server_port = c.u16()?;
    Ok(Hello { user_hash, client_id, tcp_port, tags, server_ip, server_port })
}

/// A deliberately conservative eMule-compatible HELLO: no compression, no
/// extended requests, no AICH, no source exchange, no large-file support.
/// `CT_EMULE_VERSION` is present so aMule marks us as ExtProtocolAvailable
/// (`BaseClient.cpp`: `bIsMule = dwEmuleTags & 0x04`), which is what a normal
/// eMule client looks like; the client handles both queue-ranking opcodes.
pub fn default_hello(user_hash: [u8; 16], tcp_port: u16, kad_udp_port: u16, client_udp_port: u16, nick: &str) -> Hello {
    let udp_ports = ((kad_udp_port as u32) << 16) | client_udp_port as u32;
    Hello {
        user_hash,
        client_id: 0,
        tcp_port,
        tags: vec![
            Tag::str1(ct::NAME, nick),
            Tag::int4(ct::VERSION, 0x3c), // EDONKEYVERSION, src/include/common/ClientVersion.h
            Tag::int2(ct::PORT, tcp_port),
            Tag::int4(ct::EMULE_UDPPORTS, udp_ports),
            // All feature bits 0 => GetExtendedRequestsVersion() == 0, so a
            // 16-byte OP_REQUESTFILENAME is legal (ProcessExtendedInfo returns
            // immediately instead of throwing "Wrong size on extended info").
            Tag::int4(ct::EMULE_MISCOPTIONS1, 0),
            Tag::int4(ct::EMULE_VERSION, 0),
            Tag::int4(ct::EMULE_MISCOPTIONS2, 0),
        ],
        server_ip: 0,
        server_port: 0,
    }
}

// ------------------------------------------------------- file / handshake --
pub type Hash16 = [u8; 16];

pub fn file_request(hash: &Hash16) -> Vec<u8> {
    frame(PROT_EDONKEY, op::REQUESTFILENAME, hash)
}

/// `OP_SETREQFILEID` (0x4F), the *other* file-identification opcode.
/// src/ClientTCPSocket.cpp: for an unknown file 0x58 silently `break`s, while
/// 0x4F explicitly replies `OP_FILEREQANSNOFIL` (0x48). Both carry only the
/// 16-byte fileID in their minimal form.
pub fn set_req_file_id(hash: &Hash16) -> Vec<u8> {
    frame(PROT_EDONKEY, op::SETREQFILEID, hash)
}

/// `OP_REQFILENAMEANSWER`: `<filehash 16><filename: u16 len + bytes>`.
/// src/ClientTCPSocket.cpp case OP_REQUESTFILENAME -> `WriteString(...)`.
pub fn file_name_answer(hash: &Hash16, name: &str) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(hash);
    b.u16(name.len() as u16);
    b.raw(name.as_bytes());
    frame(PROT_EDONKEY, op::REQFILENAMEANSWER, &b.0)
}

pub fn parse_file_name_answer(payload: &[u8]) -> Result<(Hash16, String), String> {
    let mut c = Cur::new(payload);
    let hash = c.id()?;
    let n = c.u16()? as usize;
    let name = String::from_utf8_lossy(c.raw(n)?).into_owned();
    Ok((hash, name))
}

pub fn no_such_file(hash: &Hash16) -> Vec<u8> {
    frame(PROT_EDONKEY, op::FILEREQANSNOFIL, hash)
}

/// `OP_FILESTATUS` (0x50): aMule 的 OP_SETREQFILEID (0x4F) 应答。
///
/// 源码（`ClientTCPSocket.cpp`，`case OP_SETREQFILEID`）对完整共享文件是
/// `<filehash 16><u16 part_status=0>`；只有 PartFile 才写 `WritePartStatus`。
/// 真实 aMule 2.3.3 对 >= PARTSIZE 的文件会先发 0x4F 选文件，收不到 0x50
/// 就不会发 STARTUPLOADREQ / HASHSETREQUEST，表现为「连上后停在文件名应答」。
pub fn file_status(hash: &Hash16) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(hash);
    b.u16(0);
    frame(PROT_EDONKEY, op::FILESTATUS, &b.0)
}

/// `OP_ASKSHAREDFILES` is a null-payload request; the answer starts with a
/// uint32 file count (`src/ClientTCPSocket.cpp` case OP_ASKSHAREDFILES).
pub fn ask_shared_files() -> Vec<u8> {
    frame(PROT_EDONKEY, op::ASKSHAREDFILES, &[])
}

pub fn start_upload_req(hash: &Hash16) -> Vec<u8> {
    frame(PROT_EDONKEY, op::STARTUPLOADREQ, hash)
}

pub fn accept_upload_req() -> Vec<u8> {
    frame(PROT_EDONKEY, op::ACCEPTUPLOADREQ, &[])
}

pub fn cancel_transfer() -> Vec<u8> {
    frame(PROT_EDONKEY, op::CANCELTRANSFER, &[])
}

/// `OP_QUEUERANKING` (extended protocol, 0xC5). src/UploadClient.cpp
/// `SendRankingInfo`: u16 rank + u32 0 + u32 0 + u16 0 = 12 bytes; the receiver
/// rejects any other size ("Invalid size (OP_QUEUERANKING)").
pub fn queue_ranking(rank: u16) -> Vec<u8> {
    let mut b = Buf::new();
    b.u16(rank);
    b.u32(0);
    b.u32(0);
    b.u16(0);
    frame(PROT_EMULE, op::QUEUERANKING, &b.0)
}

pub fn legacy_queue_rank(rank: u32) -> Vec<u8> {
    let mut b = Buf::new();
    b.u32(rank);
    frame(PROT_EDONKEY, op::QUEUERANK, &b.0)
}

// --------------------------------------------------------------- hashset --
//
// src/UploadClient.cpp `SendHashsetPacket`: `<filehash 16><count u16><hash*16>`.
// The count is aMule's `GetHashCount()` == m_iED2KPartHashCount, which for a
// file whose size is an exact multiple of PARTSIZE INCLUDES the empty part hash
// (the "Red" rule; see kad::ed2k_part_hashes).
pub fn hashset_request(hash: &Hash16) -> Vec<u8> {
    frame(PROT_EDONKEY, op::HASHSETREQUEST, hash)
}

pub fn hashset_answer(hash: &Hash16, parts: &[Hash16]) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(hash);
    b.u16(parts.len() as u16);
    for p in parts {
        b.id(p);
    }
    frame(PROT_EDONKEY, op::HASHSETANSWER, &b.0)
}

pub fn parse_hashset_answer(payload: &[u8]) -> Result<(Hash16, Vec<Hash16>), String> {
    let mut c = Cur::new(payload);
    let hash = c.id()?;
    let n = c.u16()? as usize;
    let mut parts = Vec::with_capacity(n);
    for _ in 0..n {
        parts.push(c.id()?);
    }
    Ok((hash, parts))
}

// ---------------------------------------------------------- part transfer --
//
// src/DownloadClient.cpp `SendBlockRequests` (legacy branch):
//   <filehash 16><start[0..3] u32><end[0..3] u32>  -- ALL 3 starts, then ALL
//   3 ends; unused slots are zero pairs and are skipped by the uploader.
//   NOTE: this is NOT three interleaved <start,end> pairs.
// src/UploadClient.cpp `ProcessRequestPartsPacket` reads them in that order.
// src/UploadDiskIOThread.cpp `CreateNormalPackets`:
//   OP_SENDINGPART <filehash 16><start u32><end u32><data[end-start]>.
pub const WIRE_BLOCKS: usize = 3;
/// 真实 eMule/aMule 上传端的回帧粒度（10 KiB），不是 184320。
pub const UPLOAD_CHUNK: u32 = 10_240;

pub fn request_parts(hash: &Hash16, ranges: &[(u32, u32)]) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(hash);
    for i in 0..WIRE_BLOCKS {
        b.u32(ranges.get(i).map(|r| r.0).unwrap_or(0));
    }
    for i in 0..WIRE_BLOCKS {
        b.u32(ranges.get(i).map(|r| r.1).unwrap_or(0));
    }
    frame(PROT_EDONKEY, op::REQUESTPARTS, &b.0)
}

pub fn parse_request_parts(payload: &[u8]) -> Result<(Hash16, Vec<(u32, u32)>), String> {
    let mut c = Cur::new(payload);
    let hash = c.id()?;
    // 经典布局是「hash + N 个 start + N 个 end」。eMule 通常补满 3 组，
    // 但不能假定对端一定发 40 字节：pending 少于 3 块时可能只发 1–2 组。
    // 按可用字节数推导组数，超出的按 3 组截断。
    let count = ((payload.len().saturating_sub(16)) / 8).min(WIRE_BLOCKS);
    if count == 0 {
        return Err("OP_REQUESTPARTS without any range".into());
    }
    let mut starts = [0u32; WIRE_BLOCKS];
    let mut ends = [0u32; WIRE_BLOCKS];
    for s in starts.iter_mut().take(count) {
        *s = c.u32()?;
    }
    for e in ends.iter_mut().take(count) {
        *e = c.u32()?;
    }
    let ranges = (0..count)
        .filter(|&i| ends[i] > starts[i])
        .map(|i| (starts[i], ends[i]))
        .collect();
    Ok((hash, ranges))
}

pub fn sending_part(hash: &Hash16, start: u32, end: u32, data: &[u8]) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(hash);
    b.u32(start);
    b.u32(end);
    b.raw(data);
    frame(PROT_EDONKEY, op::SENDINGPART, &b.0)
}

/// A complete, valid zlib stream built only from DEFLATE *stored* blocks.
///
/// `aMule 2.3.3` receives `OP_COMPRESSEDPART` through its streaming `inflate`
/// path; a stored stream is legal zlib and keeps this crate dependency-free.
/// It makes no compression-ratio claim -- `transize` is only used by aMule for
/// transfer accounting (`CPartFile::WriteToBuffer`); the written range comes
/// from the decompressed byte count.
pub fn zlib_stored(data: &[u8]) -> Vec<u8> {
    // CM=8, CINFO=7, FCHECK chosen so the first two bytes are a multiple of 31.
    let mut out = vec![0x78, 0x01];
    if data.is_empty() {
        // One empty final stored block: BFINAL=1, BTYPE=00, LEN=0, NLEN=0xffff.
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    }
    let mut pos = 0usize;
    while pos < data.len() {
        let n = (data.len() - pos).min(u16::MAX as usize);
        let len = n as u16;
        out.push(if pos + n == data.len() { 0x01 } else { 0x00 });
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&data[pos..pos + n]);
        pos += n;
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&(((b << 16) | a) as u32).to_be_bytes());
    out
}

// --------------------------------------------------- minimal zlib compressor --
//
// aMule's stock uploader falls back to OP_SENDINGPART when compression does
// not actually shrink a block (`CreatePackedPackets`: `togo <= newsize`).
// Stored DEFLATE is legal zlib but always grows the frame, so it cannot
// reproduce that behaviour. This small greedy LZ77 + fixed-Huffman encoder
// keeps the crate dependency-free and gives a real ratio on the patterned
// samples used by FR-25; `zlib_stored` remains the honest fallback.

const L_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83,
    99, 115, 131, 163, 195, 227, 258,
];
const L_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const D_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025,
    1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const D_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11,
    12, 12, 13, 13,
];

struct BitWriter {
    out: Vec<u8>,
    bit: u8,
    acc: u8,
}

impl BitWriter {
    fn new() -> Self {
        BitWriter { out: Vec::new(), bit: 0, acc: 0 }
    }
    fn raw_bit(&mut self, v: bool) {
        if v { self.acc |= 1 << self.bit; }
        self.bit += 1;
        if self.bit == 8 {
            self.out.push(self.acc);
            self.acc = 0;
            self.bit = 0;
        }
    }
    /// Huffman codes are packed MSB-first; the byte stream itself is LSB-first.
    fn code(&mut self, value: u16, len: u8) {
        for i in (0..len).rev() {
            self.raw_bit((value >> i) & 1 != 0);
        }
    }
    /// Extra bits are plain integer bits, LSB-first.
    fn extra(&mut self, value: u32, len: u8) {
        for i in 0..len {
            self.raw_bit((value >> i) & 1 != 0);
        }
    }
    fn literal(&mut self, sym: u16) {
        match sym {
            0..=143 => self.code(0x30 + sym, 8),
            144..=255 => self.code(0x190 + sym - 144, 9),
            256..=279 => self.code(sym - 256, 7),
            _ => self.code(0xc0 + sym - 280, 8),
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.bit != 0 {
            self.out.push(self.acc);
        }
        self.out
    }
}

fn h3(data: &[u8], i: usize) -> usize {
    (((data[i] as usize) << 10) ^ ((data[i + 1] as usize) << 5) ^ (data[i + 2] as usize)) & 0x7fff
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// A real (deflated) zlib stream with fixed Huffman tables. When the result is
/// not smaller than the input we emit the stored form instead, mirroring
/// aMule's `CreatePackedPackets` decision.
pub fn zlib_compact(data: &[u8]) -> Vec<u8> {
    let deflated = zlib_fixed_deflate(data);
    if deflated.len() < data.len() { deflated } else { zlib_stored(data) }
}

fn zlib_fixed_deflate(data: &[u8]) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.raw_bit(true); // BFINAL
    w.raw_bit(true); // BTYPE=01, low bit first
    w.raw_bit(false);

    let mut table = vec![0u32; 32768];
    let mut pos = 0usize;
    while pos < data.len() {
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        if pos + 2 < data.len() {
            let key = h3(data, pos);
            let prev = table[key] as usize;
            if prev > 0 {
                let p = prev - 1;
                let dist = pos - p;
                if dist <= 32768 {
                    let limit = (258).min(data.len() - pos);
                    let mut n = 0;
                    while n < limit && data[p + n] == data[pos + n] {
                        n += 1;
                    }
                    if n >= 3 {
                        best_len = n;
                        best_dist = dist;
                    }
                }
            }
            table[key] = (pos + 1) as u32;
        }
        if best_len >= 3 {
            let li = (0..29)
                .rev()
                .find(|&i| best_len >= L_BASE[i] as usize)
                .unwrap_or(0);
            w.literal(257 + li as u16);
            if L_EXTRA[li] > 0 {
                w.extra((best_len - L_BASE[li] as usize) as u32, L_EXTRA[li]);
            }
            let di = (0..30)
                .rev()
                .find(|&i| best_dist >= D_BASE[i] as usize)
                .unwrap_or(0);
            w.code(di as u16, 5);
            if D_EXTRA[di] > 0 {
                w.extra((best_dist - D_BASE[di] as usize) as u32, D_EXTRA[di]);
            }
            pos += best_len;
        } else {
            w.literal(data[pos] as u16);
            pos += 1;
        }
    }
    w.literal(256); // end of block

    let mut out = vec![0x78, 0x01];
    out.extend_from_slice(&w.finish());
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Stock-aMule-compatible compressed block frame, verified byte-for-byte
/// against a 2026-11-24 capture of a second stock amuled uploader:
/// `hash(16) | start(u32 LE) | compressed_len(u32 LE) | zlib stream`, sent with
/// `PROT_EMULE (0xC5)`. One frame carries one requested block; `start` is the
/// request's exclusive start offset (the block's own start).
pub fn compressed_part(hash: &Hash16, start: u32, data: &[u8]) -> Vec<u8> {
    let z = zlib_compact(data);
    let mut b = Buf::new();
    b.id(hash);
    b.u32(start);
    b.u32(z.len() as u32);
    b.raw(&z);
    frame(PROT_EMULE, op::COMPRESSEDPART, &b.0)
}

pub fn parse_sending_part(payload: &[u8]) -> Result<(Hash16, u32, u32, Vec<u8>), String> {
    let mut c = Cur::new(payload);
    let hash = c.id()?;
    let start = c.u32()?;
    let end = c.u32()?;
    if end < start {
        return Err(format!("SENDINGPART end {end} < start {start}"));
    }
    let n = (end - start) as usize;
    if c.remaining() < n {
        return Err(format!("SENDINGPART truncated: need {n}, have {}", c.remaining()));
    }
    let data = c.raw(n)?.to_vec();
    Ok((hash, start, end, data))
}

pub fn end_of_download(hash: &Hash16) -> Vec<u8> {
    frame(PROT_EDONKEY, op::END_OF_DOWNLOAD, hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip_and_length_semantics() {
        let f = frame(PROT_EDONKEY, op::HELLO, b"abc");
        assert_eq!(f[0], 0xE3);
        assert_eq!(&f[1..5], &4u32.to_le_bytes()); // opcode + 3 payload bytes
        assert_eq!(f[5], 0x01);
        assert_eq!(&f[6..], b"abc");
        let p = parse_frame(&f).unwrap();
        assert_eq!(p.payload, b"abc");
        assert_eq!(p.opcode, op::HELLO);
        assert!(parse_frame(&[0xE3, 0x00, 0, 0, 0, 0x01]).is_err());
    }

    #[test]
    fn hello_roundtrip_both_directions() {
        let h = default_hello([7u8; 16], 4662, 4672, 4672, "coldrevive");
        let req = hello_frame(&h);
        let p = parse_frame(&req).unwrap();
        assert_eq!(p.opcode, op::HELLO);
        assert_eq!(p.payload[0], 16);
        let back = parse_hello(&p.payload, true).unwrap();
        assert_eq!(back.user_hash, [7u8; 16]);
        assert_eq!(back.tags.len(), h.tags.len());
        assert_eq!(find_tag(&back.tags, ct::NAME).unwrap().value, TagValue::Str("coldrevive".into()));

        let ans = hello_answer_frame(&h);
        let p2 = parse_frame(&ans).unwrap();
        assert_eq!(p2.opcode, op::HELLOANSWER);
        assert_ne!(p2.payload[0], 16); // no hash-size byte on the answer
        let back2 = parse_hello(&p2.payload, false).unwrap();
        assert_eq!(back2.tcp_port, 4662);
    }

    #[test]
    fn compact_meta_tag_form_is_accepted() {
        // `[0x83 (type 0x03 | 0x80)][name 0x11][value u32]` -- the form
        // WriteNewEd2kTag emits, which KAD's ReadTag would reject.
        let mut raw = vec![0x83, ct::VERSION];
        raw.extend_from_slice(&0x3cu32.to_le_bytes());
        let mut c = Cur::new(&raw);
        let t = read_meta_tag(&mut c).unwrap();
        assert_eq!(t.name, vec![ct::VERSION]);
        assert_eq!(tag_as_u32(&t), Some(0x3c));
    }

    #[test]
    fn request_parts_uses_grouped_offset_layout() {
        let h = [9u8; 16];
        let f = request_parts(&h, &[(0, 100), (100, 200)]);
        let p = parse_frame(&f).unwrap();
        assert_eq!(p.opcode, op::REQUESTPARTS);
        assert_eq!(p.payload.len(), 16 + 24);
        let (hash, ranges) = parse_request_parts(&p.payload).unwrap();
        assert_eq!(hash, h);
        assert_eq!(ranges, vec![(0, 100), (100, 200)]);
        // grouped, not interleaved
        assert_eq!(&p.payload[16..20], &0u32.to_le_bytes());
        assert_eq!(&p.payload[20..24], &100u32.to_le_bytes());
    }

    #[test]
    fn sending_part_and_hashset_roundtrip() {
        let h = [3u8; 16];
        let f = sending_part(&h, 100, 104, b"data");
        let p = parse_frame(&f).unwrap();
        let (hash, start, end, data) = parse_sending_part(&p.payload).unwrap();
        assert_eq!((hash, start, end, data), (h, 100, 104, b"data".to_vec()));

        let parts = vec![[1u8; 16], [2u8; 16]];
        let p2 = parse_frame(&hashset_answer(&h, &parts)).unwrap();
        assert_eq!(parse_hashset_answer(&p2.payload).unwrap(), (h, parts));
    }

    #[test]
    fn request_parts_accepts_fewer_than_three_pairs() {
        // eMule 补满 3 组；但只发 1–2 组的对端不能被当成错误帧。
        let h = [3u8; 16];
        let mut one = Vec::new();
        one.extend_from_slice(&h);
        one.extend_from_slice(&0u32.to_le_bytes());
        one.extend_from_slice(&184_320u32.to_le_bytes());
        assert_eq!(parse_request_parts(&one).unwrap().1, vec![(0, 184_320)]);

        let mut two = Vec::new();
        two.extend_from_slice(&h);
        for v in [0u32, 184_320] {
            two.extend_from_slice(&v.to_le_bytes());
        }
        for v in [184_320u32, 368_640] {
            two.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(
            parse_request_parts(&two).unwrap().1,
            vec![(0, 184_320), (184_320, 368_640)]
        );
        assert!(parse_request_parts(&h).is_err());
    }

    #[test]
    fn file_status_answer_is_hash_plus_zero_part_status() {
        let h = [7u8; 16];
        let p = parse_frame(&file_status(&h)).unwrap();
        assert_eq!(p.opcode, op::FILESTATUS);
        assert_eq!(p.payload.len(), 18);
        assert_eq!(&p.payload[..16], &h);
        assert_eq!(&p.payload[16..], &[0, 0]);
    }

    #[test]
    fn file_name_answer_roundtrip() {
        let h = [5u8; 16];
        let p = parse_frame(&file_name_answer(&h, "a.txt")).unwrap();
        assert_eq!(p.opcode, op::REQFILENAMEANSWER);
        assert_eq!(parse_file_name_answer(&p.payload).unwrap(), (h, "a.txt".into()));
    }

    #[test]
    fn zlib_compact_beats_stored_on_patterned_block() {
        let block: Vec<u8> = (0..184_320).map(|i| ((i * 137 + 5) % 256) as u8).collect();
        let compact = zlib_compact(&block);
        let stored = zlib_stored(&block);
        assert_eq!(&compact[..2], &[0x78, 0x01]);
        assert!(compact.len() < block.len() / 10, "compact={}", compact.len());
        assert!(stored.len() > block.len());
        // aMule falls back to 0x46 when compression does not shrink the block.
        assert_eq!(zlib_compact(&[]).len(), zlib_stored(&[]).len());
    }

    #[test]
    fn compressed_part_matches_stock_field_layout() {
        let h = [9u8; 16];
        let data = vec![0x5a; 184_320];
        let p = parse_frame(&compressed_part(&h, 184_320, &data)).unwrap();
        assert_eq!(p.protocol, PROT_EMULE);
        assert_eq!(p.opcode, op::COMPRESSEDPART);
        assert_eq!(&p.payload[..16], &h);
        assert_eq!(u32::from_le_bytes(p.payload[16..20].try_into().unwrap()), 184_320);
        let zlen = u32::from_le_bytes(p.payload[20..24].try_into().unwrap()) as usize;
        assert_eq!(zlen, p.payload.len() - 24);
        assert_eq!(&p.payload[24..26], &[0x78, 0x01]);
        // FCHECK is verified on the big-endian 16-bit header value, not the
        // arithmetic sum of its two bytes: 0x7801 % 31 == 0.
        assert_eq!(u16::from_be_bytes([p.payload[24], p.payload[25]]) % 31, 0);
    }

    #[test]
    fn zlib_stored_uses_final_block_and_adler32() {
        let z = zlib_stored(b"hello");
        // 2-byte zlib header, 5-byte stored block header, 5 data bytes, 4-byte adler32.
        assert_eq!(z.len(), 16);
        assert_eq!(&z[2..7], &[0x01, 0x05, 0x00, 0xfa, 0xff]);
        assert_eq!(&z[7..12], b"hello");
        // adler32("hello") = 0x062c0215, big endian on the wire.
        assert_eq!(&z[12..], &[0x06, 0x2c, 0x02, 0x15]);
        // Multi-block data (>65535) must keep earlier blocks non-final.
        let big = vec![7u8; 70_000];
        let zb = zlib_stored(&big);
        assert_eq!(zb[2], 0x00);
        assert_eq!(zb[2 + 5 + 65_535], 0x01);
    }
}

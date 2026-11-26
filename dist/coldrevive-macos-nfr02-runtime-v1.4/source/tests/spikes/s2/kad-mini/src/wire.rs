//! eMule Kademlia v2 wire codec (minimal subset for the S2 spike).
//!
//! Every constant below is transcribed from aMule `master`
//! @ 6a1105634d81242a581a0f36a7e00379b12a83f3. Source paths are given inline.
//! This is a PROTOCOL PROBE, not a KAD implementation: no routing-zone tree,
//! no UDP firewall tester, no sender/receiver key (obfuscation), no zlib
//! packing, no `nodes.dat`. Those omissions are the subject of S2-C2.

use crate::sha1::{hex, sha1};

// src/include/protocol/Protocols.h
pub const PROTO_KAD: u8 = 0xE4;
pub const PROTO_KAD_PACKED: u8 = 0xE5;

// src/include/protocol/kad2/Client2Client/UDP.h
// Full table is kept even where this probe only uses a subset: the constants
// are part of the audit trail showing which opcodes were verified.
#[allow(dead_code)]
pub mod op {
    pub const BOOTSTRAP_REQ: u8 = 0x01;
    pub const BOOTSTRAP_RES: u8 = 0x09;
    pub const HELLO_REQ: u8 = 0x11;
    pub const HELLO_RES: u8 = 0x19;
    pub const REQ: u8 = 0x21;
    pub const HELLO_RES_ACK: u8 = 0x22;
    pub const RES: u8 = 0x29;
    pub const SEARCH_KEY_REQ: u8 = 0x33;
    pub const SEARCH_SOURCE_REQ: u8 = 0x34;
    pub const SEARCH_NOTES_REQ: u8 = 0x35;
    pub const SEARCH_RES: u8 = 0x3B;
    pub const PUBLISH_KEY_REQ: u8 = 0x43;
    pub const PUBLISH_SOURCE_REQ: u8 = 0x44;
    pub const PUBLISH_NOTES_REQ: u8 = 0x45;
    pub const PUBLISH_RES: u8 = 0x4B;
    pub const PUBLISH_RES_ACK: u8 = 0x4C;
    pub const FIREWALLED2_REQ: u8 = 0x53;
    pub const PING: u8 = 0x60;
    pub const PONG: u8 = 0x61;
    pub const FIREWALLUDP: u8 = 0x62;
}

// src/include/protocol/kad2/Constants.h
pub const KAD_VERSION: u8 = 0x08;
// src/kademlia/kademlia/Defines.h
pub const SEARCHTOLERANCE: u32 = 16_777_216; // 2^24
pub const K: usize = 10;
pub const ALPHA_QUERY: usize = 5;

// src/include/tags/TagTypes.h
#[allow(dead_code)]
pub mod tag_type {
    pub const HASH16: u8 = 0x01;
    pub const STRING: u8 = 0x02;
    pub const UINT32: u8 = 0x03;
    pub const FLOAT32: u8 = 0x04;
    pub const BOOL: u8 = 0x05;
    pub const BOOLARRAY: u8 = 0x06;
    pub const BLOB: u8 = 0x07;
    pub const UINT16: u8 = 0x08;
    pub const UINT8: u8 = 0x09;
    pub const BSOB: u8 = 0x0A;
    pub const UINT64: u8 = 0x0B;
    pub const STR1: u8 = 0x11;
    pub const STR16: u8 = 0x20;
}

// src/include/tags/FileTags.h
pub const TAG_FILENAME: u8 = 0x01;
pub const TAG_FILESIZE: u8 = 0x02;
pub const TAG_FILESIZE_HI: u8 = 0x3A;
pub const TAG_KADMISCOPTIONS: u8 = 0xF2;
pub const TAG_ENCRYPTION: u8 = 0xF3;
pub const TAG_SOURCEUPORT: u8 = 0xFC;
pub const TAG_SOURCEPORT: u8 = 0xFD;
pub const TAG_SOURCEIP: u8 = 0xFE;
pub const TAG_SOURCETYPE: u8 = 0xFF;

// src/kademlia/kademlia/Search.cpp, case STOREFILE (non-firewalled branch).
// 1 = HighID source; 4 = HighID source of a >4 GiB file; 3/5 = firewalled;
// 6 = firewalled with direct UDP callback. Publishing without a SOURCETYPE
// tag makes Process2PublishSourceRequest drop the entry (m_bSource == false).
pub const SOURCETYPE_HIGHID: u8 = 1;

pub const NAMESPACE: &str = "ColdRevive_V1:";
pub const ID_LEN: usize = 16; // KAD ID space is 128-bit, NOT SHA-1's 160-bit.
pub type KadId = [u8; ID_LEN];

/// Derive a 128-bit KAD key from a namespaced logical name.
///
/// PROTO §1 defines `Target_Key = SHA1(NAMESPACE_PREFIX + file_hash)` (160-bit).
/// eMule KAD IDs are 128-bit, so this probe truncates SHA-1 to its first 16
/// bytes. The truncation rule is a PROTO/ADR-001 decision that is still open;
/// it is recorded as a finding rather than silently assumed.
pub fn namespaced_key(logical: &str) -> KadId {
    let digest = sha1(format!("{NAMESPACE}{logical}").as_bytes());
    let mut id = [0u8; ID_LEN];
    id.copy_from_slice(&digest[..ID_LEN]);
    id
}

pub fn plain_key(logical: &str) -> KadId {
    let digest = sha1(logical.as_bytes());
    let mut id = [0u8; ID_LEN];
    id.copy_from_slice(&digest[..ID_LEN]);
    id
}

pub fn node_id(seed: &str) -> KadId {
    let digest = sha1(format!("coldrevive.s2.node:{seed}").as_bytes());
    let mut id = [0u8; ID_LEN];
    id.copy_from_slice(&digest[..ID_LEN]);
    id
}

pub fn xor_distance(a: &KadId, b: &KadId) -> u128 {
    u128::from_be_bytes(*a) ^ u128::from_be_bytes(*b)
}

/// `CUInt128::Get32BitChunk(0)` is the most significant 32 bits
/// (src/kademlia/utils/UInt128.h `Set32BitChunk`: "Chunk 0 being the most significant").
pub fn top_chunk(a: &KadId, b: &KadId) -> u32 {
    (xor_distance(a, b) >> 96) as u32
}

/// `Process2PublishKeyRequest`:
/// `if (distance.Get32BitChunk(0) > SEARCHTOLERANCE && !IsLanIP(ip)) return;`
/// i.e. LAN sources bypass the tolerance zone entirely.
pub fn publish_allowed(owner: &KadId, key: &KadId, peer_is_lan: bool) -> bool {
    peer_is_lan || top_chunk(owner, key) <= SEARCHTOLERANCE
}

// ---------------------------------------------------------------- byte IO --

pub struct Buf(pub Vec<u8>);

impl Buf {
    pub fn new() -> Self {
        Buf(Vec::new())
    }
    pub fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn id(&mut self, v: &KadId) {
        self.0.extend_from_slice(v);
    }
    pub fn raw(&mut self, v: &[u8]) {
        self.0.extend_from_slice(v);
    }
}

pub struct Cur<'a> {
    pub b: &'a [u8],
    pub p: usize,
}

impl<'a> Cur<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Cur { b, p: 0 }
    }
    pub fn remaining(&self) -> usize {
        self.b.len().saturating_sub(self.p)
    }
    fn need(&self, n: usize) -> Result<(), String> {
        if self.remaining() < n {
            Err(format!("truncated: need {n}, have {}", self.remaining()))
        } else {
            Ok(())
        }
    }
    pub fn u8(&mut self) -> Result<u8, String> {
        self.need(1)?;
        let v = self.b[self.p];
        self.p += 1;
        Ok(v)
    }
    pub fn u16(&mut self) -> Result<u16, String> {
        self.need(2)?;
        let v = u16::from_le_bytes([self.b[self.p], self.b[self.p + 1]]);
        self.p += 2;
        Ok(v)
    }
    pub fn u32(&mut self) -> Result<u32, String> {
        self.need(4)?;
        let mut a = [0u8; 4];
        a.copy_from_slice(&self.b[self.p..self.p + 4]);
        self.p += 4;
        Ok(u32::from_le_bytes(a))
    }
    pub fn u64(&mut self) -> Result<u64, String> {
        self.need(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(&self.b[self.p..self.p + 8]);
        self.p += 8;
        Ok(u64::from_le_bytes(a))
    }
    pub fn id(&mut self) -> Result<KadId, String> {
        self.need(ID_LEN)?;
        let mut a = [0u8; ID_LEN];
        a.copy_from_slice(&self.b[self.p..self.p + ID_LEN]);
        self.p += ID_LEN;
        Ok(a)
    }
    pub fn raw(&mut self, n: usize) -> Result<&'a [u8], String> {
        self.need(n)?;
        let v = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(v)
    }
}

/// `[0xE4][opcode][payload]` — src/kademlia/net/KademliaUDPListener.cpp
/// `ProcessPacket` reads `opcode = data[1]; packetData = data + 2`.
pub fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + payload.len());
    out.push(PROTO_KAD);
    out.push(opcode);
    out.extend_from_slice(payload);
    out
}

pub fn parse_frame(data: &[u8]) -> Result<(u8, &[u8]), String> {
    if data.len() < 2 {
        return Err("frame shorter than 2 bytes".into());
    }
    match data[0] {
        PROTO_KAD => Ok((data[1], &data[2..])),
        PROTO_KAD_PACKED => Err("0xE5 zlib-packed frame: unsupported by this probe".into()),
        other => Err(format!("not a KAD frame: header 0x{other:02x}")),
    }
}

pub fn opcode_name(op: u8) -> &'static str {
    match op {
        op::BOOTSTRAP_REQ => "KADEMLIA2_BOOTSTRAP_REQ",
        op::BOOTSTRAP_RES => "KADEMLIA2_BOOTSTRAP_RES",
        op::HELLO_REQ => "KADEMLIA2_HELLO_REQ",
        op::HELLO_RES => "KADEMLIA2_HELLO_RES",
        op::REQ => "KADEMLIA2_REQ",
        op::RES => "KADEMLIA2_RES",
        op::SEARCH_KEY_REQ => "KADEMLIA2_SEARCH_KEY_REQ",
        op::SEARCH_RES => "KADEMLIA2_SEARCH_RES",
        op::PUBLISH_KEY_REQ => "KADEMLIA2_PUBLISH_KEY_REQ",
        op::PUBLISH_SOURCE_REQ => "KADEMLIA2_PUBLISH_SOURCE_REQ",
        op::SEARCH_SOURCE_REQ => "KADEMLIA2_SEARCH_SOURCE_REQ",
        op::PUBLISH_RES => "KADEMLIA2_PUBLISH_RES",
        op::PING => "KADEMLIA2_PING",
        op::PONG => "KADEMLIA2_PONG",
        _ => "UNKNOWN",
    }
}

// ------------------------------------------------------------------- tags --

/// eMule `CTag` new-ED2K encoding (src/Tag.cpp `WriteNewEd2kTag`).
/// One-byte tag name is signalled by `type | 0x80`; otherwise the name is a
/// uint16-length string. STR1..STR16 carry the length in the type byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagValue {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    Str(String),
    Hash16([u8; 16]),
    Raw(u8, Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub name: Vec<u8>,
    pub value: TagValue,
}

impl Tag {
    pub fn int1(name: u8, v: u8) -> Self {
        Tag { name: vec![name], value: TagValue::U8(v) }
    }
    pub fn int2(name: u8, v: u16) -> Self {
        Tag { name: vec![name], value: TagValue::U16(v) }
    }
    pub fn int4(name: u8, v: u32) -> Self {
        Tag { name: vec![name], value: TagValue::U32(v) }
    }
    pub fn str1(name: u8, v: &str) -> Self {
        Tag { name: vec![name], value: TagValue::Str(v.into()) }
    }

    pub fn encode(&self, b: &mut Buf) -> Result<(), String> {
        // KAD tag framing is NOT the ed2k/met framing.
        //
        // aMule's KAD paths (PUBLISH_KEY_REQ, SEARCH_RES) read tags with
        // `CFileDataIO::ReadTag`, which does:
        //     type = ReadUInt8();
        //     name = ReadString(false);          // uint16 length + bytes, ALWAYS
        //     switch (type) { ... default: throw "Invalid Kad tag type" }
        // whereas the ed2k/met path (`CTag::CTag(CFileDataIO&, bool)` and
        // `CTag::WriteNewEd2kTag`) uses `[type|0x80][name:1]` for numeric names.
        // We originally wrote the ed2k form here; every tag then threw inside
        // aMule's `Process2PublishKeyRequest` and no PUBLISH_RES was ever sent
        // (v2.4.26, PLAN §19). Numeric names on the wire are a length-1 string:
        // `TAG_FILENAME` is literally `wxT("\x01")`.
        //
        // `ReadTag`'s switch also has NO case for TAGTYPE_STR1..STR22 and none
        // for TAGTYPE_BLOB, so strings must always be TAGTYPE_STRING here --
        // the compact STRn types that `WriteNewEd2kTag` would pick are invalid
        // on this path.
        let ty = match &self.value {
            TagValue::U8(_) => tag_type::UINT8,
            TagValue::U16(_) => tag_type::UINT16,
            TagValue::U32(_) => tag_type::UINT32,
            TagValue::U64(_) => tag_type::UINT64,
            TagValue::Hash16(_) => tag_type::HASH16,
            TagValue::Raw(t, _) => *t,
            TagValue::Str(_) => tag_type::STRING,
        };
        b.u8(ty);
        b.u16(self.name.len() as u16);
        b.raw(&self.name);
        match &self.value {
            TagValue::U8(v) => b.u8(*v),
            TagValue::U16(v) => b.u16(*v),
            TagValue::U32(v) => b.u32(*v),
            TagValue::U64(v) => b.u64(*v),
            TagValue::Hash16(v) => b.raw(v),
            TagValue::Raw(_, v) => b.raw(v),
            TagValue::Str(s) => {
                b.u16(s.len() as u16);
                b.raw(s.as_bytes());
            }
        }
        Ok(())
    }

    pub fn decode(c: &mut Cur<'_>) -> Result<Self, String> {
        let ty = c.u8()?;
        if (0x11..=0x26).contains(&ty) || ty == 0x07 {
            return Err(format!("tag type 0x{ty:02x} is not readable by aMule's Kad ReadTag"));
        }
        let n = c.u16()? as usize;
        let name = c.raw(n)?.to_vec();
        let value = match ty {
            tag_type::UINT8 => TagValue::U8(c.u8()?),
            tag_type::UINT16 => TagValue::U16(c.u16()?),
            tag_type::UINT32 => TagValue::U32(c.u32()?),
            tag_type::UINT64 => TagValue::U64(c.u64()?),
            tag_type::HASH16 => {
                let mut h = [0u8; 16];
                h.copy_from_slice(c.raw(16)?);
                TagValue::Hash16(h)
            }
            tag_type::STRING => {
                let n = c.u16()? as usize;
                TagValue::Str(String::from_utf8_lossy(c.raw(n)?).into_owned())
            }
            tag_type::BSOB => {
                let n = c.u32()? as usize;
                TagValue::Raw(ty, c.raw(n)?.to_vec())
            }
            other => return Err(format!("unsupported tag type 0x{other:02x}")),
        };
        Ok(Tag { name, value })
    }
}

pub fn write_tags(b: &mut Buf, tags: &[Tag]) -> Result<(), String> {
    b.u8(tags.len() as u8);
    for t in tags {
        t.encode(b)?;
    }
    Ok(())
}

pub fn read_tags(c: &mut Cur<'_>) -> Result<Vec<Tag>, String> {
    let n = c.u8()? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(Tag::decode(c)?);
    }
    Ok(out)
}

// --------------------------------------------------------------- contacts --

/// `<id 16><ip 4><udp 2><tcp 2><version 1>` = 25 bytes.
/// src/kademlia/net/KademliaUDPListener.cpp `Process2BootstrapRequest`.
///
/// BYTE ORDER (v2.4.14): `Contact.ip` is the human dotted-quad order
/// `[a,b,c,d]`, but KAD puts the address on the wire in REVERSED order
/// `[d,c,b,a]`. aMule stores KAD IPs in *host order* (see
/// `NetworkFunctions.h`: `KadIPToString` renders `ip>>24` first, and
/// `KadDlg.cpp` notes "kad expects host order"), while
/// `CFileDataIO::WriteUInt32` writes little-endian on x86. For a.b.c.d the
/// host-order integer is `a<<24|b<<16|c<<8|d`, whose LE bytes are d,c,b,a.
/// Confirmed empirically: all 200 contacts in a real nodes.dat decode to
/// globally-routable ISP ranges only when reversed (reversed first octets
/// cluster on 79/81/82/84/88/91/93/185); the un-reversed decode yields 19
/// RFC1918 addresses and multicast/reserved first octets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contact {
    pub id: KadId,
    pub ip: [u8; 4],
    pub udp: u16,
    pub tcp: u16,
    pub version: u8,
}

impl Contact {
    pub fn encode(&self, b: &mut Buf) {
        b.id(&self.id);
        // wire order is the reverse of dotted-quad order
        b.raw(&[self.ip[3], self.ip[2], self.ip[1], self.ip[0]]);
        b.u16(self.udp);
        b.u16(self.tcp);
        b.u8(self.version);
    }
    pub fn decode(c: &mut Cur<'_>) -> Result<Self, String> {
        let id = c.id()?;
        let mut ip = [0u8; 4];
        ip.copy_from_slice(c.raw(4)?);
        ip.reverse(); // wire order -> dotted-quad order
        Ok(Contact { id, ip, udp: c.u16()?, tcp: c.u16()?, version: c.u8()? })
    }
    pub fn addr(&self) -> String {
        format!("{}.{}.{}.{}:{}", self.ip[0], self.ip[1], self.ip[2], self.ip[3], self.udp)
    }
}

pub fn closest<'a>(target: &KadId, contacts: &'a [Contact], n: usize) -> Vec<&'a Contact> {
    let mut v: Vec<&Contact> = contacts.iter().collect();
    v.sort_by_key(|c| xor_distance(&c.id, target));
    v.truncate(n);
    v
}

// --------------------------------------------------------------- messages --

pub fn bootstrap_req() -> Vec<u8> {
    frame(op::BOOTSTRAP_REQ, &[])
}

pub fn bootstrap_res(self_contact: &Contact, contacts: &[Contact]) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(&self_contact.id);
    b.u16(self_contact.tcp);
    b.u8(KAD_VERSION);
    b.u16(contacts.len() as u16);
    for c in contacts {
        c.encode(&mut b);
    }
    frame(op::BOOTSTRAP_RES, &b.0)
}

pub struct BootstrapRes {
    pub self_id: KadId,
    pub self_tcp: u16,
    pub version: u8,
    pub contacts: Vec<Contact>,
}

pub fn parse_bootstrap_res(payload: &[u8]) -> Result<BootstrapRes, String> {
    let mut c = Cur::new(payload);
    let self_id = c.id()?;
    let self_tcp = c.u16()?;
    let version = c.u8()?;
    let n = c.u16()? as usize;
    let mut contacts = Vec::with_capacity(n);
    for _ in 0..n {
        contacts.push(Contact::decode(&mut c)?);
    }
    Ok(BootstrapRes { self_id, self_tcp, version, contacts })
}

/// HELLO_REQ / HELLO_RES share `<id><tcp><version><tags>`.
/// src/kademlia/net/KademliaUDPListener.cpp `SendMyDetails` / `AddContact2`.
pub fn hello(opcode: u8, contact: &Contact, extra_tags: &[Tag]) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(&contact.id);
    b.u16(contact.tcp);
    b.u8(KAD_VERSION);
    let mut tags = vec![Tag::int2(TAG_SOURCEUPORT, contact.udp)];
    tags.extend_from_slice(extra_tags);
    write_tags(&mut b, &tags).expect("tag encode is infallible for these types");
    frame(opcode, &b.0)
}

pub struct Hello {
    pub id: KadId,
    pub tcp: u16,
    pub version: u8,
    pub tags: Vec<Tag>,
    pub udp_from_tag: Option<u16>,
}

pub fn parse_hello(payload: &[u8]) -> Result<Hello, String> {
    let mut c = Cur::new(payload);
    let id = c.id()?;
    let tcp = c.u16()?;
    let version = c.u8()?;
    let tags = read_tags(&mut c)?;
    let udp_from_tag = tags.iter().find_map(|t| match (&t.name[..], &t.value) {
        ([TAG_SOURCEUPORT], TagValue::U16(v)) => Some(*v),
        ([TAG_SOURCEUPORT], TagValue::U8(v)) => Some(*v as u16),
        ([TAG_SOURCEUPORT], TagValue::U32(v)) => Some(*v as u16),
        _ => None,
    });
    Ok(Hello { id, tcp, version, tags, udp_from_tag })
}

/// `KADEMLIA2_REQ`: `<type/contactCount 1><target 16><receiverID 16>`.
/// src/kademlia/kademlia/Search.cpp `StorePacket` (REQUEST branch).
pub fn find_node_req(target: &KadId, receiver: &KadId, wanted: u8) -> Vec<u8> {
    let mut b = Buf::new();
    b.u8(wanted & 0x1F);
    b.id(target);
    b.id(receiver);
    frame(op::REQ, &b.0)
}

pub struct FindNodeReq {
    pub wanted: u8,
    pub target: KadId,
    pub receiver: KadId,
}

pub fn parse_find_node_req(payload: &[u8]) -> Result<FindNodeReq, String> {
    let mut c = Cur::new(payload);
    let wanted = c.u8()? & 0x1F;
    let target = c.id()?;
    let receiver = c.id()?;
    Ok(FindNodeReq { wanted, target, receiver })
}

pub fn find_node_res(target: &KadId, contacts: &[Contact]) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(target);
    b.u8(contacts.len() as u8);
    for c in contacts {
        c.encode(&mut b);
    }
    frame(op::RES, &b.0)
}

pub struct FindNodeRes {
    pub target: KadId,
    pub contacts: Vec<Contact>,
}

pub fn parse_find_node_res(payload: &[u8]) -> Result<FindNodeRes, String> {
    let mut c = Cur::new(payload);
    let target = c.id()?;
    let n = c.u8()? as usize;
    let mut contacts = Vec::with_capacity(n);
    for _ in 0..n {
        contacts.push(Contact::decode(&mut c)?);
    }
    Ok(FindNodeRes { target, contacts })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    /// In `PUBLISH_KEY_REQ` this is the *file/source ID*; the keyword hash is
    /// the packet's leading target. Value is carried by tags (KAD is not a KV store).
    pub source_id: KadId,
    pub tags: Vec<Tag>,
}

pub fn publish_key_req(key: &KadId, entries: &[IndexEntry]) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(key);
    b.u16(entries.len() as u16);
    for e in entries {
        b.id(&e.source_id);
        write_tags(&mut b, &e.tags).expect("tag encode is infallible for these types");
    }
    frame(op::PUBLISH_KEY_REQ, &b.0)
}

pub struct PublishKeyReq {
    pub key: KadId,
    pub entries: Vec<IndexEntry>,
}

pub fn parse_publish_key_req(payload: &[u8]) -> Result<PublishKeyReq, String> {
    let mut c = Cur::new(payload);
    let key = c.id()?;
    let n = c.u16()? as usize;
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        let source_id = c.id()?;
        let tags = read_tags(&mut c)?;
        entries.push(IndexEntry { source_id, tags });
    }
    Ok(PublishKeyReq { key, entries })
}

/// `PUBLISH_RES`: `<key 16><load 1>` (+ optional options byte for ACK request).
pub fn publish_res(key: &KadId, load: u8) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(key);
    b.u8(load);
    frame(op::PUBLISH_RES, &b.0)
}

pub struct PublishRes {
    pub key: KadId,
    pub load: u8,
}

pub fn parse_publish_res(payload: &[u8]) -> Result<PublishRes, String> {
    let mut c = Cur::new(payload);
    Ok(PublishRes { key: c.id()?, load: c.u8()? })
}

/// `SEARCH_KEY_REQ`: `<target 16><startPosition 2>`; bit 0x8000 marks a
/// restrictive search followed by an expression tree. We always send 0.
pub fn search_key_req(key: &KadId) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(key);
    b.u16(0);
    frame(op::SEARCH_KEY_REQ, &b.0)
}

pub fn parse_search_key_req(payload: &[u8]) -> Result<KadId, String> {
    let mut c = Cur::new(payload);
    let key = c.id()?;
    let start = c.u16()?;
    if start & 0x8000 != 0 {
        return Err("restrictive search expression tree: unsupported by this probe".into());
    }
    Ok(key)
}

/// `SEARCH_RES` (KAD2): `<senderID 16><target 16><count 2>[<answer 16><tags>]*`.
pub fn search_res(sender: &KadId, key: &KadId, answers: &[IndexEntry]) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(sender);
    b.id(key);
    b.u16(answers.len() as u16);
    for a in answers {
        b.id(&a.source_id);
        write_tags(&mut b, &a.tags).expect("tag encode is infallible for these types");
    }
    frame(op::SEARCH_RES, &b.0)
}

pub struct SearchRes {
    pub sender: KadId,
    pub target: KadId,
    pub answers: Vec<IndexEntry>,
}

pub fn parse_search_res(payload: &[u8]) -> Result<SearchRes, String> {
    let mut c = Cur::new(payload);
    let sender = c.id()?;
    let target = c.id()?;
    let n = c.u16()? as usize;
    let mut answers = Vec::with_capacity(n);
    for _ in 0..n {
        let source_id = c.id()?;
        let tags = read_tags(&mut c)?;
        answers.push(IndexEntry { source_id, tags });
    }
    Ok(SearchRes { sender, target, answers })
}

// ---------------------------------------------- L2: KAD source dictionary --
//
// src/kademlia/net/KademliaUDPListener.cpp::SendPublishSourcePacket /
// Process2PublishSourceRequest, and kademlia/Search.cpp case STOREFILE.
//
// PUBLISH_SOURCE_REQ (0x44), Kad2 only, contact version >= 4:
//     <fileID 16> <sourceID 16> <tagcount u8> <tags...>
// Note the differences from PUBLISH_KEY_REQ (0x43):
//   * there is NO u16 entry count -- the packet carries exactly one source;
//   * the tag count is ONE byte (CFileDataIO::WriteTagPtrList), not two;
//   * `distance = myKadID XOR fileID` is checked against SEARCHTOLERANCE, i.e.
//     the target node must be near the FILE hash, not the keyword hash;
//   * an entry with no TAG_SOURCETYPE tag is silently discarded.
// The source's IP/UDP port are taken from the datagram header (m_uIP /
// m_uUDPport); TAG_SOURCEIP is added by the receiver. Only TCP port, UDP port
// override, size and type need to be sent as tags.

pub struct PublishSourceReq {
    pub file_id: KadId,
    pub source_id: KadId,
    pub tags: Vec<Tag>,
}

pub fn publish_source_req(file_id: &KadId, source_id: &KadId, tags: &[Tag]) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(file_id);
    b.id(source_id);
    write_tags(&mut b, tags).expect("tag encode is infallible for these types");
    frame(op::PUBLISH_SOURCE_REQ, &b.0)
}

pub fn parse_publish_source_req(payload: &[u8]) -> Result<PublishSourceReq, String> {
    let mut c = Cur::new(payload);
    let file_id = c.id()?;
    let source_id = c.id()?;
    let tags = read_tags(&mut c)?;
    Ok(PublishSourceReq { file_id, source_id, tags })
}

/// The tag set a non-firewalled (HighID) eMule publishes for itself as a source.
/// Mirrors `Search.cpp` case STOREFILE, `theApp->IsFirewalled() == false`.
pub fn source_tags(tcp_port: u16, udp_port: u16, file_size: u64) -> Vec<Tag> {
    vec![
        Tag::int1(TAG_SOURCETYPE, SOURCETYPE_HIGHID),
        Tag::int2(TAG_SOURCEPORT, tcp_port),
        Tag::int2(TAG_SOURCEUPORT, udp_port),
        Tag::int4(TAG_FILESIZE, file_size.min(u32::MAX as u64) as u32),
        Tag::int1(TAG_ENCRYPTION, 0),
    ]
}

/// SEARCH_SOURCE_REQ (0x34): `<fileID 16> <startPosition u16> <fileSize u64>`.
/// `Process2SearchSourceRequest` masks startPosition with 0x7FFF.
pub fn search_source_req(file_id: &KadId, file_size: u64) -> Vec<u8> {
    let mut b = Buf::new();
    b.id(file_id);
    b.u16(0);
    b.u64(file_size);
    frame(op::SEARCH_SOURCE_REQ, &b.0)
}

pub struct SearchSourceReq {
    pub file_id: KadId,
    pub start_position: u16,
    pub file_size: u64,
}

pub fn parse_search_source_req(payload: &[u8]) -> Result<SearchSourceReq, String> {
    let mut c = Cur::new(payload);
    let file_id = c.id()?;
    let start_position = c.u16()? & 0x7FFF;
    let file_size = c.u64()?;
    Ok(SearchSourceReq { file_id, start_position, file_size })
}

pub fn id_hex(id: &KadId) -> String {
    hex(id)
}

// ------------------------------------------------- KAD UDP obfuscation ----
//
// aMule wraps every KAD UDP datagram in a "basic obfuscation" envelope
// (src/EncryptedDatagramSocket.cpp EncryptSendClient / DecryptReceivedClient).
// For KAD an 8-byte crypt header (no padding) precedes the plain Kademlia
// frame `[0xE4][opcode][payload]`. The whole datagram is RC4-encrypted under a
// key derived from the *receiver's* KadID (try-0) on the way out, or from the
// receiver-verify key (try-2) on the way back.
//
// Admission reality (verified against aMule master @ 6a110563):
// KademliaUDPListener's `Process2PublishKeyRequest` only *logs* the received
// `validReceiverKey` — it does NOT gate publish/search on it. The real
// admission check is the decryption layer: `DecryptReceivedClient` silently
// drops any datagram that does not decrypt to the obfuscation magic. A
// CLEARTEXT KAD frame is still accepted (its first byte 0xE4 is a known
// protocol byte), but an *obfuscated* request the peer cannot decrypt is
// dropped. We therefore send every live request obfuscated with the peer's own
// KadID (try-0) so the peer decrypts and processes it normally.
//
// Replies: aMule encrypts its reply to us either with try-0 using OUR KadID (if
// it learned it) or with try-2 using the verify key we declared as
// `senderVerifyKey`. `deobfuscate` tries both and accepts whichever yields the
// magic.

/// Our constant UDP verify key. Non-zero + consistent across all packets so
/// every peer that decrypts one of our packets stores it and thereafter admits
/// our publish/search. Not security-sensitive for this probe.
pub const OUR_UDP_KEY: u32 = 0x0A0B0C0D;

/// aMule `MAGICVALUE_UDP_SYNC_CLIENT` (the obfuscated-packet marker).
pub const KAD_MAGIC: u32 = 0x395F2EC1;

/// aMule `CUInt128::StoreCryptValue` (src/kademlia/utils/UInt128.cpp):
/// writes the 128-bit ID as four uint32 words `m_data.u32_data[3..0]`, where
/// word 3 is the MOST significant, each word stored little-endian. For wire
/// bytes `B=[b0..b15]` (b0 most significant), aMule's internal words are
/// u32_data[3]=BE(b0..b3), [2]=BE(b4..b7), [1]=BE(b8..b11), [0]=BE(b12..b15),
/// and StoreCryptValue emits each word little-endian, so the result is
/// `[b3,b2,b1,b0, b7,b6,b5,b4, b11,b10,b9,b8, b15,b14,b13,b12]`.
/// This is the value aMule feeds into the obfuscation MD5 key, so it MUST match
/// byte-for-byte or the peer cannot decrypt our datagram.
fn store_crypt_value(kad_id_be: &[u8; 16]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for w in 0..4 {
        let word = u32::from_be_bytes([
            kad_id_be[4 * w],
            kad_id_be[4 * w + 1],
            kad_id_be[4 * w + 2],
            kad_id_be[4 * w + 3],
        ]);
        out[4 * w..4 * w + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// Build an obfuscated KAD datagram. `target_kad_id` is the RECEIVER's 16-byte
/// KAD ID (known from BOOTSTRAP_RES / KADEMLIA2_RES contacts); `inner` is the
/// plaintext `[0xE4][opcode][payload]`.
pub fn obfuscate(target_kad_id: &[u8; 16], our_key: u32, inner: &[u8]) -> Vec<u8> {
    // aMule uses a random 16-bit part; deterministic here for reproducibility.
    let random_key_part: u16 = 0x1234;
    let mut buf = vec![0u8; 1 + 2 + 4 + 1 + 4 + 4 + inner.len()];
    // byte 0: kad marker, low 2 bits cleared, never a protocol byte.
    buf[0] = 0x40;
    buf[1..3].copy_from_slice(&random_key_part.to_le_bytes());
    // RC4 key = MD5(StoreCryptValue(target_kad_id) [16] || randomKeyPart [2])
    let mut keydata = [0u8; 18];
    keydata[..16].copy_from_slice(&store_crypt_value(target_kad_id));
    keydata[16..18].copy_from_slice(&random_key_part.to_le_bytes());
    let key = md5(&keydata);
    let mut rc4 = Rc4::new(&key);
    // magic value (ENDIAN_SWAP_32(MAGIC) on x86 => big-endian bytes)
    let mut magic = KAD_MAGIC.to_be_bytes();
    rc4.crypt(&mut magic);
    buf[3..7].copy_from_slice(&magic);
    // padLen = 0
    let mut pad = [0u8; 1];
    rc4.crypt(&mut pad);
    buf[7] = pad[0];
    // receiverVerifyKey = 0 (we do not yet know the peer's verify key); the
    // packet still decrypts on the peer via the KadID-based (try-0) RC4 key.
    // senderVerifyKey carries OUR_UDP_KEY so the peer locks its reply to it and
    // we can decrypt the response.
    let kb = our_key.to_be_bytes();
    let mut rvk = [0u8; 4];
    rc4.crypt(&mut rvk);
    let mut svk = kb;
    rc4.crypt(&mut svk);
    buf[8..12].copy_from_slice(&rvk);
    buf[12..16].copy_from_slice(&svk);
    // payload
    let mut payload = inner.to_vec();
    rc4.crypt(&mut payload);
    buf[16..].copy_from_slice(&payload);
    buf
}

/// Reverse `obfuscate` for inbound packets. aMule may have encrypted the reply
/// to us with try-0 (our own KadID, if it learned it) or try-2 (our UDP verify
/// key, which it stored from the `senderVerifyKey` we declared). We try both
/// candidate RC4 bases and accept the first that decrypts to the obfuscation
/// magic. On success returns `(inner_frame, peer_sender_verify_key)`.
pub fn deobfuscate(data: &[u8], our_key: u32, our_node_id: &KadId) -> Option<(Vec<u8>, u32)> {
    if data.len() < 16 {
        return None;
    }
    let random_key_part = u16::from_le_bytes([data[1], data[2]]);
    // Candidate obfuscation bases, in priority order:
    //   1. try-0: our own KadID (StoreCryptValue layout) — peer learned it.
    //   2. try-2: our UDP verify key — peer locked the reply to it.
    let bases: [Vec<u8>; 2] = [
        store_crypt_value(our_node_id).to_vec(),
        // try-2: aMule derives the key with PokeUInt32(verifyKey) -> big-endian
        // bytes, so we must mirror that here.
        our_key.to_be_bytes().to_vec(),
    ];
    for base in &bases {
        let mut keydata = Vec::with_capacity(base.len() + 2);
        keydata.extend_from_slice(base);
        keydata.extend_from_slice(&random_key_part.to_le_bytes());
        let key = md5(&keydata);
        let mut rc4 = Rc4::new(&key);
        let mut buf = data.to_vec();
        rc4.crypt(&mut buf[3..]);
        let v = u32::from_be_bytes([buf[3], buf[4], buf[5], buf[6]]);
        if v != KAD_MAGIC {
            continue;
        }
        let pad_len = buf[7] as usize;
        let start = 16 + pad_len;
        if buf.len() < start {
            continue;
        }
        // senderVerifyKey sits at offset 12..16 of the decrypted crypt header.
        let svk_off = 12 + pad_len;
        let sender_verify_key = if buf.len() >= svk_off + 4 {
            u32::from_be_bytes([buf[svk_off], buf[svk_off + 1], buf[svk_off + 2], buf[svk_off + 3]])
        } else {
            0
        };
        let inner = buf[start..].to_vec();
        return Some((inner, sender_verify_key));
    }
    None
}

/// Obfuscated KAD datagram using the RECEIVER's verify key (aMule `try 2`):
/// RC4 key = `MD5(receiverVerifyKey[4 LE] || randomKeyPart[2 LE])`. Used for
/// publish/search once we have learned the peer's verify key from a prior
/// response's `senderVerifyKey` field, so `validReceiverKey` validates true.
pub fn obfuscate_verify(receiver_verify_key: u32, our_sender_key: u32, inner: &[u8]) -> Vec<u8> {
    let random_key_part: u16 = 0x1234;
    let mut buf = vec![0u8; 1 + 2 + 4 + 1 + 4 + 4 + inner.len()];
    buf[0] = 0x42; // KAD, try-2 (receiver-key) marker: bit0=0, bit1=1
    buf[1..3].copy_from_slice(&random_key_part.to_le_bytes());
    let mut keydata = [0u8; 6];
    // aMule derives the try-2 key with PokeUInt32(verifyKey) -> big-endian bytes.
    keydata[..4].copy_from_slice(&receiver_verify_key.to_be_bytes());
    keydata[4..6].copy_from_slice(&random_key_part.to_le_bytes());
    let key = md5(&keydata);
    let mut rc4 = Rc4::new(&key);
    let mut magic = KAD_MAGIC.to_be_bytes();
    rc4.crypt(&mut magic);
    buf[3..7].copy_from_slice(&magic);
    let mut pad = [0u8; 1];
    rc4.crypt(&mut pad);
    buf[7] = pad[0];
    let mut rvk = receiver_verify_key.to_be_bytes();
    rc4.crypt(&mut rvk);
    buf[8..12].copy_from_slice(&rvk);
    let mut svk = our_sender_key.to_be_bytes();
    rc4.crypt(&mut svk);
    buf[12..16].copy_from_slice(&svk);
    let mut payload = inner.to_vec();
    rc4.crypt(&mut payload);
    buf[16..].copy_from_slice(&payload);
    buf
}

// --- RC4 (aMule uses the standard PRGA, no keystream drop) ----------------
pub struct Rc4 {
    i: u8,
    j: u8,
    s: [u8; 256],
}

impl Rc4 {
    pub fn new(key: &[u8]) -> Self {
        let mut s = [0u8; 256];
        for (n, b) in s.iter_mut().enumerate() {
            *b = n as u8;
        }
        let mut j = 0u16;
        for n in 0..256 {
            j = (j + s[n] as u16 + key[n % key.len()] as u16) & 0xFF;
            s.swap(n, j as usize);
        }
        Rc4 { i: 0, j: 0, s }
    }
    pub fn crypt(&mut self, data: &mut [u8]) {
        for x in data.iter_mut() {
            self.i = self.i.wrapping_add(1);
            self.j = self.j.wrapping_add(self.s[self.i as usize]);
            self.s.swap(self.i as usize, self.j as usize);
            let k = self.s
                [(self.s[self.i as usize] as usize + self.s[self.j as usize] as usize) & 0xFF];
            *x ^= k;
        }
    }
}

// --- MD5 (RFC 1321) -------------------------------------------------------
pub fn md5(message: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
        14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
        21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let mut k = [0u32; 64];
    for (i, slot) in k.iter_mut().enumerate() {
        *slot = (f64::from(i as u32 + 1).sin().abs() * 4294967296.0).floor() as u32;
    }
    let mut h = [0x67452301u32, 0xEFCDAB89, 0x98BADCFE, 0x10325476];
    let mut msg = message.to_vec();
    let bit_len = (message.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 16];
        for (i, word) in w.iter_mut().enumerate() {
            *word = u32::from_le_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        for i in 0..64 {
            let f = match i {
                0..=15 => (b & c) | ((!b) & d),
                16..=31 => (d & b) | ((!d) & c),
                32..=47 => b ^ c ^ d,
                _ => c ^ (b | !d),
            };
            let g = if i < 16 {
                i
            } else if i < 32 {
                (5 * i + 1) % 16
            } else if i < 48 {
                (3 * i + 5) % 16
            } else {
                (7 * i) % 16
            };
            let f = f
                .wrapping_add(a)
                .wrapping_add(k[i])
                .wrapping_add(w[g]);
            let f = f.rotate_left(S[i]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

// --- MD4 (RFC 1320) -------------------------------------------------------
// aMule uses CryptoPP::Weak::MD4 in two places ADR-011's compatibility track
// needs: `KadGetKeywordHash()` (keyword dictionary Key) and the eD2k file hash
// (MD4 over 9.28 MB part hashes). The probe stays dependency-free, so MD4 is
// implemented here with known-answer vectors below.
pub fn md4(message: &[u8]) -> [u8; 16] {
    const S1: [u32; 4] = [3, 7, 11, 19];
    const S2: [u32; 4] = [3, 5, 9, 13];
    const S3: [u32; 4] = [3, 9, 11, 15];
    const K2: u32 = 0x5A82_7999;
    const K3: u32 = 0x6ED9_EBA1;
    const R2: [usize; 16] = [0, 4, 8, 12, 1, 5, 9, 13, 2, 6, 10, 14, 3, 7, 11, 15];
    const R3: [usize; 16] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];

    let mut msg = message.to_vec();
    let bit_len = (message.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    let mut h = [0x6745_2301u32, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476];
    for chunk in msg.chunks(64) {
        let mut x = [0u32; 16];
        for (i, w) in x.iter_mut().enumerate() {
            *w = u32::from_le_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        // Each step updates `a`, then rotates (a,b,c,d) <- (d,a,b,c).
        // 16 steps x 3 rounds = 48 rotations, so the registers are back in
        // their original alignment when the results are folded in.
        for i in 0..16 {
            let f = (b & c) | ((!b) & d);
            a = a.wrapping_add(f).wrapping_add(x[i]).rotate_left(S1[i % 4]);
            let t = a;
            a = d;
            d = c;
            c = b;
            b = t;
        }
        for i in 0..16 {
            let g = (b & c) | (b & d) | (c & d);
            a = a
                .wrapping_add(g)
                .wrapping_add(x[R2[i]])
                .wrapping_add(K2)
                .rotate_left(S2[i % 4]);
            let t = a;
            a = d;
            d = c;
            c = b;
            b = t;
        }
        for i in 0..16 {
            let f = b ^ c ^ d;
            a = a
                .wrapping_add(f)
                .wrapping_add(x[R3[i]])
                .wrapping_add(K3)
                .rotate_left(S3[i % 4]);
            let t = a;
            a = d;
            d = c;
            c = b;
            b = t;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// `KadGetKeywordHash` (src/kademlia/kademlia/Kademlia.cpp):
/// `MD4(unicode2UTF8(term))` -> `CUInt128::SetValueBE`. `ToByteArray` is the
/// inverse of `SetValueBE`, so the 16 bytes on the wire ARE the MD4 digest.
/// No lowercasing / trimming / Unicode normalisation happens in this function.
pub fn kad_keyword_hash(term: &str) -> KadId {
    md4(term.as_bytes())
}

/// eMule part size = 9500 KiB = 9_728_000 bytes (ADR-004: "MD4 / 9.28 MB").
pub const ED2K_PART_SIZE: usize = 9_500 * 1024;

/// eD2k file hash (`fileID`): MD4 of the file when it is SMALLER than one part,
/// otherwise MD4 of the concatenation of every part's MD4. AICH is out of scope.
/// This value is what an old client uses as `fileID` in the keyword dictionary;
/// faking it produces files that can be found but never downloaded.
///
/// ## Exact-multiple rule (v2.4.31 — L2 corrected an L1 wrong assumption)
///
/// Two mutually incompatible ED2K hashes exist for files whose size is an exact
/// multiple of `PARTSIZE`: the "Red" variant appends the MD4 of an empty part,
/// the "Blue" variant does not. crates.io `ed2k::Ed2k` defaults to **Blue** and
/// calls it "official"; aMule (the interoperability target of ADR-011) uses
/// **Red**. Verified three ways at aMule `6a110563`:
///
///   * `src/KnownFile.cpp` `CKnownFile::SetFileSize` documents test vectors:
///     a 1*PARTSIZE file has 2 part hashes, the last being
///     `31D6CFE0D16AE931B73C59D7E0C089C0` (MD4 of empty input), and file hash
///     `A72CA8DF7F07154E217C236C89C17619`.
///   * `src/ThreadTasks.cpp` `CHashingTask::CreateNextPartHash` explicitly
///     pushes `g_emptyMD4Hash` when `partLength == PARTSIZE && file.Eof()`.
///   * `src/KnownFile.cpp` `CreateHashFromHashlist` MD4s the whole hash list.
///
/// `m_iED2KPartHashCount = size / PARTSIZE; if != 0 { +1 }` — so a file of
/// exactly N parts has N+1 part hashes. Using Blue here would give a *different*
/// `fileID` from every real eMule/aMule client for those files: the entry would
/// be searchable but permanently undownloadable. L1's 63-byte probe file never
/// crossed the boundary, so the bug stayed invisible until L2 source publishing.
pub fn ed2k_hash(data: &[u8]) -> KadId {
    if data.len() < ED2K_PART_SIZE {
        return md4(data);
    }
    let mut part_hashes = Vec::with_capacity((data.len() / ED2K_PART_SIZE + 1) * 16);
    for chunk in data.chunks(ED2K_PART_SIZE) {
        part_hashes.extend_from_slice(&md4(chunk));
    }
    if data.len() % ED2K_PART_SIZE == 0 {
        // aMule's "special part hash" for a file ending on a part boundary.
        part_hashes.extend_from_slice(&md4(&[]));
    }
    md4(&part_hashes)
}

/// Full eMule part-hash list, for `OP_HASHSETANSWER` / `0x44` file-size tags.
/// `parts.len()` equals `m_iED2KPartHashCount`: 0 for files < PARTSIZE,
/// otherwise `ceil(size / PARTSIZE) + 1` when size is a multiple, else the
/// number of non-empty parts.
pub fn ed2k_part_hashes(data: &[u8]) -> Vec<[u8; 16]> {
    if data.len() < ED2K_PART_SIZE {
        return Vec::new();
    }
    let mut parts: Vec<[u8; 16]> = data.chunks(ED2K_PART_SIZE).map(md4).collect();
    if data.len() % ED2K_PART_SIZE == 0 {
        parts.push(md4(&[]));
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md4_known_vectors() {
        assert_eq!(hex(&md4(b"")), "31d6cfe0d16ae931b73c59d7e0c089c0");
        assert_eq!(hex(&md4(b"a")), "bde52cb31de33e46245e05fbdbd6fb24");
        assert_eq!(hex(&md4(b"abc")), "a448017aaf21d8525fc10ae87aa6729d");
        assert_eq!(hex(&md4(b"message digest")), "d9130a8164549fe818874806e1c7014b");
        assert_eq!(hex(&md4(b"abcdefghijklmnopqrstuvwxyz")), "d79e1c308aa5bbcdeea8ed63df412da9");
    }

    #[test]
    fn keyword_hash_is_unnamespaced_md4() {
        // Source-verified in ADR-011: eMule's keyword Key is MD4(UTF-8(term)),
        // NOT SHA-1 and NOT the ColdRevive namespaced Key.
        assert_eq!(kad_keyword_hash("abc"), md4(b"abc"));
        assert_ne!(kad_keyword_hash("abc"), namespaced_key("abc"));
        assert_ne!(kad_keyword_hash("abc"), plain_key("abc"));
        // The C++ function does no case folding.
        assert_ne!(kad_keyword_hash("ABC"), kad_keyword_hash("abc"));
        // A non-ASCII term is hashed as raw UTF-8 bytes.
        assert_eq!(kad_keyword_hash("冷"), md4("冷".as_bytes()));
    }

    #[test]
    fn ed2k_hash_single_and_multi_part() {
        assert_eq!(ed2k_hash(b"abc"), md4(b"abc"));
        let mut data = vec![0x5Au8; ED2K_PART_SIZE];
        data.extend_from_slice(b"tail");
        let mut expected_input = Vec::new();
        expected_input.extend_from_slice(&md4(&data[..ED2K_PART_SIZE]));
        expected_input.extend_from_slice(&md4(&data[ED2K_PART_SIZE..]));
        assert_eq!(ed2k_hash(&data), md4(&expected_input));
        assert_ne!(ed2k_hash(&data), md4(&data));
        assert_eq!(ed2k_part_hashes(&data).len(), 2);

        // v2.4.31: aMule/eMule append the MD4 of an EMPTY part whenever the
        // file size is an exact multiple of PARTSIZE. L1 had asserted the
        // opposite ("Blue"), which would produce a fileID no old client can
        // ever download. See the doc comment on ed2k_hash for the source trail.
        let exact = vec![0x33u8; ED2K_PART_SIZE * 2];
        let mut three_parts = Vec::new();
        three_parts.extend_from_slice(&md4(&exact[..ED2K_PART_SIZE]));
        three_parts.extend_from_slice(&md4(&exact[ED2K_PART_SIZE..]));
        three_parts.extend_from_slice(&md4(&[]));
        assert_eq!(ed2k_hash(&exact), md4(&three_parts));
        assert_eq!(ed2k_part_hashes(&exact).len(), 3);
        assert_ne!(ed2k_hash(&exact), md4(&three_parts[..32])); // Blue != Red
    }

    /// aMule `src/KnownFile.cpp` `CKnownFile::SetFileSize` documents a real
    /// 1*PARTSIZE file: part hash `4891ED2E...`, special empty part hash
    /// `31D6CFE0...`, resulting file hash `A72CA8DF7F07154E217C236C89C17619`.
    /// Recomputing it from those two hashes independently confirms both our MD4
    /// and the Red rule against the upstream implementation's own vector.
    #[test]
    fn ed2k_hash_matches_amule_documented_one_part_file() {
        let part = [
            0x48, 0x91, 0xED, 0x2E, 0x5C, 0x9C, 0x49, 0xF4,
            0x42, 0x14, 0x5A, 0x3A, 0x5F, 0x60, 0x82, 0x99,
        ];
        let empty = md4(&[]);
        assert_eq!(hex(&empty), "31d6cfe0d16ae931b73c59d7e0c089c0");
        let mut concat = Vec::from(part);
        concat.extend_from_slice(&empty);
        assert_eq!(hex(&md4(&concat)), "a72ca8df7f07154e217c236c89c17619");
        assert_ne!(hex(&md4(&part)), "a72ca8df7f07154e217c236c89c17619");
    }

    #[test]
    fn publish_source_req_matches_amule_layout() {
        let file = kad_keyword_hash("file");
        let src = node_id("l2-source");
        let tags = source_tags(4662, 4672, 1234);
        let f = publish_source_req(&file, &src, &tags);
        let (opcode, payload) = parse_frame(&f).unwrap();
        assert_eq!(opcode, op::PUBLISH_SOURCE_REQ);
        assert_eq!(&payload[..16], &file);
        assert_eq!(&payload[16..32], &src);
        assert_eq!(payload[32], tags.len() as u8); // ONE byte, not u16
        let parsed = parse_publish_source_req(payload).unwrap();
        assert_eq!(parsed.file_id, file);
        assert_eq!(parsed.source_id, src);
        assert_eq!(parsed.tags, tags);
        // SOURCETYPE is mandatory or the receiver drops the entry entirely.
        assert!(parsed.tags.iter().any(|t| t.name == [TAG_SOURCETYPE]));
        // Tags must use the Kad ReadTag layout (u16 name length), never 0x80.
        assert_eq!(payload[34], 0x01); // name length low byte of first tag
    }

    #[test]
    fn search_source_req_layout() {
        let file = ed2k_hash(b"abc");
        let f = search_source_req(&file, 9_728_001);
        let (opcode, payload) = parse_frame(&f).unwrap();
        assert_eq!(opcode, op::SEARCH_SOURCE_REQ);
        assert_eq!(payload.len(), 16 + 2 + 8);
        let parsed = parse_search_source_req(payload).unwrap();
        assert_eq!(parsed.file_id, file);
        assert_eq!(parsed.start_position, 0);
        assert_eq!(parsed.file_size, 9_728_001);
    }

    #[test]
    fn frame_roundtrip() {
        let f = frame(op::BOOTSTRAP_REQ, &[]);
        assert_eq!(f, vec![0xE4, 0x01]);
        let (op, payload) = parse_frame(&f).unwrap();
        assert_eq!(op, op::BOOTSTRAP_REQ);
        assert!(payload.is_empty());
        assert!(parse_frame(&[0xE5, 0x01]).is_err());
        assert!(parse_frame(&[0xC5, 0x01]).is_err());
    }

    #[test]
    fn contact_roundtrip_is_25_bytes() {
        let c = Contact {
            id: node_id("A"),
            ip: [127, 0, 0, 1],
            udp: 4672,
            tcp: 4662,
            version: KAD_VERSION,
        };
        let mut b = Buf::new();
        c.encode(&mut b);
        assert_eq!(b.0.len(), 25);
        assert_eq!(Contact::decode(&mut Cur::new(&b.0)).unwrap(), c);
    }

    /// aMule writes KAD IPs in host-order LE, i.e. the reverse of dotted quad.
    /// Documentation address 203.0.113.5 has wire bytes 05 71 00 cb.
    #[test]
    fn contact_ip_wire_order_is_reversed() {
        let c = Contact {
            id: node_id("A"),
            ip: [203, 0, 113, 5],
            udp: 24575,
            tcp: 24575,
            version: 9,
        };
        let mut b = Buf::new();
        c.encode(&mut b);
        assert_eq!(&b.0[16..20], &[5, 113, 0, 203]);
        assert_eq!(b.0[20..22], [0xFF, 0x5F]); // 24575 LE
        let mut wire = [0u8; 25];
        wire[16..20].copy_from_slice(&[5, 113, 0, 203]);
        let decoded = Contact::decode(&mut Cur::new(&wire)).unwrap();
        assert_eq!(decoded.ip, [203, 0, 113, 5]);
        assert_eq!(decoded.addr(), "203.0.113.5:0");
        assert_eq!(Contact::decode(&mut Cur::new(&b.0)).unwrap(), c);
    }

    #[test]
    fn tag_roundtrip_all_types() {
        let tags = vec![
            Tag::int2(TAG_SOURCEUPORT, 4672),
            Tag::int1(TAG_KADMISCOPTIONS, 0),
            Tag::int4(TAG_FILESIZE, 262_144),
            Tag::str1(TAG_FILENAME, "ColdRevive_V1:probe"),
            Tag { name: b"ColdRevive_V1:descriptor".to_vec(), value: TagValue::Str("node=A;port=4672".into()) },
        ];
        let mut b = Buf::new();
        write_tags(&mut b, &tags).unwrap();
        assert_eq!(read_tags(&mut Cur::new(&b.0)).unwrap(), tags);
    }

    #[test]
    fn tag_kad_layout_matches_amule_readtag() {
        // Byte-exact vector for aMule's `CFileDataIO::ReadTag` (Kad path):
        //   type(1) + name_len(2, LE) + name + [value]
        // A numeric name is a length-1 string (`TAG_FILENAME = wxT("\x01")`),
        // NOT the ed2k `type|0x80` short form. Getting this wrong makes every
        // PUBLISH throw inside aMule before it can send PUBLISH_RES.
        let mut b = Buf::new();
        Tag::str1(TAG_FILENAME, "hi").encode(&mut b).unwrap();
        assert_eq!(b.0, vec![0x02, 0x01, 0x00, 0x01, 0x02, 0x00, b'h', b'i']);

        let mut b = Buf::new();
        Tag::int4(TAG_FILESIZE, 1024).encode(&mut b).unwrap();
        assert_eq!(b.0, vec![0x03, 0x01, 0x00, 0x02, 0x00, 0x04, 0x00, 0x00]);

        // Strings are always TAGTYPE_STRING (0x02); the compact STRn range is
        // rejected by ReadTag's switch, so a 3-char value must not become 0x13.
        let mut b = Buf::new();
        Tag::str1(TAG_FILENAME, "abc").encode(&mut b).unwrap();
        assert_eq!(b.0[0], 0x02);
        assert_eq!(b.0.len(), 9);
        assert_eq!(&b.0[6..], b"abc");
    }

    #[test]
    fn tag_decode_rejects_types_amule_cannot_read() {
        // 0x1E = TAGTYPE_STR14: accepted by CTag::CTag but NOT by ReadTag, which
        // throws "Invalid Kad tag type 0x1e" and aborts the whole publish.
        let raw = [0x1E, 0x01, 0x00, 0x01, b'x'];
        assert!(Tag::decode(&mut Cur::new(&raw)).is_err());
    }

    #[test]
    fn all_message_roundtrips() {
        let a = Contact { id: node_id("A"), ip: [127, 0, 0, 1], udp: 14672, tcp: 14662, version: KAD_VERSION };
        let b = Contact { id: node_id("B"), ip: [127, 0, 0, 1], udp: 15672, tcp: 15662, version: KAD_VERSION };
        let key = namespaced_key("coldrevive:t0:sample");

        let packet = bootstrap_res(&b, &[a.clone(), b.clone()]);
        let (op, p) = parse_frame(&packet).unwrap();
        assert_eq!(op, op::BOOTSTRAP_RES);
        let r = parse_bootstrap_res(p).unwrap();
        assert_eq!(r.self_id, b.id);
        assert_eq!(r.version, KAD_VERSION);
        assert_eq!(r.contacts.len(), 2);

        let packet = hello(op::HELLO_REQ, &a, &[]);
        let (op, p) = parse_frame(&packet).unwrap();
        assert_eq!(op, op::HELLO_REQ);
        assert_eq!(parse_hello(p).unwrap().udp_from_tag, Some(14672));

        let packet = find_node_req(&key, &b.id, 2);
        let (op, p) = parse_frame(&packet).unwrap();
        assert_eq!(op, op::REQ);
        assert_eq!(parse_find_node_req(p).unwrap().target, key);

        let packet = find_node_res(&key, &[a.clone(), b.clone()]);
        let (op, p) = parse_frame(&packet).unwrap();
        assert_eq!(op, op::RES);
        assert_eq!(parse_find_node_res(p).unwrap().contacts.len(), 2);

        let entries = vec![IndexEntry {
            source_id: node_id("descriptor-1"),
            tags: vec![Tag::str1(TAG_FILENAME, "ColdRevive_V1:probe")],
        }];
        let packet = publish_key_req(&key, &entries);
        let (op, p) = parse_frame(&packet).unwrap();
        assert_eq!(op, op::PUBLISH_KEY_REQ);
        assert_eq!(parse_publish_key_req(p).unwrap().entries, entries);

        let packet = publish_res(&key, 0);
        let (op, p) = parse_frame(&packet).unwrap();
        assert_eq!(op, op::PUBLISH_RES);
        assert_eq!(parse_publish_res(p).unwrap().load, 0);

        let packet = search_key_req(&key);
        let (op, p) = parse_frame(&packet).unwrap();
        assert_eq!(op, op::SEARCH_KEY_REQ);
        assert_eq!(parse_search_key_req(p).unwrap(), key);

        let packet = search_res(&b.id, &key, &entries);
        let (op, p) = parse_frame(&packet).unwrap();
        assert_eq!(op, op::SEARCH_RES);
        assert_eq!(parse_search_res(p).unwrap().answers, entries);
    }

    #[test]
    fn namespace_changes_key_id() {
        let logical = "coldrevive:t0:sample";
        assert_ne!(namespaced_key(logical), plain_key(logical));
        assert_eq!(namespaced_key(logical).len(), 16);
    }

    #[test]
    fn tolerance_zone_and_lan_bypass() {
        let key = namespaced_key("x");
        // Exact match: XOR distance 0 -> always inside the tolerance zone.
        assert_eq!(top_chunk(&key, &key), 0);
        assert!(publish_allowed(&key, &key, false));

        // Flip the most significant byte: top chunk is guaranteed > 2^24.
        let mut far = key;
        far[0] ^= 0xFF;
        assert!(top_chunk(&far, &key) > SEARCHTOLERANCE);
        // aMule silently drops such a publish from a non-LAN peer...
        assert!(!publish_allowed(&far, &key, false));
        // ...but LAN sources bypass the tolerance check entirely.
        assert!(publish_allowed(&far, &key, true));
    }
}

#[cfg(test)]
mod crypto_tests {
    use super::*;

    #[test]
    fn md5_known_vectors() {
        assert_eq!(
            hex(&md5(b"")),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
        assert_eq!(
            hex(&md5(b"abc")),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            hex(&md5(b"The quick brown fox jumps over the lazy dog")),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
    }

    #[test]
    fn rc4_is_reciprocal() {
        let key = [0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04];
        let mut data = b"Kademlia UDP obfuscation test".to_vec();
        let orig = data.clone();
        let mut a = Rc4::new(&key);
        a.crypt(&mut data);
        assert_ne!(data, orig); // actually encrypted
        let mut b = Rc4::new(&key);
        b.crypt(&mut data);
        assert_eq!(data, orig); // decrypts back
    }

    #[test]
    fn store_crypt_value_zero_and_known() {
        assert_eq!(store_crypt_value(&[0u8; 16]), [0u8; 16]);
        // kad_id == 1 (big-endian, LSB bit set at byte 15): word3 = BE(b12..b15)
        // = 0x00000001, emitted little-endian at out[12..16] => [1,0,0,0]; the
        // high words stay zero. Matches aMule UInt128.cpp StoreCryptValue.
        let mut one = [0u8; 16];
        one[15] = 1;
        assert_eq!(
            store_crypt_value(&one),
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]
        );
    }

    #[test]
    fn obfuscate_roundtrip() {
        let target = node_id("some-kad-peer");
        let inner = frame(op::BOOTSTRAP_REQ, &[]);
        let env = obfuscate(&target, OUR_UDP_KEY, &inner);
        assert!(env.len() > inner.len());
        // byte 0 must not be a KAD protocol byte
        assert_ne!(env[0], PROTO_KAD);
        // The recipient decrypts with its own KadID (try-0). We emulate the
        // recipient by passing `target` as our_node_id: obfuscate used
        // StoreCryptValue(target), so deobfuscate's try-0 branch recovers it.
        let back = deobfuscate(&env, OUR_UDP_KEY, &target).expect("deobfuscate (try-0) must succeed");
        // deobfuscate returns (inner_frame, peer_sender_verify_key).
        assert_eq!(back.0, inner);
        // A genuinely wrong recipient id AND wrong verify key must fail.
        let wrong = node_id("not-the-recipient");
        assert!(deobfuscate(&env, 0x12345678, &wrong).is_none());
    }

    #[test]
    fn obfuscate_verify_roundtrip() {
        // Peer's receiver verify key we learned earlier; we are the sender.
        let rvk = 0x0A0B0C0D;
        let inner = frame(op::PUBLISH_KEY_REQ, &[]);
        let env = obfuscate_verify(rvk, OUR_UDP_KEY, &inner);
        assert_ne!(env[0], PROTO_KAD);
        // Reply-side decode: aMule encrypts its reply with try-2 using the
        // verify key it stored for us. Our own deobfuscate with try-2 (our_key)
        // recovers it; the try-0 branch (wrong id) is simply skipped.
        let back = deobfuscate(&env, rvk, &node_id("whatever"))
            .expect("deobfuscate (try-2) must succeed");
        assert_eq!(back.0, inner);
    }
}

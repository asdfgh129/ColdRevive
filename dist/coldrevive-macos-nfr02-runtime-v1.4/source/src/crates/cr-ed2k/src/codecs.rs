//! Honest codecs at the edge of the L2 eD2k stack.
//!
//! The only eD2k wire formats implemented here are those actually pinned in
//! `docs/04-protocol/PROTO.md` §2.2.1 (the KAD source dictionary: `0x44`,
//! `0x34`, `0x4B`, `0x3B`). Server login, credit / secure identification and
//! AICH are explicitly listed as out of scope in that document and have **no
//! verified layout in this repo**. For those we return
//! [`CodecError::Unsupported`] instead of producing bytes that look like
//! success while a real aMule would silently drop them (see PROTO.md §2.0:
//! silent-drop paths are indistinguishable from "no such entry").
//!
//! This module is deliberately std-only and allocation-bounded: every decoder
//! rejects trailing garbage and caps list lengths.

use std::fmt;

use crate::kad::{self, Buf, Cur, Tag, TagValue};
use crate::wire::Hash16;

// ------------------------------------------------------------------ error --

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// The feature has no verified wire format in this repo. No bytes were
    /// invented and no success was claimed.
    Unsupported { feature: &'static str, reason: &'static str },
    /// A documented format was present but the payload was not valid.
    Malformed(String),
}

impl CodecError {
    pub fn is_unsupported(&self) -> bool {
        matches!(self, CodecError::Unsupported { .. })
    }
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::Unsupported { feature, reason } => {
                write!(f, "unsupported eD2k feature {feature}: {reason}")
            }
            CodecError::Malformed(msg) => write!(f, "malformed eD2k payload: {msg}"),
        }
    }
}

impl std::error::Error for CodecError {}

pub type CodecResult<T> = Result<T, CodecError>;

fn unsupported(feature: &'static str, reason: &'static str) -> CodecError {
    CodecError::Unsupported { feature, reason }
}

fn malformed(what: &str) -> CodecError {
    CodecError::Malformed(what.to_string())
}

// ------------------------------------------------- KAD source dictionary --

/// Wire formats from PROTO.md §2.2.1. All tags use the KAD `ReadTag` layout
/// (`<type u8><u16 name_len><name><value>`), **not** the eD2k compact form.
pub mod source_exchange {
    use super::*;

    pub const PUBLISH_SOURCE_REQ: u8 = 0x44;
    pub const SEARCH_SOURCE_REQ: u8 = 0x34;
    pub const PUBLISH_RES: u8 = 0x4B;
    pub const SEARCH_RES: u8 = 0x3B;

    /// PROTO.md §2.2.1: without this tag aMule never sets `m_bSource` and
    /// deletes the entry, so decoding "success" without it would be a lie.
    pub const TAG_SOURCETYPE: u8 = 0xFF;
    pub const TAG_SOURCEPORT: u8 = 0xFD;
    pub const TAG_SOURCEUPORT: u8 = 0xFC;
    /// PROTO.md §2: result processing caps a query at 200 entries.
    pub const MAX_SEARCH_ANSWERS: usize = 200;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PublishSourceRequest {
        pub file_id: Hash16,
        pub source_id: Hash16,
        /// KAD-format tags (u8 count prefix on the wire).
        pub tags: Vec<Tag>,
    }

    fn numeric_tag(tags: &[Tag], name: u8) -> Option<u64> {
        let t = tags.iter().find(|t| t.name.len() == 1 && t.name[0] == name)?;
        match t.value {
            TagValue::U8(v) => Some(v as u64),
            TagValue::U16(v) => Some(v as u64),
            TagValue::U32(v) => Some(v as u64),
            TagValue::U64(v) => Some(v),
            _ => None,
        }
    }

    impl PublishSourceRequest {
        /// Mirrors `CIndexed::AddSources` preconditions documented in
        /// PROTO.md §2.2.1: non-empty tag list, `TAG_SOURCETYPE=1`, and both
        /// TCP/UDP ports non-zero. A `false` here means aMule would silently
        /// reject the publish; the codec itself still decodes it honestly.
        pub fn is_indexable(&self) -> bool {
            !self.tags.is_empty()
                && numeric_tag(&self.tags, TAG_SOURCETYPE) == Some(1)
                && numeric_tag(&self.tags, TAG_SOURCEPORT).unwrap_or(0) != 0
                && numeric_tag(&self.tags, TAG_SOURCEUPORT).unwrap_or(0) != 0
        }
    }

    /// `<fileID 16><sourceID 16><tagcount u8><Kad ReadTag tags...>`.
    /// Note there is deliberately no u16 entry count: one packet = one source.
    pub fn encode_publish_source(req: &PublishSourceRequest) -> Vec<u8> {
        let mut b = Buf::new();
        b.id(&req.file_id);
        b.id(&req.source_id);
        // `write_tags` emits the u8 tag count required by §2.2.1.
        let _ = kad::write_tags(&mut b, &req.tags);
        b.0
    }

    pub fn decode_publish_source(payload: &[u8]) -> CodecResult<PublishSourceRequest> {
        let mut c = Cur::new(payload);
        let file_id = c.id().map_err(|e| malformed(&e))?;
        let source_id = c.id().map_err(|e| malformed(&e))?;
        let tags = kad::read_tags(&mut c).map_err(|e| malformed(&e))?;
        if tags.is_empty() {
            return Err(malformed("PUBLISH_SOURCE_REQ has no tags (aMule drops it)"));
        }
        if !tags.iter().any(|t| t.name.len() == 1 && t.name[0] == TAG_SOURCETYPE) {
            return Err(malformed("PUBLISH_SOURCE_REQ lacks TAG_SOURCETYPE 0xFF"));
        }
        if c.remaining() != 0 {
            return Err(malformed("PUBLISH_SOURCE_REQ has trailing bytes"));
        }
        Ok(PublishSourceRequest { file_id, source_id, tags })
    }

    /// `<fileID 16><startPosition u16><fileSize u64>`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct SearchSourceRequest {
        pub file_id: Hash16,
        /// Raw wire value. aMule treats high bits as a paging marker.
        pub raw_start_position: u16,
        pub file_size: u64,
    }

    impl SearchSourceRequest {
        /// PROTO.md §2.2.1: `startPosition & 0x7FFF`.
        pub fn start_position(&self) -> u16 {
            self.raw_start_position & 0x7FFF
        }
    }

    pub fn encode_search_source(req: &SearchSourceRequest) -> Vec<u8> {
        let mut b = Buf::new();
        b.id(&req.file_id);
        b.u16(req.raw_start_position);
        b.u64(req.file_size);
        b.0
    }

    pub fn decode_search_source(payload: &[u8]) -> CodecResult<SearchSourceRequest> {
        if payload.len() < 26 {
            return Err(malformed("SEARCH_SOURCE_REQ shorter than 26 bytes"));
        }
        let mut c = Cur::new(payload);
        let file_id = c.id().map_err(|e| malformed(&e))?;
        let raw_start_position = c.u16().map_err(|e| malformed(&e))?;
        let file_size = c.u64().map_err(|e| malformed(&e))?;
        Ok(SearchSourceRequest { file_id, raw_start_position, file_size })
    }

    /// `<fileID 16><load u8>`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PublishResult {
        pub file_id: Hash16,
        pub load: u8,
    }

    pub fn encode_publish_result(r: &PublishResult) -> Vec<u8> {
        let mut b = Buf::new();
        b.id(&r.file_id);
        b.u8(r.load);
        b.0
    }

    pub fn decode_publish_result(payload: &[u8]) -> CodecResult<PublishResult> {
        if payload.len() != 17 {
            return Err(malformed("PUBLISH_RES must be exactly 17 bytes"));
        }
        let mut c = Cur::new(payload);
        let file_id = c.id().map_err(|e| malformed(&e))?;
        let load = c.u8().map_err(|e| malformed(&e))?;
        Ok(PublishResult { file_id, load })
    }

    /// One `<answerID 16><tagcount u8><tags>` entry of a `SEARCH_RES`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct SearchAnswer {
        pub answer_id: Hash16,
        pub tags: Vec<Tag>,
    }

    /// `<senderID 16><target 16><count u16>[<answer 16><tagcount u8><tags>]*`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct SearchSourceResult {
        pub sender_id: Hash16,
        pub target: Hash16,
        pub answers: Vec<SearchAnswer>,
    }

    pub fn encode_search_result(r: &SearchSourceResult) -> CodecResult<Vec<u8>> {
        if r.answers.len() > MAX_SEARCH_ANSWERS {
            return Err(malformed("SEARCH_RES answer count exceeds 200"));
        }
        let mut b = Buf::new();
        b.id(&r.sender_id);
        b.id(&r.target);
        b.u16(r.answers.len() as u16);
        for a in &r.answers {
            b.id(&a.answer_id);
            let _ = kad::write_tags(&mut b, &a.tags);
        }
        Ok(b.0)
    }

    pub fn decode_search_result(payload: &[u8]) -> CodecResult<SearchSourceResult> {
        let mut c = Cur::new(payload);
        let sender_id = c.id().map_err(|e| malformed(&e))?;
        let target = c.id().map_err(|e| malformed(&e))?;
        let count = c.u16().map_err(|e| malformed(&e))? as usize;
        if count > MAX_SEARCH_ANSWERS {
            return Err(malformed("SEARCH_RES answer count exceeds 200"));
        }
        let mut answers = Vec::with_capacity(count);
        for _ in 0..count {
            let answer_id = c.id().map_err(|e| malformed(&e))?;
            let tags = kad::read_tags(&mut c).map_err(|e| malformed(&e))?;
            answers.push(SearchAnswer { answer_id, tags });
        }
        if c.remaining() != 0 {
            return Err(malformed("SEARCH_RES has trailing bytes"));
        }
        Ok(SearchSourceResult { sender_id, target, answers })
    }
}

// --------------------------------------------- features with no wire spec --

/// A named feature this crate deliberately does not pretend to implement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedFeature {
    pub name: &'static str,
    pub reference: &'static str,
    pub reason: &'static str,
}

pub const SERVER_LOGIN: UnsupportedFeature = UnsupportedFeature {
    name: "eD2k LoginServer TCP login",
    reference: "docs/04-protocol/PROTO.md §2.2 (server connection excluded from L2)",
    reason: "no verified packet layout in this repo; guessing one would be silent-drop bait",
};

pub const CREDIT: UnsupportedFeature = UnsupportedFeature {
    name: "eMule credit / secure identification",
    reference: "docs/04-protocol/PROTO.md §2.2.2 note; ADR-011 (credit is a non-goal)",
    reason: "PROTO.md lists credit as unimplemented and pins no tag/opcode layout",
};

pub const AICH: UnsupportedFeature = UnsupportedFeature {
    name: "AICH hash set / integrity exchange",
    reference: "docs/04-protocol/PROTO.md §2.2.2 note; ADR-004 (AICH not implemented)",
    reason: "no verified request/response opcode or payload layout in this repo",
};

pub fn all_unsupported_features() -> [UnsupportedFeature; 3] {
    [SERVER_LOGIN, CREDIT, AICH]
}

pub fn feature_status(f: &UnsupportedFeature) -> CodecError {
    unsupported(f.name, f.reason)
}

/// Local observation only. `None` means "unknown": no value is ever derived
/// from bytes we cannot parse, so this cannot be read as a login success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ServerLoginState {
    pub observed_high_id: Option<bool>,
}

pub fn decode_server_login(_payload: &[u8]) -> CodecResult<ServerLoginState> {
    Err(feature_status(&SERVER_LOGIN))
}

pub fn encode_server_login(_state: &ServerLoginState) -> CodecResult<Vec<u8>> {
    Err(feature_status(&SERVER_LOGIN))
}

/// Local upload/download bookkeeping only; NOT a wire-decoded credit blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CreditState {
    pub uploaded: u64,
    pub downloaded: u64,
}

pub fn decode_credit(_payload: &[u8]) -> CodecResult<CreditState> {
    Err(feature_status(&CREDIT))
}

pub fn encode_credit(_state: &CreditState) -> CodecResult<Vec<u8>> {
    Err(feature_status(&CREDIT))
}

/// Capability flag rather than a fake hash-set codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AichCapability {
    pub available: bool,
}

pub fn decode_aich(_payload: &[u8]) -> CodecResult<Vec<u8>> {
    Err(feature_status(&AICH))
}

// ------------------------------------------------------------------ tests --

#[cfg(test)]
mod tests {
    use super::*;
    use source_exchange::*;

    fn hash(v: u8) -> Hash16 {
        [v; 16]
    }

    #[test]
    fn publish_source_roundtrip_and_indexability() {
        let req = PublishSourceRequest {
            file_id: hash(1),
            source_id: hash(2),
            tags: vec![
                Tag::int1(TAG_SOURCETYPE, 1),
                Tag::int2(TAG_SOURCEPORT, 4662),
                Tag::int2(TAG_SOURCEUPORT, 4672),
            ],
        };
        let wire = encode_publish_source(&req);
        let p = crate::wire::parse_frame(&crate::wire::frame(
            crate::wire::PROT_EDONKEY, PUBLISH_SOURCE_REQ, &wire,
        ))
        .unwrap();
        assert_eq!(p.opcode, PUBLISH_SOURCE_REQ);
        let back = decode_publish_source(&p.payload).unwrap();
        assert_eq!(back.file_id, hash(1));
        assert_eq!(back.source_id, hash(2));
        assert_eq!(back.tags, req.tags);
        assert!(back.is_indexable());
    }

    #[test]
    fn publish_source_rejects_missing_sourcetype_and_trailing_bytes() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&hash(1));
        raw.extend_from_slice(&hash(2));
        // one tag, but not TAG_SOURCETYPE
        let mut b = Buf::new();
        let _ = kad::write_tags(&mut b, &[Tag::int2(TAG_SOURCEPORT, 4662)]);
        raw.extend_from_slice(&b.0);
        assert!(decode_publish_source(&raw).is_err());

        let good = PublishSourceRequest {
            file_id: hash(1),
            source_id: hash(2),
            tags: vec![Tag::int1(TAG_SOURCETYPE, 1)],
        };
        let mut wire = encode_publish_source(&good);
        wire.push(0xEE); // trailing garbage
        assert!(decode_publish_source(&wire).is_err());
    }

    #[test]
    fn search_source_applies_high_bit_mask() {
        let req = SearchSourceRequest {
            file_id: hash(3),
            raw_start_position: 0x9000,
            file_size: 9_736_192,
        };
        let wire = encode_search_source(&req);
        assert_eq!(wire.len(), 26);
        let back = decode_search_source(&wire).unwrap();
        assert_eq!(back.raw_start_position, 0x9000);
        assert_eq!(back.start_position(), 0x1000);
        assert_eq!(back.file_size, 9_736_192);
        assert!(decode_search_source(&wire[..25]).is_err());
    }

    #[test]
    fn publish_result_roundtrip() {
        let r = PublishResult { file_id: hash(4), load: 7 };
        let wire = encode_publish_result(&r);
        assert_eq!(wire.len(), 17);
        assert_eq!(decode_publish_result(&wire).unwrap(), r);
        assert!(decode_publish_result(&wire[..16]).is_err());
    }

    #[test]
    fn search_result_roundtrip_and_count_cap() {
        let r = SearchSourceResult {
            sender_id: hash(5),
            target: hash(6),
            answers: vec![
                SearchAnswer { answer_id: hash(7), tags: vec![Tag::int1(TAG_SOURCETYPE, 1)] },
                SearchAnswer { answer_id: hash(8), tags: vec![] },
            ],
        };
        let wire = encode_search_result(&r).unwrap();
        assert_eq!(decode_search_result(&wire).unwrap(), r);

        let huge = SearchSourceResult {
            sender_id: hash(5),
            target: hash(6),
            answers: vec![SearchAnswer { answer_id: hash(7), tags: vec![] }; MAX_SEARCH_ANSWERS + 1],
        };
        assert!(encode_search_result(&huge).is_err());
    }

    #[test]
    fn unsupported_features_never_claim_success() {
        for f in all_unsupported_features() {
            let e = feature_status(&f);
            assert!(e.is_unsupported(), "{f:?} must be explicit Unsupported");
            assert!(format!("{e}").contains(f.name));
        }
        assert!(decode_server_login(&[0u8; 32]).is_err());
        assert!(encode_server_login(&ServerLoginState::default()).is_err());
        assert!(decode_credit(&[0u8; 32]).is_err());
        assert!(encode_credit(&CreditState::default()).is_err());
        assert!(decode_aich(&[0u8; 32]).is_err());
        // A default-constructed placeholder is not a wire-derived success.
        assert_eq!(ServerLoginState::default().observed_high_id, None);
        assert!(!AichCapability::default().available);
    }
}

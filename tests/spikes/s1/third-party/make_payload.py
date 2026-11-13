#!/usr/bin/env python3
"""Generate a deterministic payload file + a standards-compliant .torrent.

Used by the S1 third-party-BT-client interop round: a foreign client (Motrix
Next's bundled aria2c) seeds `payload.bin` from `payload.torrent`; rqbit's
`cross-lee` leeches it via a direct peer connect and verifies every piece.
"""
import hashlib
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
PAYLOAD = os.path.join(HERE, "payload.bin")
TORRENT = os.path.join(HERE, "payload.torrent")
SIZE = 2 * 1024 * 1024  # 2 MiB
PIECE_LEN = 16384        # 16 KiB -> 128 pieces


def deterministic_bytes(n: int) -> bytes:
    # xorshift32 seeded stream -> reproducible content (no external deps)
    x = 0x9E3779B9
    out = bytearray()
    while len(out) < n:
        x ^= (x << 13) & 0xFFFFFFFF
        x ^= x >> 17
        x ^= (x << 5) & 0xFFFFFFFF
        out.append((x >> 24) & 0xFF)
    return bytes(out[:n])


def bencode_int(i: int) -> bytes:
    return b"i" + str(i).encode() + b"e"


def bencode_str(s: bytes) -> bytes:
    return str(len(s)).encode() + b":" + s


def bencode_dict(d: dict) -> bytes:
    parts = []
    for k in sorted(d.keys()):
        parts.append(bencode_str(k))
        v = d[k]
        if isinstance(v, int):
            parts.append(bencode_int(v))
        elif isinstance(v, bytes):
            parts.append(bencode_str(v))
        elif isinstance(v, dict):
            parts.append(bencode_dict(v))
        else:
            raise TypeError(v)
    return b"d" + b"".join(parts) + b"e"


def main() -> int:
    data = deterministic_bytes(SIZE)
    with open(PAYLOAD, "wb") as f:
        f.write(data)
    print(f"[make_payload] wrote {PAYLOAD} ({SIZE} bytes)")

    pieces = b"".join(
        hashlib.sha1(data[i : i + PIECE_LEN]).digest()
        for i in range(0, len(data), PIECE_LEN)
    )

    info = {
        b"length": SIZE,
        b"name": b"payload.bin",
        b"piece length": PIECE_LEN,
        b"pieces": pieces,
    }
    info_benc = bencode_dict(info)
    infohash = hashlib.sha1(info_benc).hexdigest()

    # NOTE: `info` must be a *nested dict* (bencode_dict recurses), not a
    # pre-bencoded bytestring — some clients (aria2) reject info-as-string.
    torrent = bencode_dict(
        {
            b"announce": b"http://127.0.0.1:1/announce",  # dummy; DHT/tracker disabled on both sides
            b"info": info,
        }
    )
    with open(TORRENT, "wb") as f:
        f.write(torrent)
    print(f"[make_payload] wrote {TORRENT}")
    print(f"[make_payload] infohash = {infohash}")
    print(f"[make_payload] pieces   = {len(pieces) // 20}")
    print(f"[make_payload] piece_len= {PIECE_LEN}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

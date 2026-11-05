#!/usr/bin/env python3
"""Give each app identity its own deterministic Mach-O UUID, before codesigning.

macOS local-network privacy also tracks the executable UUID (Apple TN3179).
Copying a release executable into a differently named QA bundle must not reuse it.
Only LC_UUID changes; no code, audio data, identifiers or access rules are altered.
"""
import argparse
import hashlib
from pathlib import Path
import struct
import uuid


def app_uuid(data: bytes, bundle_id: str) -> tuple[bytes, uuid.UUID]:
    if len(data) < 32 or struct.unpack_from("<I", data)[0] != 0xFEEDFACF:
        raise ValueError("Expected a thin little-endian 64-bit Mach-O executable")
    count, size = struct.unpack_from("<II", data, 16)
    end = 32 + size
    if count > 65536 or end > len(data):
        raise ValueError("Invalid Mach-O load-command table")
    position = 32
    uuid_offset = None
    payload_end = len(data)
    for _ in range(count):
        if position + 8 > end:
            raise ValueError("Truncated Mach-O load command")
        command, command_size = struct.unpack_from("<II", data, position)
        if command_size < 8 or command_size % 8 or position + command_size > end:
            raise ValueError("Invalid Mach-O load-command size")
        if command == 0x1B:  # LC_UUID
            if command_size != 24 or uuid_offset is not None:
                raise ValueError("Expected one LC_UUID command")
            uuid_offset = position + 8
        elif command == 0x1D:  # LC_CODE_SIGNATURE: exclude the mutable signing blob.
            if command_size != 16:
                raise ValueError("Invalid code-signature command")
            offset, length = struct.unpack_from("<II", data, position + 8)
            if offset < end or offset + length > len(data):
                raise ValueError("Invalid code-signature bounds")
            payload_end = min(payload_end, offset)
        position += command_size
    if position != end or uuid_offset is None or not bundle_id.strip():
        raise ValueError("Missing UUID or app identity")
    # Excluding the header/load commands and signature makes repeated runs stable
    # even after re-signing; the executable payload and bundle identity determine it.
    digest = hashlib.sha256(b"MusicCraftAppUUID-v1\0" + bundle_id.encode() + b"\0" + data[end:payload_end]).digest()
    identity = uuid.UUID(bytes=digest[:16], version=5)
    output = bytearray(data)
    output[uuid_offset:uuid_offset + 16] = identity.bytes
    return bytes(output), identity


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("bundle_id")
    args = parser.parse_args()
    original = args.executable.read_bytes()
    updated, identity = app_uuid(original, args.bundle_id)
    args.executable.write_bytes(updated)
    print(f"UUID app: {identity}")

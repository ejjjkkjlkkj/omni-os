import struct
import tempfile
import unittest
from pathlib import Path

from omni.firmware import inspect


def make_fv_blob() -> bytearray:
    blob = bytearray(256)
    base = 32
    fv_length = 128
    header_length = 64

    blob[base + 32 : base + 40] = fv_length.to_bytes(8, "little")
    blob[base + 40 : base + 44] = b"_FVH"
    blob[base + 44 : base + 48] = (0).to_bytes(4, "little")
    blob[base + 48 : base + 50] = header_length.to_bytes(2, "little")
    blob[base + 50 : base + 52] = b"\x00\x00"
    blob[base + 52 : base + 54] = b"\x00\x00"
    blob[base + 54] = 0
    blob[base + 55] = 2
    # Block map terminator {0, 0}.
    blob[base + 56 : base + 64] = b"\x00" * 8

    header = blob[base : base + header_length]
    total = sum(
        struct.unpack_from("<H", header, offset)[0]
        for offset in range(0, header_length, 2)
    )
    checksum = (-total) & 0xFFFF
    blob[base + 50 : base + 52] = checksum.to_bytes(2, "little")
    return blob


class FirmwareVolumeTests(unittest.TestCase):
    def inspect_blob(self, blob: bytes) -> dict[str, object]:
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "firmware.bin"
            path.write_bytes(blob)
            return inspect(path)

    def test_valid_fv_header_requires_checksum_and_block_map(self):
        result = self.inspect_blob(make_fv_blob())
        volume = result["firmware_volumes"][0]
        self.assertTrue(volume["valid_bounds"])
        self.assertTrue(volume["header_checksum_valid"])
        self.assertTrue(volume["block_map_terminated"])
        self.assertTrue(volume["ext_header_valid"])
        self.assertTrue(volume["valid_header"])

    def test_tampered_fv_header_fails_checksum(self):
        blob = make_fv_blob()
        base = 32
        blob[base + 44] ^= 0x01
        result = self.inspect_blob(blob)
        volume = result["firmware_volumes"][0]
        self.assertTrue(volume["valid_bounds"])
        self.assertFalse(volume["header_checksum_valid"])
        self.assertFalse(volume["valid_header"])

    def test_signature_with_plausible_lengths_is_not_enough(self):
        blob = bytearray(256)
        base = 32
        blob[base + 32 : base + 40] = (128).to_bytes(8, "little")
        blob[base + 40 : base + 44] = b"_FVH"
        blob[base + 48 : base + 50] = (64).to_bytes(2, "little")
        blob[base + 56 : base + 64] = b"\x00" * 8
        result = self.inspect_blob(blob)
        volume = result["firmware_volumes"][0]
        self.assertTrue(volume["valid_bounds"])
        self.assertFalse(volume["header_checksum_valid"])
        self.assertFalse(volume["valid_header"])

    def test_odd_header_length_is_invalid(self):
        blob = make_fv_blob()
        base = 32
        blob[base + 48 : base + 50] = (63).to_bytes(2, "little")
        result = self.inspect_blob(blob)
        volume = result["firmware_volumes"][0]
        self.assertFalse(volume["header_even"])
        self.assertFalse(volume["valid_header"])


if __name__ == "__main__":
    unittest.main()

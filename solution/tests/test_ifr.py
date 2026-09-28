import random
import struct
import unittest

from omni.ifr import IfrError, parse_hii_package_list, parse_ifr_stream

def op(code: int, payload: bytes = b"", scope: bool = False) -> bytes:
    length = 2 + len(payload)
    assert 2 <= length <= 0x7F
    return bytes((code, length | (0x80 if scope else 0))) + payload

class IfrTests(unittest.TestCase):
    def test_semantic_question_and_scope(self):
        form = op(0x01, struct.pack("<H", 7), scope=True)
        question_header = struct.pack("<HHHHHB", 10, 11, 42, 3, 0, 0)
        checkbox = op(0x06, question_header + b"\x00")
        end = op(0x29)
        parsed = parse_ifr_stream(form + checkbox + end)
        self.assertEqual([x.name for x in parsed], ["FORM", "CHECKBOX", "END"])
        self.assertEqual(parsed[1].role, "checkbox")
        self.assertEqual(parsed[1].prompt_id, 10)
        self.assertEqual(parsed[1].question_id, 42)
        self.assertEqual(parsed[1].depth, 1)

    def test_password_never_exports_value_semantics(self):
        qh = struct.pack("<HHHHHB", 20, 21, 9, 1, 0, 0)
        pw = op(0x08, qh + struct.pack("<HH", 0, 64))
        parsed = parse_ifr_stream(pw)
        self.assertTrue(parsed[0].password)
        self.assertEqual(parsed[0].role, "edit")

    def test_package_list_forms(self):
        stream = op(0x01, struct.pack("<H", 1), scope=True) + op(0x29)
        package = struct.pack("<I", (0x02 << 24) | (4 + len(stream))) + stream
        guid = bytes(range(16))
        total = 20 + len(package)
        blob = guid + struct.pack("<I", total) + package
        result = parse_hii_package_list(blob)
        self.assertEqual(result["packages"][0]["type"], 2)
        self.assertEqual(result["packages"][0]["ifr"][0]["role"], "form")

    def test_rejects_bad_lengths_and_scopes(self):
        with self.assertRaises(IfrError):
            parse_ifr_stream(b"\x01\x00")
        with self.assertRaises(IfrError):
            parse_ifr_stream(op(0x29))
        with self.assertRaises(IfrError):
            parse_ifr_stream(op(0x01, struct.pack("<H", 1), scope=True))
        with self.assertRaises(IfrError):
            parse_hii_package_list(b"\x00" * 19)

    def test_random_bytes_fail_closed(self):
        rng = random.Random(0)
        for _ in range(2000):
            data = rng.randbytes(rng.randrange(0, 96))
            try:
                parse_ifr_stream(data)
            except IfrError:
                pass

    def test_all_two_byte_ifr_headers_fail_closed(self):
        for opcode in range(256):
            for encoded in range(256):
                data = bytes((opcode, encoded))
                try:
                    parsed = parse_ifr_stream(data)
                except IfrError:
                    continue
                self.assertEqual(len(parsed), 1)
                self.assertEqual(parsed[0].opcode, opcode)
                self.assertEqual(parsed[0].length, 2)

if __name__ == "__main__":
    unittest.main()

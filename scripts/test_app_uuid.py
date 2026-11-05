import importlib.util
from pathlib import Path
import struct
import unittest

spec = importlib.util.spec_from_file_location("app_uuid", Path(__file__).with_name("app-uuid.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class AppUuidTests(unittest.TestCase):
    def fixture(self):
        header = struct.pack("<IIIIIIII", 0xFEEDFACF, 0x100000C, 0, 2, 2, 40, 0, 0)
        commands = struct.pack("<II", 0x1B, 24) + bytes(range(16)) + struct.pack("<IIII", 0x1D, 16, 80, 8)
        return header + commands + b"PAYLOAD!" + b"SIGNHERE"

    def test_app_and_qa_ids_differ_and_only_uuid_changes(self):
        original = self.fixture()
        prod, prod_uuid = module.app_uuid(original, "io.github.ckiikc.musiccraft")
        test, test_uuid = module.app_uuid(original, "io.github.ckiikc.musiccraft.gapless-test")
        self.assertNotEqual(prod_uuid, test_uuid)
        self.assertEqual(original[:40], prod[:40])
        self.assertEqual(original[56:], prod[56:])
        self.assertEqual(prod[40:56], prod_uuid.bytes)
        self.assertNotEqual(prod, test)

    def test_idempotent_and_independent_of_signature_and_old_uuid(self):
        original = self.fixture()
        first, identity = module.app_uuid(original, "io.github.ckiikc.musiccraft")
        self.assertEqual(module.app_uuid(first, "io.github.ckiikc.musiccraft"), (first, identity))
        resigned = first[:-8] + b"NEWSIGN!"
        self.assertEqual(module.app_uuid(resigned, "io.github.ckiikc.musiccraft")[1], identity)
        changed = bytearray(original)
        changed[72] ^= 1
        self.assertNotEqual(module.app_uuid(bytes(changed), "io.github.ckiikc.musiccraft")[1], identity)

    def test_rejects_bad_headers_and_bounds(self):
        for data in [b"", b"not a Mach-O", self.fixture()[:50]]:
            with self.assertRaises(ValueError):
                module.app_uuid(data, "app")
        bad = bytearray(self.fixture())
        struct.pack_into("<I", bad, 60, 4096)
        with self.assertRaises(ValueError):
            module.app_uuid(bytes(bad), "app")


if __name__ == "__main__":
    unittest.main()

"""The refusal fixture remains a valid package with only one point import's hash changed."""
import hashlib
import importlib.util
import pathlib
import subprocess
import tempfile
import tomllib
import unittest
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools/ci/mismatch_wasm_import.py"
spec = importlib.util.spec_from_file_location("mismatch", SCRIPT)
mismatch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mismatch)

HASH = "0123456789abcdef0123456789abcdef"
MODULE = b"\0asm\x01\0\0\0 junk point:Points::tick#" + HASH.encode() + b" more point:Points::other#" + HASH.encode()


class ImportMismatch(unittest.TestCase):
    def test_only_the_first_points_last_digit_changes(self):
        changed = mismatch.mismatch(MODULE)
        self.assertEqual(len(changed), len(MODULE))
        self.assertEqual(
            changed,
            MODULE.replace(b"tick#" + HASH.encode(), b"tick#" + HASH.encode()[:-1] + b"0", 1),
        )

    def test_a_module_that_imports_no_point_is_refused(self):
        with self.assertRaises(ValueError):
            mismatch.mismatch(b"\0asm\x01\0\0\0")

    def test_package_integrity_survives_the_change(self):
        with tempfile.TemporaryDirectory() as directory:
            source = pathlib.Path(directory) / "source.hmod"
            output = pathlib.Path(directory) / "changed.hmod"
            manifest = 'id = "import_test"\n[wasm]\nsha512 = "' + hashlib.sha512(MODULE).hexdigest() + '"\n'
            with zipfile.ZipFile(source, "w") as archive:
                archive.writestr("sunrise-mod.toml", manifest)
                archive.writestr("mod.wasm", MODULE)
                archive.writestr("data/unchanged.txt", "kept")
            result = subprocess.check_output(["python3", str(SCRIPT), str(source), str(output)], text=True)
            self.assertEqual(result.strip(), "import_test")
            with zipfile.ZipFile(output) as archive:
                changed = archive.read("mod.wasm")
                parsed = tomllib.loads(archive.read("sunrise-mod.toml").decode())
                self.assertEqual(parsed["wasm"]["sha512"], hashlib.sha512(changed).hexdigest())
                self.assertEqual(archive.read("data/unchanged.txt"), b"kept")


if __name__ == "__main__":
    unittest.main()

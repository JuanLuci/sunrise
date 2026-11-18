#!/usr/bin/env python3
"""Make a valid mod package use a point the game has with another signature, for the server refusal check.

A built mod names each item it uses as an import of module `sunrise`, `point:Points::<place>#<hash>`
for a point listener. One digit of the first such hash changes, in place: the name keeps its length, so
no section moves. Rehashing mod.wasm keeps package integrity valid: the loader must reach the import
check, not fail SHA512.
"""
import hashlib
import re
import sys
import tomllib
import zipfile
from pathlib import Path

NAME = re.compile(rb"point:Points::[A-Za-z0-9_]+#[0-9a-f]{32}")


def mismatch(module):
    if module[:8] != b"\0asm\x01\0\0\0":
        raise ValueError("expected a core wasm module")
    found = NAME.search(module)
    if found is None:
        raise ValueError("package imports no point")
    last = found.end() - 1
    flipped = b"1" if module[last:last + 1] == b"0" else b"0"
    return module[:last] + flipped + module[last + 1:]


def main():
    source, destination = map(Path, sys.argv[1:])
    with zipfile.ZipFile(source) as package:
        files = {name: package.read(name) for name in package.namelist()}
    manifest = tomllib.loads(files["sunrise-mod.toml"].decode())
    files["mod.wasm"] = mismatch(files["mod.wasm"])
    digest = hashlib.sha512(files["mod.wasm"]).hexdigest()
    files["sunrise-mod.toml"] = re.sub(
        rb'(?m)^sha512 = "[a-f0-9]+"$', f'sha512 = "{digest}"'.encode(),
        files["sunrise-mod.toml"],
    )
    destination.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(destination, "w", zipfile.ZIP_DEFLATED) as package:
        for name, data in files.items():
            package.writestr(name, data)
    print(manifest["id"])


if __name__ == "__main__":
    main()

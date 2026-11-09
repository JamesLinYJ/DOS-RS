#!/usr/bin/env python3
"""Write one checklist row per upstream v4.0 path.

Usage: scripts/gen-checklist.py FILE_LIST UPSTREAM_ROOT > checklist/v4.0.tsv
"""

import sys
from pathlib import Path


def rust_module(path: str) -> str:
    prefix = "v4.0/src/"
    if not path.startswith(prefix):
        raise SystemExit(f"path outside v4.0/src: {path}")
    rest = path[len(prefix) :]
    parent, _, name = rest.rpartition("/")
    stem, dot, _ext = name.rpartition(".")
    if dot:
        rust_name = f"{stem}.rs"
    else:
        rust_name = f"{name}.rs"
    module = f"{parent}/{rust_name}" if parent else rust_name
    return "src/" + module.lower()


def kind_of(file_path: Path) -> str:
    with file_path.open("rb") as handle:
        return "binary" if b"\0" in handle.read(4096) else "text"


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: gen-checklist.py FILE_LIST UPSTREAM_ROOT")
    list_path = Path(sys.argv[1])
    root = Path(sys.argv[2])
    print("path\tkind\tstatus\trust_module")
    for raw in list_path.read_text().splitlines():
        if not raw:
            continue
        file_path = root / raw
        if not file_path.is_file():
            raise SystemExit(f"missing {file_path}")
        print(f"{raw}\t{kind_of(file_path)}\tuntranslated\t{rust_module(raw)}")


if __name__ == "__main__":
    main()

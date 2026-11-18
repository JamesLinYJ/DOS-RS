#!/usr/bin/env python3
"""Link the standalone Rust cwd acceptance ELF against a frozen native musl sysroot."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def build(arch, output):
    target = arch + "-unknown-linux-gnu"
    cache = ROOT / "target/cwd-probe" / arch / "cargo"
    manifest = ROOT / "tests/native-cwd/Cargo.toml"
    flags = "-C relocation-model=static -C no-redzone=yes"
    env = dict(os.environ, RUSTFLAGS=flags)
    subprocess.run(["cargo", "build", "--locked", "--offline", "--release", "--lib",
                    "--manifest-path", manifest, "--target", target, "--target-dir", cache],
                   cwd=ROOT, env=env, check=True)
    output.parent.mkdir(parents=True, exist_ok=True)
    archive = cache / target / "release/libcwdprobe.a"
    subprocess.run([sys.executable, ROOT / "ports/musl/cc.py", "--arch", arch,
                    "-o", output, "--", "-Wl,--gc-sections,-s",
                    ROOT / "userland/rust/stat.c", archive], cwd=ROOT, check=True)
    binary = output.read_bytes()
    bits = 32 if arch == "i686" else 64
    if binary[:4] != b"\x7fELF" or binary[4] != (1 if bits == 32 else 2):
        raise ValueError("unexpected ELF class")
    if int.from_bytes(binary[18:20], "little") != (3 if bits == 32 else 62):
        raise ValueError("unexpected ELF machine")
    sources = [manifest, ROOT / "tests/native-cwd/Cargo.lock",
               ROOT / "userland/rust/Cargo.toml", ROOT / "userland/rust/stat.c",
               *sorted((ROOT / "tests/native-cwd/src").glob("*.rs")),
               *sorted((ROOT / "userland/rust/src").glob("*.rs"))]
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    libraries = ROOT / "target/musl" / arch / "sysroot/lib"
    report = dict(arch=arch, elf_bits=bits, sha256=sha(output), rustflags=flags,
                  compiler=subprocess.check_output(["rustc", "--version"], cwd=ROOT, text=True).strip(),
                  source_sha256={str(path.relative_to(ROOT)): sha(path) for path in sources},
                  inputs={str(path.relative_to(ROOT)): sha(path)
                          for path in [libraries / "libc.a", libraries / "crt1.o"]})
    output.with_suffix(".json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"{arch}: Rust cwd probe {report['sha256']}", flush=True)


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch", choices=["i686", "x86_64"], required=True)
    cli.add_argument("--output", type=Path, required=True)
    args = cli.parse_args()
    build(args.arch, args.output)

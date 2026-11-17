#!/usr/bin/env python3
"""Build standalone upstream Rust utilities against the native DOS-RS musl port."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
VERSION = "0.12.0"
ARCHIVE = "4fb327655cb4ffcbf2f16550cf9234079ffe839692f7aa1a6eda104af684e122"
TOOLS = (
    "base32", "base64", "comm", "cut", "expand", "fold", "nl", "paste", "printf",
    "seq", "sort", "tr", "unexpand", "uniq",
)
DATA_TOOLS = (
    "basenc", "b2sum", "cksum", "md5sum", "sha1sum", "sha224sum", "sha256sum",
    "sha384sum", "sha512sum", "sum", "expr", "factor", "fmt", "numfmt", "od", "tac", "tsort",
)


SYSTEM_TOOLS = ("arch", "uname", "nproc")
FILE_TOOLS = (
    "chmod", "cp", "du", "install", "ln", "ls", "mv", "readlink", "rm", "split",
    "stat", "tee", "touch", "truncate",
)
TREE_TOOLS = ("dd", "df", "pathchk", "realpath", "shred")
IDENTITY_TOOLS = ("chgrp", "chown", "groups", "id", "whoami")
POSIX_TOOLS = ("csplit", "env", "join", "pr", "printenv", "test")
DEFAULT_TOOLS = TOOLS + DATA_TOOLS + SYSTEM_TOOLS + FILE_TOOLS + TREE_TOOLS + POSIX_TOOLS + IDENTITY_TOOLS


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source(archive):
    if digest(archive) != ARCHIVE:
        raise SystemExit("uutils release archive SHA-256 mismatch")
    patches = sorted((ROOT / "userland/uutils").glob("*.patch"))
    version = hashlib.sha256("".join(map(digest, patches)).encode()).hexdigest()[:16]
    work = ROOT / "target/uutils-source" / version
    work.mkdir(parents=True, exist_ok=True)
    package = "coreutils-" + VERSION
    src = work / package
    if not src.exists():
        with tempfile.TemporaryDirectory(dir=work) as temporary:
            subprocess.run(["tar", "xzf", archive], cwd=temporary, check=True)
            extracted = Path(temporary) / package
            for patch in patches:
                subprocess.run(["patch", "-p1", "-i", patch], cwd=extracted, check=True)
            try:
                extracted.rename(src)
            except OSError:
                if not (src / "Cargo.lock").is_file():
                    raise
    return src


def build(arch, archive, output, names, offline):
    src = source(archive.resolve())
    sysroot = ROOT / "target/musl" / arch / "sysroot"
    libc = sysroot / "lib/libc.a"
    if not libc.is_file():
        raise SystemExit(f"build ports/musl for {arch} first")
    linker = ROOT / "scripts/rust-musl-ld.py"
    triple = arch + "-unknown-linux-musl"
    target = ROOT / "target/rust-std-tools"
    key = triple.replace("-", "_")
    compiler_headers = subprocess.check_output(
        [arch + "-elf-gcc", "-print-file-name=include"], text=True
    ).strip()
    rustflags = ["--cfg", "rustix_use_libc"]
    # musl 1.2 uses 64-bit time_t on i686, including the timespecs in stat.
    if arch == "i686":
        rustflags.extend(["--cfg", "libc_unstable_musl_v1_2"])
    rustflags.extend([
        "-C", "relocation-model=static", "-C", "no-redzone=yes",
        "-C", "link-self-contained=no", "-C", "linker=" + str(linker),
    ])
    env = dict(os.environ)
    env.update({
        "DOSRS_RUST_ARCH": arch,
        "CARGO_ENCODED_RUSTFLAGS": "\x1f".join(rustflags),
        "CC_" + key: arch + "-elf-gcc",
        "AR_" + key: arch + "-elf-ar",
        "CFLAGS_" + key: "-nostdinc -isystem " + str(sysroot / "include") + " -isystem " + compiler_headers,
        "CARGO_PROFILE_RELEASE_PANIC": "abort",
    })
    output.mkdir(parents=True, exist_ok=True)
    # Cargo cannot infer freshness of the external CRT and linker script.
    # Changing a link input must rebuild the final binary even with cached std.
    link_inputs = [libc, sysroot / "lib/crt1.o", ROOT / "ports/musl/user.ld", linker]
    tag = hashlib.sha256("".join(map(digest, link_inputs)).encode()).hexdigest()[:16]
    for name in names:
        cmd = ["cargo", "rustc", "--locked", "--release", "-Zbuild-std=std,panic_abort",
               "--manifest-path", src / "Cargo.toml", "--target", triple,
               "--target-dir", target, "-p", "uu_" + name, "--bin", name]
        if offline:
            cmd.append("--offline")
        subprocess.run([*cmd, "--", "-C", "link-arg=--defsym=__dosrs_runtime_" + tag + "=0"],
                       check=True, env=env, cwd=ROOT)
        shutil.copy2(target / triple / "release" / name, output / name)


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch", required=True, choices=["i686", "x86_64"])
    cli.add_argument("--archive", required=True, type=Path, help="uutils/coreutils 0.12.0 release source archive")
    cli.add_argument("--output", required=True, type=Path)
    cli.add_argument("--offline", action="store_true")
    cli.add_argument("tools", nargs="*", metavar="TOOL")
    args = cli.parse_args()
    if set(args.tools) - set(DEFAULT_TOOLS):
        cli.error("unknown utility in requested build")
    build(args.arch, args.archive, args.output, args.tools or DEFAULT_TOOLS, args.offline)

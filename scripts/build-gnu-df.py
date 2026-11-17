#!/usr/bin/env python3
"""Cross-build an unmodified GNU 9.12 utility oracle for the AArch64 Linux RAM guest."""

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SHA256 = "a480198559733e9b3da999e90543ac6f888a2caa544d8d664c5a1f17e528e210"


def main(args):
    if hashlib.sha256(args.archive.read_bytes()).hexdigest() != SHA256:
        raise SystemExit("GNU coreutils 9.12 source digest mismatch")
    work = args.build_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    source = work / "coreutils-9.12"
    if not source.exists():
        subprocess.run(["tar", "-xJf", args.archive.resolve(), "-C", work], check=True)
    wrapper = work / "aarch64-cc"
    wrapper.write_text(
        '#!/bin/sh\nexec zig cc -target aarch64-linux-musl -static "$@"\n'
    )
    wrapper.chmod(0o755)
    env = dict(
        os.environ,
        CC=str(wrapper),
        AR=args.ar,
        RANLIB=args.ranlib,
        ZIG_GLOBAL_CACHE_DIR=str(work / "zig-cache"),
        FORCE_UNSAFE_CONFIGURE="1",
    )
    if not (source / "config.status").is_file():
        triplet = subprocess.check_output(
            [source / "build-aux/config.guess"], text=True
        ).strip()
        subprocess.run(
            [
                source / "configure",
                "--build=" + triplet,
                "--host=aarch64-linux-musl",
                "--disable-nls",
            ],
            cwd=source,
            env=env,
            check=True,
        )
    cmd = [args.make, "-j" + str(args.jobs), "AR=" + args.ar, "RANLIB=" + args.ranlib]
    # A single executable target bypasses the normal 'all' prerequisite
    # that generates gnulib headers in a fresh cross build.
    subprocess.run(
        [*cmd, "-f", "Makefile", "-f", "-", "dosrs-headers"],
        cwd=source,
        env=env,
        input=b"dosrs-headers: $(BUILT_SOURCES)\n",
        check=True,
    )
    subprocess.run([*cmd, "src/" + args.tool], cwd=source, env=env, check=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source / "src" / args.tool, args.output)


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--archive", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--tool", choices=["df", "id", "groups", "whoami", "chgrp", "chown"], default="df")
    p.add_argument("--build-dir", type=Path, default=ROOT / "target/gnu-linux-df")
    p.add_argument("--ar", default=os.environ.get("LLVM_AR", "llvm-ar"))
    p.add_argument("--ranlib", default=os.environ.get("LLVM_RANLIB", "llvm-ranlib"))
    p.add_argument("--make", default=shutil.which("gmake") or "make")
    p.add_argument("--jobs", type=int, default=8)
    main(p.parse_args())

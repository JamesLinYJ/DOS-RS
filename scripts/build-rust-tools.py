#!/usr/bin/env python3
"""Build standalone DOS-RS Rust ELF programs and stage their /bin contents."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def build(arch, output, include_shell=True, uutils_archive=None):
    sysroot = ROOT / "target/musl" / arch / "sysroot"
    archive = os.environ.get("DOSRS_MUSL_ARCHIVE")
    tarball = Path(archive) if archive else ROOT / "target/upstream/musl-1.2.6.tar.gz"
    if archive or tarball.is_file():
        subprocess.run(
            [
                sys.executable,
                ROOT / "ports/musl/build.py",
                "--arch",
                arch,
                "--archive",
                tarball,
            ],
            check=True,
        )
    elif not (sysroot / "lib/libc.a").is_file():
        raise SystemExit(
            f"Set DOSRS_MUSL_ARCHIVE to musl-1.2.6.tar.gz or supply the prebuilt {arch} DOS-RS musl sysroot"
        )
    output.mkdir(parents=True, exist_ok=True)
    target = arch + "-unknown-linux-gnu"
    cache = ROOT / "target" / "rust-userland"
    env = dict(os.environ, RUSTFLAGS="-C relocation-model=static -C no-redzone=yes")
    projects = [(ROOT / "userland/rust", "--examples")]
    shell = ROOT / "userland/shell"
    if include_shell:
        projects.append((shell, "--lib"))
    for directory, selection in projects:
        manifest = directory / "Cargo.toml"
        cmd = [
            "cargo",
            "build",
            "--locked",
            "--release",
            selection,
            "--target",
            target,
            "--target-dir",
            str(cache),
            "--manifest-path",
            str(manifest),
            "--message-format=json",
        ]
        if selection == "--examples":
            cmd.extend(["--features", "alloc"])
        built = subprocess.run(
            cmd, cwd=ROOT, env=env, stdout=subprocess.PIPE, text=True
        )
        for line in built.stdout.splitlines():
            message = json.loads(line)
            if message["reason"] == "compiler-message":
                print(message["message"]["rendered"], file=sys.stderr, end="")
            if built.returncode or message["reason"] != "compiler-artifact":
                continue
            if "staticlib" not in message["target"]["crate_types"]:
                continue
            name = message["target"]["name"]
            archive = next(path for path in message["filenames"] if path.endswith(".a"))
            subprocess.run(
                [
                    sys.executable,
                    ROOT / "ports/musl/cc.py",
                    "--arch",
                    arch,
                    "-o",
                    output / name,
                    "--",
                    "-Wl,--gc-sections,-s",
                    ROOT / "userland/rust/stat.c",
                    *([shell / "paths.c"] if name == "sh" else []),
                    archive,
                ],
                check=True,
            )
        built.check_returncode()
    if uutils_archive:
        subprocess.run([
            sys.executable, ROOT / "scripts/build-uutils.py", "--arch", arch,
            "--archive", uutils_archive, "--output", output, "--offline",
        ], check=True)


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch", choices=["i686", "x86_64"], required=True)
    cli.add_argument("--output", type=Path, required=True)
    cli.add_argument("--tools-only", action="store_true")
    cli.add_argument("--uutils-archive", type=Path, default=os.environ.get("DOSRS_UUTILS_ARCHIVE"),
                     help="also install standalone std utilities from the pinned uutils release")
    args = cli.parse_args()
    build(args.arch, args.output, not args.tools_only, args.uutils_archive)

#!/usr/bin/env python3
"""Build the DOS device ABI and link the freestanding i386 EMS monitor."""
import argparse
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def build(tools, target):
    target.mkdir(parents=True, exist_ok=True)
    subprocess.run([
        "cargo", "+stable", "build", "--release", "--locked", "--manifest-path",
        tools / "arch386/Cargo.toml", "--target-dir", target / "host",
    ], check=True)
    adapter = target / "host/release/rust16-device"
    subprocess.run([adapter, "caller", target / "device.obj"], check=True)
    subprocess.run([adapter, "client", target / "client.obj"], check=True)
    subprocess.run([adapter, "upper", target / "upper.obj"], check=True)
    subprocess.run([adapter, "header", target / "device.hdr"], check=True)

    spec = json.loads((ROOT / "target/bios-cpu/i686-stage-3.json").read_text())
    spec.update({
        "cpu": "i386",
        "rustc-abi": "softfloat",
        "features": "+soft-float,-sse,-mmx",
        "max-atomic-width": 32,
    })
    model = target / "i386-ems.json"
    content = json.dumps(spec, indent=2)
    if not model.exists() or model.read_text() != content:
        model.write_text(content)
    subprocess.run([
        "cargo", "build", "--locked", "--release", "-Zjson-target-spec", "--target",
        model, "--target-dir", target, "--manifest-path", ROOT / "boot/emm386/Cargo.toml",
    ], cwd=ROOT, check=True)
    elf = target / "i386-ems/release/dos-ems-monitor"
    monitor = target / "monitor.bin"
    subprocess.run(["i686-elf-objcopy", "-O", "binary", elf, monitor], check=True)
    header = (target / "device.hdr").read_bytes()
    (target / "EMM386.SYS").write_bytes(header + monitor.read_bytes())


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--toolchain", required=True, type=Path)
    args = cli.parse_args()
    build(args.toolchain.resolve(), ROOT / "target/native16/emm")

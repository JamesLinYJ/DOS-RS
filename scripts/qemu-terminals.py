#!/usr/bin/env python3
"""Run simultaneous DOS programs on four virtual terminals on both targets."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def prepare(out):
    image = out / "terminals.img"
    shutil.copyfile(ROOT / "disks/guest12.img", image)
    for name in ["SPIN", "WAITKEY", "CRASH", "CALC"]:
        program = out / (name + ".COM")
        subprocess.run(["nasm", "-f", "bin", "-D" + name, ROOT / "tests/terminals.asm",
                        "-o", program], check=True)
        subprocess.run(["mcopy", "-o", "-i", image, program, "::"], check=True)
    batch = out / "AUTOEXEC.BAT"
    batch.write_bytes(b"ECHO Ready\r\n")
    subprocess.run(["mcopy", "-o", "-i", image, batch, "::"], check=True)
    return image


def check(out, image, target, display):
    arch = target.split("-")[0]
    emulator = "qemu-system-" + ("i386" if arch == "i686" else "x86_64")
    build = ["cargo", "build", "--release", "--target-dir", ROOT / "target/terminals"]
    build += ["-Zjson-target-spec", "--target", "./i686-unknown-none.json"] if arch == "i686" else ["--target", target]
    subprocess.run(build, cwd=ROOT, env={**os.environ, "DOS_RS_GUEST_IMAGE": str(image)}, check=True)
    kernel = ROOT / "target/terminals" / target / "release/dos4-rs"
    log = out / (arch + ".txt")
    log.write_bytes(b"")
    cmd = [emulator, "-kernel", kernel, "-display", display, "-vga", "std", "-m", "64",
           "-serial", "file:" + str(log), "-monitor", "stdio", "-nic", "none", "-no-reboot"]
    vm = subprocess.Popen(cmd, cwd=out, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL)

    def monitor(command):
        vm.stdin.write((command + "\n").encode())
        vm.stdin.flush()

    def key(name):
        monitor("sendkey " + name + " 15")
        time.sleep(.035)

    def enter(line):
        for char in line:
            key({".": "dot", " ": "spc"}.get(char, char))
        key("ret")

    def wait(marker, count=1):
        end = time.monotonic() + 12
        while time.monotonic() < end:
            if log.read_bytes().count(marker.encode()) >= count:
                return
            if vm.poll() is not None:
                break
            time.sleep(.03)
        raise RuntimeError(f"{arch} missing {marker!r}\n{log.read_bytes()[-1600:]!r}")

    try:
        wait("A:\\> ")
        enter("spin.com")
        wait("BACKGROUND RUNNING")
        key("alt-f2")
        wait("DOS-RS terminal 2")
        enter("waitkey.com")
        wait("WAITING FOR A")
        key("alt-f3")
        wait("DOS-RS terminal 3")
        enter("crash.com")
        wait("Program stopped at opcode 63")
        key("alt-f4")
        wait("DOS-RS terminal 4")
        enter("calc.com")
        wait("CALCULATION COMPLETE")
        wait("BACKGROUND SAW FILE")
        key("alt-f2")
        key("a")
        wait("KEY DELIVERED")
        enter("type shared.txt")
        wait("VT shared file")
        (out / (arch + ".ppm")).unlink(missing_ok=True)
        monitor(f"screendump {arch}.ppm")
        end = time.monotonic() + 3
        while not (out / (arch + ".ppm")).exists() and time.monotonic() < end:
            time.sleep(.02)
        print(f"{arch} four-terminal check passed", flush=True)
    finally:
        if vm.poll() is None:
            monitor("quit")
            vm.wait(timeout=5)


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--output", type=Path, default=ROOT / "target/terminal-proof")
    cli.add_argument("--display", default="none")
    args = cli.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    image = prepare(out)
    for target in ["i686-unknown-none", "x86_64-unknown-none"]:
        check(out, image, target, args.display)

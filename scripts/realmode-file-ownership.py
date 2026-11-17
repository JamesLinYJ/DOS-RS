#!/usr/bin/env python3
"""Cold-boot the native DOS file-ownership regression on disposable FAT volumes."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
PAYLOAD = b"0123456789ABCDEF"
TRIALS = {"CTRL.BIN": "control", "EXECA.BIN": "exec-com", "EXECB.BIN": "exec-exe"}


def fat_tool(name):
    result = shutil.which(name)
    if result:
        return result
    for base in ("/opt/homebrew/opt/dosfstools/sbin", "/usr/local/opt/dosfstools/sbin"):
        path = Path(base) / name
        if path.is_file():
            return path
    raise FileNotFoundError(name)


def client(tools):
    build = ROOT / "target/file-ownership"
    build.mkdir(parents=True, exist_ok=True)
    io, adapter = build / "io.obj", build / "client.obj"
    subprocess.run([tools / "target/release/bios16", "--dos-io", io], check=True)
    host = ROOT / "target/native16/emm/host"
    subprocess.run(["cargo", "+stable", "build", "--locked", "--release", "--manifest-path",
                    tools / "arch386/Cargo.toml", "--target-dir", host], check=True)
    subprocess.run([host / "release/rust16-device", "client", adapter], check=True)
    env = dict(os.environ, RUST16_PLATFORM="dos", RUST16_CPU="286",
               RUST16_OBJECTS=os.pathsep.join(map(str, (io, adapter))))
    subprocess.run([tools / "scripts/cargo16.sh", "--locked", "--release", "--manifest-path",
                    ROOT / "native-tools/Cargo.toml", "--target-dir", build,
                    "--example", "file_ownership"], check=True, env=env)
    return build / "dos16/release/examples/file_ownership.exe"


def mcopy(image, src, dest, data=None):
    return subprocess.run(["mcopy", "-o", "-i", image, src, dest], check=True,
                          input=data, stdout=subprocess.PIPE).stdout


def boot(work, proof, name, cpu, cyl, commands, image, dosbox):
    floppy = work / "boot.img"
    shutil.copyfile(image, floppy)
    batch = "\r\n".join(["C:", *commands, ""]).encode("ascii")
    mcopy(floppy, "-", "::AUTOEXEC.BAT", batch)
    serial = proof / (name + ".serial")
    serial.unlink(missing_ok=True)
    hard = work / "drive.img"
    if cpu == "286":
        conf = work / "dosbox.conf"
        conf.write_text(
            "[sdl]\noutput=surface\n[dosbox]\nshow start banner=false\nquit warning=false\n"
            "[cpu]\ncore=normal\ncputype=286\ncycles=fixed 50000\n[mixer]\nnosound=true\n"
            f"[serial]\nserial1=file file:{serial} timeout:0\n[autoexec]\nmount d .\n"
            f"imgmount 2 d:\\drive.img -t hdd -fs none -size 512,63,16,{cyl}\nboot boot.img\n"
        )
        args = [dosbox, "-conf", conf, "-nogui", "-nomenu"]
    else:
        # QEMU TCG models start at the 486. DOS selects its 80386 kernel on this model.
        args = ["qemu-system-i386", "-machine", "pc,accel=tcg", "-cpu", "486", "-m", "16",
                "-boot", "a", "-drive", f"if=floppy,format=raw,file={floppy}",
                "-drive", f"if=ide,format=raw,file={hard}", "-display", "none",
                "-monitor", "none", "-serial", f"file:{serial}", "-no-reboot"]
    with (proof / (name + ".log")).open("wb") as out:
        proc = subprocess.Popen(args, cwd=work, stdout=out, stderr=out,
                                env=dict(os.environ, SDL_VIDEODRIVER="dummy", SDL_AUDIODRIVER="dummy"))
        try:
            deadline = time.monotonic() + 75
            while time.monotonic() < deadline:
                data = serial.read_bytes() if serial.exists() else b""
                if b"C:\\> " in data:
                    break
                if proc.poll() is not None:
                    raise RuntimeError(f"{name} emulator exited, see {out.name}")
                time.sleep(0.1)
            else:
                raise RuntimeError(f"{name} boot timed out, serial tail {data[-200:]!r}")
        finally:
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
    expected_cpu = b"80286" if cpu == "286" else b"80386"
    if b"DOS / " + expected_cpu not in data:
        raise RuntimeError(f"{name} incorrect guest CPU: {data[:120]!r}")
    return data


def matrix(args, exe, cpu, fat):
    # Full cylinders allow DOSBox-X and QEMU to expose identical sector counts.
    cyl = 16 if fat == 16 else 68
    prefix = f"{cpu}-fat{fat}"
    failures = []
    with tempfile.TemporaryDirectory(dir=args.ram_dir, prefix="owner-") as temp:
        work = Path(temp)
        disk = work / "drive.img"
        with disk.open("wb") as stream:
            stream.truncate(cyl * 16 * 63 * 512)
        subprocess.run([fat_tool("mkfs.fat"), "-F", str(fat), "-s", "1", disk], check=True)
        mcopy(disk, exe, "::OWNER.EXE")
        mcopy(disk, ROOT / "tests/fixtures/dos/FILES.COM", "::FILES.COM")
        def run(phase, commands):
            return boot(work, args.proof, prefix + "-" + phase, cpu, cyl, commands,
                        args.image, args.dosbox)
        write = run("write", ["OWNER N", "OWNER A", "OWNER B"])
        if f"C: FAT{fat}".encode() not in write or write.count(b"OWN: OK") != 3 or b"OWN: child ran" not in write:
            failures.append(f"DOS API or EXEC failed: {write!r}")
        read = run("reboot", ["OWNER READ"])
        if b"OWN: OK" not in read:
            failures.append(f"DOS read after reboot: {read!r}")
        for filename, test in TRIALS.items():
            value = mcopy(disk, "::" + filename, "-")
            print(f"{prefix} {test}: {value.hex()}", flush=True)
            if value != PAYLOAD:
                failures.append(f"{test} expected {PAYLOAD.hex()}, got {value.hex()}")
        fsck = subprocess.run([fat_tool("fsck.fat"), "-n", disk], capture_output=True)
        (args.proof / (prefix + "-fsck.txt")).write_bytes(fsck.stdout + fsck.stderr)
        if fsck.returncode:
            failures.append(f"fsck.fat -n exited {fsck.returncode}")
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("toolchain", "image", "ram-dir", "proof"):
        parser.add_argument("--" + option, type=Path, required=True)
    parser.add_argument("--cpu", choices=("286", "386", "both"), default="both")
    parser.add_argument("--fat", choices=("16", "32", "both"), default="both")
    parser.add_argument("--dosbox", default=shutil.which("dosbox-x") or "dosbox-x")
    args = parser.parse_args()
    args.proof = args.proof.resolve()
    args.proof.mkdir(parents=True, exist_ok=True)
    args.image = args.image.resolve()
    print("Boot image SHA-256", hashlib.sha256(args.image.read_bytes()).hexdigest(), flush=True)
    exe = client(args.toolchain.resolve())
    failures = []
    for cpu in (("286", "386") if args.cpu == "both" else (args.cpu,)):
        for fat in ((16, 32) if args.fat == "both" else (int(args.fat),)):
            for error in matrix(args, exe, cpu, fat):
                failures.append(f"{cpu} FAT{fat}: {error}")
    for failure in failures:
        print("FAIL", failure, file=sys.stderr)
    if not failures:
        print("PASS: native DOS file ownership and cold reboot")
    return int(bool(failures))


if __name__ == "__main__":
    sys.exit(main())

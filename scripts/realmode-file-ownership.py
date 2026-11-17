#!/usr/bin/env python3
"""Cold-boot the native DOS file-ownership regression on disposable FAT volumes."""
import argparse
import hashlib
import os
import re
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
    floppy.write_bytes(image)
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


def free_bytes(disk):
    listing = subprocess.check_output(["mdir", "-i", disk, "::"], env=dict(os.environ, LC_ALL="C"))
    match = re.search(rb"([\d ]+) bytes free", listing)
    if match is None:
        raise RuntimeError(f"cannot determine free space from mdir: {listing!r}")
    return int(match.group(1).replace(b" ", b""))


def fill_volume(disk):
    # The sparse host file and the virtual disk both live in the private RAM directory.
    with tempfile.TemporaryFile(dir=disk.parent) as padding:
        padding.truncate(free_bytes(disk) - 512)
        subprocess.run(["mcopy", "-i", disk, "-", "::FILLER.BIN"], stdin=padding, check=True)
    if free_bytes(disk) != 512:
        raise RuntimeError("expected exactly one free 512-byte cluster before the near-full write")


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
        mcopy(disk, "-", "::OWNER.EXE", exe)
        mcopy(disk, ROOT / "tests/fixtures/dos/FILES.COM", "::FILES.COM")
        if args.suite == "all":
            # Input for the historical COM child, which also creates its own API.TXT.
            mcopy(disk, "-", "::BIG.BIN", bytes(65538) + b"ABCD")
            mcopy(disk, "-", "::SEQ.BIN", b"0123456789")
            mcopy(disk, "-", "::GUARD.BIN", bytes(range(256)) * 4)

        def run(phase, commands):
            return boot(work, args.proof, prefix + "-" + phase, cpu, cyl, commands,
                        args.image, args.dosbox)
        write = run("write", ["OWNER N", "OWNER A", "OWNER B"])
        if f"C: FAT{fat}".encode() not in write or write.count(b"OWN: OK") != 3 or b"OWN: child ran" not in write:
            failures.append(f"DOS API or EXEC failed: {write!r}")
        if args.suite == "all":
            if b"INT21 7.10 FILE API OK" not in write:
                failures.append("the COM child failed its DOS file API check")
            advanced = run("handles-stream", ["OWNER HANDLES", "OWNER STREAM", "OWNER SHORT"])
            if advanced.count(b"OWN: OK") != 3 or b"OWN: child closed inherited handles" not in advanced:
                failures.append(f"handle/stream DOS checks: {advanced!r}")
            fill_volume(disk)
            full = run("full", ["OWNER FULL"])
            if b"OWN: OK" not in full:
                failures.append(f"near-full DOS writes: {full!r}")
        read = run("reboot", ["OWNER READ", "OWNER LARGE"] if args.suite == "all" else ["OWNER READ"])
        if read.count(b"OWN: OK") != (2 if args.suite == "all" else 1):
            failures.append(f"DOS read after reboot: {read!r}")
        expected = {name: PAYLOAD for name in TRIALS}
        if args.suite == "all":
            expected["API.TXT"] = b"ABCD"
            expected["GUARD.BIN"] = bytes(range(256)) * 4 + b"\xac" * 512
            expected["SEQ.BIN"] = b"0123456789"
            expected["STREAM.BIN"] = bytes(
                (block ^ (block >> 8) ^ i ^ (i >> 8)) & 255
                for block in range(257) for i in range(512)
            )[:0x20017]
        # mtools 4.0.49 rejects a valid FAT16 link to the very last data
        # cluster when the volume is full. Extract with an independent FAT reader.
        unzip = shutil.which("7zz") or shutil.which("7z")
        if unzip is None:
            raise FileNotFoundError("7zz or 7z is required for raw FAT extraction")
        report = [f"boot-image sha256 {hashlib.sha256(args.image).hexdigest()}"]
        for filename, want in expected.items():
            try:
                value = subprocess.check_output([unzip, "x", "-tFAT", "-so", disk, filename])
            except subprocess.CalledProcessError:
                failures.append(f"{filename} unavailable after reboot")
                continue
            digest = hashlib.sha256(value).hexdigest()
            summary = value.hex() if len(value) <= 16 else digest
            print(f"{prefix} {filename}: {len(value)} bytes {summary}", flush=True)
            report.append(f"{filename} {len(value)} bytes sha256 {digest} match={value == want}")
            if value != want:
                failures.append(f"{filename} expected {hashlib.sha256(want).hexdigest()}, got {digest}")
        (args.proof / (prefix + "-bytes.txt")).write_text("\n".join(report) + "\n")
        fsck = subprocess.run([fat_tool("fsck.fat"), "-n", disk], capture_output=True)
        (args.proof / (prefix + "-fsck.txt")).write_bytes(fsck.stdout + fsck.stderr)
        if fsck.returncode:
            failures.append(f"fsck.fat -n exited {fsck.returncode}")
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("toolchain", "image", "ram-dir", "proof"):
        parser.add_argument("--" + option, type=Path, required=True)
    parser.add_argument("--suite", choices=("regression", "all"), default="all")
    parser.add_argument("--cpu", choices=("286", "386", "both"), default="both")
    parser.add_argument("--fat", choices=("16", "32", "both"), default="both")
    parser.add_argument("--dosbox", default=shutil.which("dosbox-x") or "dosbox-x")
    args = parser.parse_args()
    args.proof = args.proof.resolve()
    args.proof.mkdir(parents=True, exist_ok=True)
    args.image = args.image.read_bytes()
    print("Boot image SHA-256", hashlib.sha256(args.image).hexdigest(), flush=True)
    exe = client(args.toolchain.resolve()).read_bytes()
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

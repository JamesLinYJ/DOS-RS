"""Cold boot the native image and verify DOS file calls on a disposable FAT32 disk."""

import argparse
import hashlib
import mmap
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def run(*args, **kwargs):
    subprocess.run(args, check=True, **kwargs)


def contents(image, name):
    return subprocess.check_output(["mtype", "-i", image, "::" + name])


def high_clusters(image):
    """Relocate the root and a file head, preserving both FATs and backup BPB."""
    with image.open("r+b") as stream:

        def read(at, size):
            stream.seek(at)
            return stream.read(size)

        def write(at, data):
            stream.seek(at)
            stream.write(data)

        def word(at):
            return struct.unpack("<H", read(at, 2))[0]

        def dword(at):
            return struct.unpack("<I", read(at, 4))[0]

        def set_dword(at, value):
            write(at, struct.pack("<I", value))

        reserved, size, copies = word(14), dword(36), read(16, 1)[0]
        cluster_bytes = read(13, 1)[0] * 512
        first = (reserved + size * copies) * 512

        def address(c):
            return first + (c - 2) * cluster_bytes

        def fat(c):
            return dword(reserved * 512 + c * 4) & 0xFFFFFFF

        def relocate(old, new):
            if fat(new):
                raise ValueError("fixture destination already allocated")
            write(address(new), read(address(old), cluster_bytes))
            nxt = fat(old)
            for table in range(copies):
                base = (reserved + table * size) * 512
                set_dword(base + new * 4, nxt)
                set_dword(base + old * 4, 0)

        root = 66536
        relocate(dword(44), root)
        backup = word(50)
        set_dword(44, root)
        set_dword(backup * 512 + 44, root)
        cluster = root
        while 2 <= cluster < 0xFFFFFF8:
            for i in range(0, cluster_bytes, 32):
                at = address(cluster) + i
                if read(at, 11) == b"CHAIN   TXT":
                    head = word(at + 26) | (word(at + 20) << 16)
                    relocate(head, 68001)
                    write(at + 20, struct.pack("<H", 68001 >> 16))
                    write(at + 26, struct.pack("<H", 68001 & 65535))
            cluster = fat(cluster)
        info = word(48)
        for lba in (info, backup + info):
            set_dword(lba * 512 + 492, 68002)


def prepare(d, tools):
    fixtures = ROOT / "tests/native16"
    check = d / "FATCHK.EXE"
    run(
        tools / "scripts/rustc16.sh",
        "-Copt-level=2",
        "-Ctarget-cpu=286",
        fixtures / "fatchk.rs",
        "-o",
        d / "check.obj",
    )
    run(
        tools / "target/release/link16",
        d / "check.obj",
        tools / "dist/RT.OBJ",
        "-o",
        check,
    )
    run("nasm", "-f", "bin", fixtures / "files.asm", "-o", d / "FILES.COM")
    note = d / "NOTE.TXT"
    note.write_bytes(b"Subdirectory read OK\r\n")
    batch = d / "AUTOEXEC.BAT"
    batch.write_bytes(
        b"FATCHK A\r\nC:\r\nDIR\r\nFATCHK A\r\nCOPY CHAIN.TXT CLONE.TXT\r\n"
        b"COPY BIG.BIN BIGCOPY.BIN\r\nCOPY DATA\\INNER.TXT CROSS.TXT\r\nA:\\FILES.COM\r\nCD DATA\r\nTYPE INNER.TXT\r\nCD ..\r\nVER\r\n"
    )
    boot = d / "boot.img"
    run(
        sys.executable,
        ROOT / "scripts/realmode-image.py",
        "--toolchain",
        tools,
        "--output",
        boot,
        "--file",
        batch,
        "--file",
        check,
        "--file",
        d / "FILES.COM",
    )
    disk = d / "drive.img"
    with disk.open("wb") as out:
        out.truncate(80 * 16 * 63 * 512)
    run(
        "mformat", "-i", disk, "-F", "-c", "1", "-t", "80", "-h", "16", "-n", "63", "::"
    )

    def copy(path, name):
        run("mcopy", "-i", disk, path, "::" + name)

    for index in range(20):
        copy(note, f"N{index:02}.TXT")
    big = d / "BIG.BIN"
    data = bytearray(102400)
    data[65538:65542] = b"ABCD"
    big.write_bytes(data)
    copy(big, big.name)
    run("mmd", "-i", disk, "::DATA")
    copy(note, "DATA/INNER.TXT")
    chain = d / "CHAIN.TXT"
    chain.write_bytes(b"Across cluster boundaries\r\n" * 190)
    copy(chain, chain.name)
    copy(check, check.name)
    high_clusters(disk)
    if contents(disk, "CHAIN.TXT") != chain.read_bytes():
        raise ValueError("fixture chain")
    return boot, disk


def check_log(log, cpu, filesystem="FAT32"):
    data = log.read_bytes()
    for text in (
        f"C: {filesystem}\r\n".encode(),
        f"DOS / 80{cpu}\r\n".encode(),
        b"INT21 7.10 FILE API OK\r\n",
        b"Subdirectory read OK\r\n",
        b"C:\\> ",
    ):
        if text not in data:
            raise RuntimeError(f"missing {text!r} in {log}")
    if data.count(b"FAT ABI OK\r\n") != 2 or data.count(b"File copied\r\n") != 3:
        raise RuntimeError(f"failed executable or COPY in {log}")


def wait(log, marker, offset, vm, timeout):
    stop = time.monotonic() + timeout
    while time.monotonic() < stop and vm.poll() is None:
        if log.exists() and marker in log.read_bytes()[offset:]:
            return
        time.sleep(0.08)
    if vm.poll() is not None:
        raise RuntimeError(
            f"emulator exited with {vm.returncode} before {marker!r} in {log}"
        )
    raise RuntimeError(f"timeout waiting for {marker!r} in {log}")


def dosbox(d, proof, emu, cpu):
    # Short local image names also work when the host path exceeds DOS's tail limit.
    conf = d / "machine.conf"
    log = proof / f"boot-{cpu}.txt"
    log.unlink(missing_ok=True)
    conf.write_text(
        f"[sdl]\noutput=surface\n[dosbox]\nshow start banner=false\nquit warning=false\n[cpu]\ncore=normal\ncputype={cpu}\ncycles=fixed 30000\n"
        f"[mixer]\nnosound=true\n[serial]\nserial1=file file:{log} timeout:0\n[autoexec]\n"
        "mount d .\nimgmount 2 d:\\drive.img -t hdd -fs none -size 512,63,16,80 > d:\\mount.txt\nboot boot.img\n"
    )
    with (proof / f"dosbox-{cpu}.txt").open("wb") as trace:
        vm = subprocess.Popen(
            [emu, "-conf", conf, "-nogui", "-nomenu"],
            cwd=d,
            stdout=trace,
            stderr=trace,
            env=dict(os.environ, SDL_VIDEODRIVER="dummy", SDL_AUDIODRIVER="dummy"),
        )
        try:
            wait(log, b"\\> ", 0, vm, 45)
        finally:
            if vm.poll() is None:
                vm.terminate()
                try:
                    vm.wait(4)
                except subprocess.TimeoutExpired:
                    vm.kill()
                    vm.wait()
    for mount_log in d.glob("*"):
        if mount_log.name.lower() == "mount.txt":
            (proof / f"mount-{cpu}.txt").write_bytes(mount_log.read_bytes())
    check_log(log, cpu)


def qemu(d, proof, machine, image="drive.img", filesystem="FAT32"):
    log = proof / f"qemu-{machine}-{image}.txt"
    log.unlink(missing_ok=True)
    vm = subprocess.Popen(
        [
            "qemu-system-" + machine,
            "-m",
            "16",
            "-boot",
            "a",
            "-display",
            "none",
            "-nic",
            "none",
            "-drive",
            # These private, sequentially attached images have one owner. HFS+
            # RAM volumes on macOS do not reliably support QEMU's byte locks.
            "if=floppy,readonly=on,format=raw,file.driver=file,file.filename=boot.img,file.locking=off",
            "-drive",
            "if=ide,format=raw,file.driver=file,file.locking=off,file.filename="
            + image,
            "-serial",
            "file:" + str(log),
            "-monitor",
            "stdio",
        ],
        cwd=d,
        stdin=subprocess.PIPE,
        stdout=subprocess.DEVNULL,
    )

    def command(line):
        vm.stdin.write((line + "\n").encode())
        vm.stdin.flush()

    try:
        wait(log, b"C:\\> ", 0, vm, 30)
        offset = log.stat().st_size
        for key in ("v", "e", "r", "ret"):
            command("sendkey " + key + " 30")
            time.sleep(0.08)
        wait(log, b"ver\r\nDOS / 80386\r\nC:\\> ", offset, vm, 10)
    finally:
        if vm.poll() is None:
            command("quit")
            vm.wait(10)
    check_log(log, "386", filesystem)


def partition_test(d, proof, fsck):
    raw = d / "drive.img"
    mbr = bytearray(512)
    start = 2048
    size = raw.stat().st_size // 512
    mbr[446 + 4] = 0x0C
    struct.pack_into("<II", mbr, 446 + 8, start, size)
    mbr[510:] = bytes((0x55, 0xAA))
    image = d / "mbr.img"
    raw.rename(image)
    offset = start * 512
    length = size * 512
    # Shift the volume in place so the RAM disk only needs one full image.
    with image.open("r+b") as f:
        f.truncate(offset + length)
        with mmap.mmap(f.fileno(), 0) as data:
            data.move(offset, 0, length)
            data[:offset] = bytes(offset)
            data[:512] = mbr
            backup = struct.unpack_from("<H", data, offset + 50)[0]
            for base in (offset, offset + backup * 512):
                struct.pack_into("<I", data, base + 28, start)
    qemu(d, proof, "i386", image.name)
    with image.open("r+b") as f:
        with mmap.mmap(f.fileno(), 0) as data:
            data.move(0, offset, length)
        f.truncate(length)
    run(fsck, "-n", image)
    if contents(image, "CROSS.TXT") != (d / "NOTE.TXT").read_bytes():
        raise RuntimeError("cross-directory COPY")
    image.unlink()


def fat16_test(d, proof, fsck):
    disk = d / "fat16.img"
    with disk.open("wb") as f:
        f.truncate(16 * 1024 * 1024)
    run("mformat", "-i", disk, "-T", "32768", "-c", "2", "::")
    run("mmd", "-i", disk, "::DATA")
    for name in ("CHAIN.TXT", "BIG.BIN", "FATCHK.EXE"):
        run("mcopy", "-i", disk, d / name, "::" + name)
    run("mcopy", "-i", disk, d / "NOTE.TXT", "::DATA/INNER.TXT")
    qemu(d, proof, "i386", disk.name, "FAT16")
    if contents(disk, "BIGCOPY.BIN") != (d / "BIG.BIN").read_bytes():
        raise RuntimeError("FAT16 copy")
    run(fsck, "-n", disk)


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--toolchain", required=True, type=Path)
    cli.add_argument("--ram-dir", required=True, type=Path)
    cli.add_argument("--dosbox-x", default="dosbox-x")
    cli.add_argument("--fsck", default="fsck.fat")
    cli.add_argument("--proof", type=Path, default=ROOT / "target/native16/proof")
    args = cli.parse_args()
    proof = args.proof.resolve()
    proof.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=args.ram_dir) as temp:
        d = Path(temp).resolve()
        _, disk = prepare(d, args.toolchain.resolve())
        for kind in ("286", "386", "i386", "x86_64"):
            if kind in ("286", "386"):
                dosbox(d, proof, args.dosbox_x, kind)
            else:
                qemu(d, proof, kind)
            for source, copy in (
                ("CHAIN.TXT", "CLONE.TXT"),
                ("BIG.BIN", "BIGCOPY.BIN"),
            ):
                actual = contents(disk, copy)
                if actual != (d / source).read_bytes():
                    raise RuntimeError(copy + " content differs")
                print(kind, copy, hashlib.sha256(actual).hexdigest(), flush=True)
            if contents(disk, "API.TXT") != b"ABCD":
                raise RuntimeError("incorrect DOS handle output")
            run(args.fsck, "-n", disk)
        partition_test(d, proof, args.fsck)
        fat16_test(d, proof, args.fsck)


if __name__ == "__main__":
    main()

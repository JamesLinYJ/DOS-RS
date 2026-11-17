#!/usr/bin/env python3
"""Verify native threads from ring 3, the kernel trace and a stopped QEMU FAT disk."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell

THREAD = re.compile(rb"THREAD pid=(\d+) tid=(\d+) cpu=(\d+) start=(\d+) end=(\d+)")
FUTEX = re.compile(rb"FUTEX pid=(\d+) from=(\d+) to=(\d+) from_cpu=(\d+) to_cpu=(\d+)")
HEADER = struct.Struct("<6I")
WORKER = struct.Struct("<4I")
SAMPLE = struct.Struct("<QQII")
SAMPLES = 64
ITERATIONS = 524288


def checksum(seed):
    value = seed
    for _ in range(SAMPLES * ITERATIONS):
        value ^= value << 13 & 0xffffffff
        value ^= value >> 17
        value ^= value << 5 & 0xffffffff
    return value


def checked(condition, message):
    if not condition:
        raise RuntimeError(message)


def verify(data, serial, cpus, sums):
    checked(len(data) == HEADER.size + 2 * (WORKER.size + SAMPLES * SAMPLE.size),
            "thread.bin is incomplete")
    magic, pid, parent, samples, count, workers = HEADER.unpack_from(data)
    checked((magic, samples, count, workers) == (0x31524854, SAMPLES, ITERATIONS, 2),
            "unexpected thread report format")
    checked(pid == parent and pid > 0, "initial thread and process identity disagree")
    intervals = [tuple(map(int, row)) for row in THREAD.findall(serial)]
    intervals = [(tid, cpu, start, end) for p, tid, cpu, start, end in intervals if p == pid]
    checked(intervals and all(cpu < cpus and start < end for _, cpu, start, end in intervals),
            "missing or invalid scheduler trace")
    offset = HEADER.size
    results, tids, contained = [], [], []
    for i in range(workers):
        tid, owner, value, err = WORKER.unpack_from(data, offset)
        offset += WORKER.size
        checked(owner == pid and tid > 0 and tid not in (parent, *tids), "worker PID/TID mismatch")
        checked(value == sums[i] and err == (9, 35)[i], "worker computation or errno mismatch")
        tids.append(tid)
        slices = [(cpu, start, end) for t, cpu, start, end in intervals if t == tid]
        complete = []
        for _ in range(samples):
            start, end, cpu, last_cpu = SAMPLE.unpack_from(data, offset)
            offset += SAMPLE.size
            checked(cpu < cpus and last_cpu < cpus and start < end, "bad ring-3 CPU or TSC sample")
            if cpu == last_cpu and any(c == cpu and a <= start < end <= b for c, a, b in slices):
                complete.append((cpu, start, end))
        checked(complete, f"no complete ring-3 sample inside a matching CPU slice for TID {tid}")
        contained.append(complete)
        results.append(dict(tid=tid, checksum=value, errno=err, contained_samples=len(complete)))
    wakeups = [tuple(map(int, row)) for row in FUTEX.findall(serial)]
    wakeups = [(t, a, b) for p, f, t, a, b in wakeups if p == pid and f == parent and t in tids]
    checked({t for t, _, _ in wakeups} == set(tids), "missing positive futex wake for a worker")
    overlap = None
    if cpus == 1:
        ordered = sorted((a, b) for _, _, a, b in intervals)
        checked(all(b <= c for (_, b), (c, _) in zip(ordered, ordered[1:])),
                "scheduler reported simultaneous slices on a single vCPU")
    else:
        checked(any(a != b for _, a, b in wakeups), "missing cross-CPU futex wake")
        for cpu, a, b in contained[0]:
            for other, c, d in contained[1]:
                if cpu != other and max(a, c) < min(b, d):
                    overlap = dict(cpus=[cpu, other], samples=[[a, b], [c, d]],
                                   overlap_tsc=min(b, d) - max(a, c))
                    break
            if overlap:
                break
        checked(overlap, "no overlapping ring-3 computation in two distinct vCPU slices")
    return dict(pid=pid, parent=parent, workers=results, wakeups=wakeups, overlap=overlap)


def exit_code(shell, code):
    start = shell.log.stat().st_size
    shell.command("echo $?")
    response = shell.log.read_bytes()[start:]
    status = re.search(rb"\r?\n(\d+)\r?\n", response)
    checked(status and int(status.group(1)) == code, f"expected exit {code}, got {response[-1000:]!r}")


def run(arch, firmware, cpus, kernel, work, proof, sums):
    name = f"{arch}-{firmware}-smp{cpus}"
    disk = work / "c.img"
    with disk.open("wb") as f:
        f.truncate(4 << 20)
    subprocess.run(["mformat", "-i", disk, "::"], check=True)
    binaries = {}
    for app in ["threads", "hello"]:
        elf = work / (app + ".elf")
        subprocess.run([sys.executable, qemu.ROOT / "scripts/native-cc.py", "--arch", arch,
                        "-o", elf, qemu.ROOT / "native-exec/examples" / (app + ".c")], check=True)
        subprocess.run(["mcopy", "-i", disk, elf, "::"], check=True)
        binaries[elf.name] = hashlib.sha256(elf.read_bytes()).hexdigest()
    loaded = work / "kernel.efi"
    shutil.copyfile(kernel, loaded)
    log = proof / (name + ".serial")
    shell = Shell(qemu.boot_args(loaded, arch, work / "boot", cpus, firmware) +
                  ["-accel", "tcg,thread=multi", "-drive", f"if=ide,format=raw,file={disk}"], log, work)
    try:
        shell.wait(b"A:\\> ")
        shell.command("c:")
        shell.command("threads.elf > thread.bin")
        exit_code(shell, 0)
        shell.command("threads.elf send | threads.elf receive > pages.txt")
        exit_code(shell, 0)
        start = log.stat().st_size
        shell.key("alt-f2")
        shell.wait(b"DOS terminal 2", start)
        shell.command("c:")
        start = log.stat().st_size
        shell.enter("threads.elf witness")
        shell.wait(b"witness waiting for input", start)
        shell.key("alt-f1")
        shell.command("threads.elf fault", expect=b"user exception 00")
        exit_code(shell, 128)
        shell.key("alt-f2")
        start = log.stat().st_size
        shell.enter("still alive")
        shell.wait(b"C:\\> ", start)
        exit_code(shell, 0)
        shell.command("a:", prompt=b"A:\\> ")
        shell.command("hello.com", prompt=b"A:\\> ", expect=b"GUEST HELLO COM")
        shell.command("c:")
        shell.command("hello.elf 7", expect=b"hello from native C")
        exit_code(shell, 0)
        shell.monitor("screendump " + json.dumps(str(work / "terminal.ppm")))
    finally:
        shell.close()
    def read_file(path):
        return subprocess.check_output(["mcopy", "-i", disk, "::" + path, "-"])
    for path, expected in [("pages.txt", b"same virtual address, distinct process pages\n"),
                           ("live.txt", b"still alive\n")]:
        checked(read_file(path) == expected, "incorrect stopped-disk bytes in " + path)
    serial = log.read_bytes()
    fault = re.search(rb"native process (\d+): user exception 00", serial)
    wakeups = [tuple(map(int, row)) for row in FUTEX.findall(serial)]
    checked(fault and any(p == int(fault[1]) and waiter == p and waker != p
                          for p, waker, waiter, _, _ in wakeups),
            "faulting worker did not wake its own blocked main thread")
    data = read_file("thread.bin")
    (proof / (name + ".bin")).write_bytes(data)
    summary = verify(data, serial, cpus, sums)
    archive = work.with_name(f"{name}-{work.name}.img")
    disk.rename(archive)
    shutil.copyfile(work / "terminal.ppm", proof / (name + ".ppm"))
    summary.update(arch=arch, firmware=firmware, cpus=cpus, executables=binaries,
                   kernel_sha256=hashlib.sha256(loaded.read_bytes()).hexdigest(),
                   disk=str(archive), disk_sha256=hashlib.sha256(archive.read_bytes()).hexdigest(),
                   qemu=subprocess.check_output(["qemu-system-" + qemu.ARCH[arch][0], "--version"],
                                                text=True).splitlines()[0],
                   compiler=subprocess.check_output([arch + "-elf-gcc", "--version"],
                                                    text=True).splitlines()[0])
    (proof / (name + ".json")).write_text(json.dumps(summary, indent=2) + "\n")
    print(name, json.dumps(summary), flush=True)


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--ram-dir", type=Path, required=True)
    cli.add_argument("--proof", type=Path, default=qemu.ROOT / "target/native-threads-proof")
    cli.add_argument("--arch", choices=["i686", "x86_64"], action="append")
    cli.add_argument("--smp", type=int, choices=[1, 4], action="append")
    cli.add_argument("--kernel", type=Path, help="test this existing kernel with one selected architecture")
    args = cli.parse_args()
    if args.kernel and len(args.arch or []) != 1:
        cli.error("--kernel requires exactly one --arch")
    args.proof.mkdir(parents=True, exist_ok=True)
    sums = [checksum(1), checksum(2)]
    for arch in args.arch or ["i686", "x86_64"]:
        kernel = args.kernel or qemu.build(arch, qemu.ROOT / "target", features="thread-trace")
        for cpus in args.smp or [1, 4]:
            with tempfile.TemporaryDirectory(dir=args.ram_dir.resolve()) as directory:
                run(arch, "bios" if arch == "i686" else "uefi", cpus, kernel, Path(directory), args.proof, sums)


if __name__ == "__main__":
    main()

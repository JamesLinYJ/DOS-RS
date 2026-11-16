#!/usr/bin/env python3
"""Exercise the native DOS interrupt on a stopped QEMU CPU and inspect its return frame."""

import argparse
import re
import socket
import subprocess
import tempfile
import time
from pathlib import Path
import qemu


class Debugger:
    def __init__(self, sock):
        self.sock = sock

    def call(self, cmd):
        body = cmd.encode()
        self.sock.sendall(b"$" + body + b"#" + f"{sum(body) % 256:02x}".encode())
        while True:
            first = self.sock.recv(1)
            if first == b"$":
                break
            if not first:
                raise RuntimeError("debugger disconnected")
        data = bytearray()
        while True:
            b = self.sock.recv(1)
            if b == b"#":
                break
            if not b:
                raise RuntimeError("QEMU exited")
            data.extend(b)
        checksum = b""
        while len(checksum) < 2:
            checksum += self.sock.recv(2 - len(checksum))
        assert int(checksum, 16) == sum(data) % 256
        self.sock.sendall(b"+")
        return data.decode()


def check(arch, mode, work, proof):
    work.mkdir(parents=True, exist_ok=True)
    kernel = qemu.build(arch, qemu.ROOT / "target", features="native-gate-test")
    proof.mkdir(parents=True, exist_ok=True)
    log = proof / "console.txt"
    disk = work / "c.img"
    with disk.open("wb") as f:
        f.truncate(4 << 20)
    subprocess.run(["mformat","-i",disk,"::"],check=True)
    seed = bytes(range(256)) + b"last record"
    subprocess.run(["mcopy","-i",disk,"-","::seed.bin"],input=seed,check=True)
    log.unlink(missing_ok=True)
    with tempfile.TemporaryDirectory() as short:
        endpoint = Path(short) / "cpu"
        cmd = qemu.boot_args(kernel, arch, work, 1, mode) + [
            "-drive", f"if=ide,index=0,format=raw,file={disk}",
            "-display",
            "none",
            "-serial",
            f"file:{log}",
            "-monitor",
            "stdio",
            "-nic",
            "none",
            "-gdb",
            f"unix:{endpoint},server=on,wait=off",
        ]
        with subprocess.Popen(
            cmd, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL
        ) as vm:
            try:
                for _ in range(1500):
                    if log.exists() and re.search(rb"Native gate test buffer ([0-9a-fA-F]+)\r?\n", log.read_bytes()):
                        break
                    if vm.poll() is not None:
                        raise RuntimeError("QEMU exited before DOS prompt")
                    time.sleep(0.02)
                else:
                    raise RuntimeError("Native gate regression did not boot")
                vm.stdin.write(b"stop\n")
                vm.stdin.flush()
                with socket.socket(socket.AF_UNIX) as sock:
                    sock.settimeout(10)
                    sock.connect(str(endpoint))
                    dbg = Debugger(sock)
                    dbg.call("?")
                    assert dbg.call("Hg1") == "OK"
                    # QEMU's GDB ABI uses full-width GPRs then 32-bit EFLAGS.
                    # Request target XML to select the actual target architecture.
                    dbg.call("qSupported:xmlRegisters=i386")
                    dbg.call("qXfer:features:read:target.xml:0,fff")
                    size = 4 if arch == "i686" else 8
                    ax, bx, cx, ip = (0, 3, 1, 8) if size == 4 else (0, 1, 2, 16)
                    original = bytearray.fromhex(dbg.call("g"))

                    def value(regs, i, width=size):
                        return int.from_bytes(
                            regs[size * i : size * i + width], "little"
                        )

                    def setreg(regs, i, n, width=size):
                        regs[size * i : size * i + width] = n.to_bytes(width, "little")

                    pc = value(original, ip)
                    flags = ip + 1
                    # Run a DOS version query in mapped instruction memory. Restore the
                    # stopped guest's original instruction bytes and every register.
                    saved = dbg.call(f"m{pc:x},2")
                    assert dbg.call(f"M{pc:x},2:cd21") == "OK"
                    assert dbg.call(f"Z1,{pc + 2:x},1") == "OK"
                    dx = 3 if size == 8 else 2
                    cases = [(0x3000, [(ax, 0x0A07), (bx, 0xFF00), (cx, 0)]),
                             (0x3001, [(ax, 0x0A07), (bx, 0), (cx, 0)]),
                             (0x3306, [(bx, 0x0A07), (dx, 0)])]
                    for query, expected, carry in [(q, e, c) for q, e in cases for c in (0, 1)]:
                        regs = original.copy()
                        for i, low in [(ax, query), (bx, 0x1234), (cx, 0x5678), (dx, 0xAAAA)]:
                            setreg(regs, i, (value(regs, i) & ~0xFFFF) | low)
                        setreg(regs, flags, (value(regs, flags, 4) & ~1) | carry, 4)
                        assert dbg.call("G" + regs.hex()) == "OK"
                        result = dbg.call("c")
                        after = bytearray.fromhex(dbg.call("g"))
                        if value(after, ip) != pc + 2:
                            raise RuntimeError(f"INT 21h failed to return: {result}")
                        for i, low in expected:
                            assert (
                                value(after, i) == (value(regs, i) & ~0xFFFF) | low
                            ), (arch, i, value(after, i))
                        assert value(after, flags, 4) & 1 == carry
                        print(
                            f"{arch}/{mode} INT 21h function={query:04X} passed CF={carry}",
                            flush=True,
                        )
                    arena = int(re.search(rb"Native gate test buffer ([0-9a-fA-F]+)", log.read_bytes())[1],16)
                    def put(addr, data):
                        assert dbg.call(f"M{addr:x},{len(data):x}:{data.hex()}") == "OK"
                    def read(addr,n):
                        return bytes.fromhex(dbg.call(f"m{addr:x},{n:x}"))
                    def invoke(function, handle=0, count=0, address=0):
                        regs = original.copy()
                        for reg, bits in [(ax,function),(bx,handle),(cx,count),(dx,address)]:
                            setreg(regs,reg,bits)
                        assert dbg.call("G" + regs.hex()) == "OK"
                        dbg.call("c")
                        after = bytearray.fromhex(dbg.call("g"))
                        assert value(after,ip) == pc+2
                        return value(after,ax) & 0xffff
                    put(arena,b"C:\\gate.bin\0")
                    h = invoke(0x3c00,address=arena)
                    assert h >= 5
                    payload = b"shared VFS gate"
                    put(arena+128,payload)
                    assert invoke(0x4000,h,len(payload),arena+128) == len(payload)
                    assert invoke(0x3e00,h) == 0
                    # Explicit C while the default drive is A. Read the second full record,
                    # then the final partial record through the same FCB and DTA.
                    fcb = bytes([3])+b"SEED    BIN"+bytes(25)
                    put(arena,fcb)
                    assert invoke(0x0f00,address=arena)&255 == 0
                    invoke(0x1a00,address=arena+128)
                    for start,status in [(0,0),(128,0),(256,3)]:
                        assert invoke(0x1400,address=arena)&255 == status
                        assert read(arena+128,128) == seed[start:start+128].ljust(128,b"\0")
                    assert invoke(0x1000,address=arena)&255 == 0
                    print(f"{arch}/{mode}: native handle write and three explicit drive C FCB reads passed",flush=True)
                    assert dbg.call(f"z1,{pc + 2:x},1") == "OK"
                    assert dbg.call(f"M{pc:x},2:{saved}") == "OK"
                    assert dbg.call("G" + original.hex()) == "OK"
            finally:
                if vm.poll() is None:
                    vm.stdin.write(b"quit\n")
                    vm.stdin.flush()
                    vm.wait(timeout=5)


    if subprocess.check_output(["mcopy","-i",disk,"::gate.bin","-"]) != b"shared VFS gate":
        raise ValueError("native INT 21h write did not reach the FAT disk")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ram-dir", type=Path)
    args = parser.parse_args()
    for mode in ["bios", "uefi"]:
        for arch in qemu.ARCH:
            with tempfile.TemporaryDirectory(dir=args.ram_dir) as scratch:
                check(arch, mode, Path(scratch), qemu.ROOT / "target/gate-proof" / f"{arch}-{mode}")

#!/usr/bin/env python3
"""Exercise the native DOS interrupt on a stopped QEMU CPU and inspect its return frame."""

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


def check(arch, mode, work):
    work.mkdir(parents=True, exist_ok=True)
    kernel = qemu.build(arch, qemu.ROOT / "target")
    log = work / "console.txt"
    log.unlink(missing_ok=True)
    with tempfile.TemporaryDirectory() as short:
        endpoint = Path(short) / "cpu"
        cmd = qemu.boot_args(kernel, arch, work, 1, mode) + [
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
                    if log.exists() and log.read_bytes().rstrip().endswith(b"A:\\>"):
                        break
                    if vm.poll() is not None:
                        raise RuntimeError("QEMU exited before DOS prompt")
                    time.sleep(0.02)
                else:
                    raise RuntimeError("DOS prompt missing")
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
                    for carry in (0, 1):
                        regs = original.copy()
                        for i, low in [(ax, 0x3000), (bx, 0x1234), (cx, 0x5678)]:
                            setreg(regs, i, (value(regs, i) & ~0xFFFF) | low)
                        setreg(regs, flags, (value(regs, flags, 4) & ~1) | carry, 4)
                        assert dbg.call("G" + regs.hex()) == "OK"
                        result = dbg.call("c")
                        after = bytearray.fromhex(dbg.call("g"))
                        if value(after, ip) != pc + 2:
                            raise RuntimeError(f"INT 21h failed to return: {result}")
                        for i, low in [(ax, 4), (bx, 0xFF00), (cx, 0)]:
                            assert (
                                value(after, i) == (value(regs, i) & ~0xFFFF) | low
                            ), (arch, i, value(after, i))
                        assert value(after, flags, 4) & 1 == carry
                        print(
                            f"{arch}/{mode} native INT 21h AX=0004 BX=FF00 CX=0000 CF={carry}",
                            flush=True,
                        )
                    assert dbg.call(f"z1,{pc + 2:x},1") == "OK"
                    assert dbg.call(f"M{pc:x},2:{saved}") == "OK"
                    assert dbg.call("G" + original.hex()) == "OK"
            finally:
                if vm.poll() is None:
                    vm.stdin.write(b"quit\n")
                    vm.stdin.flush()
                    vm.wait(timeout=5)


if __name__ == "__main__":
    for mode in ["bios", "uefi"]:
        for arch in qemu.ARCH:
            check(arch, mode, qemu.ROOT / "target/gate-proof" / f"{arch}-{mode}")

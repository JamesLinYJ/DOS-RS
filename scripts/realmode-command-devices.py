"""Exercise LOADHIGH, CTTY, CLS, PROMPT and PAUSE on a booted DOS kernel."""
import argparse
import hashlib
from pathlib import Path
import re
import runpy
import shutil
import struct
import subprocess
import tempfile
import time

from realmode_probe import build_example

common = runpy.run_path(str(Path(__file__).with_name("realmode-command.py")))
mtool = common["mtool"]
cases = [
    ("BASE", ""), ("HIGH", "LH"), ("LOW", "LH /L:0"), ("ONE", "LH /L:1"),
    ("TWO", "LH /L:2"), ("MIX", "LH /L:0;1"), ("REVERSE", "LH /L:1;0"),
    ("MIN", "LH /L:1,5120"), ("SHRINK", "LH /L:1,5120 /S"),
    ("TOOBIG", "LH /L:1,999999"),
    ("BADOPT", "LH /Z"), ("NOLIST", "LH /L:"), ("NOSIZE", "LH /L:1,"),
    ("NONUM", "LH /L:X"), ("TRAIL", "LH /L:1;"), ("NOS", "LH /S"),
    ("DUPS", "LH /L:1 /S /S"), ("DUPL", "LH /L:2 /L:1"),
    ("SIMPLE", "LOADHIGH"), ("SREV", "LH /S /L:1,5120"),
    ("NEST", "LH /L:1,5120;2,4096 /S"), ("LATER", "LH /L:2;1"),
    ("ABSENT", "LH /L:99"), ("ZERO", "LH /L:1,0"), ("BADSIZE", "LH /L:1,X"), ("RESTORE", ""),
]

def run(image, probe, placement, ram, proof, drivers, command):
    proof.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=ram) as name:
        work = Path(name)
        boot = work / "a.img"
        shutil.copyfile(image, boot)
        disk = work / "c.img"
        vol = common["image"](disk)
        def put(volume, file, data):
            mtool("mcopy", volume, "-o", "-", "::" + file, data=data)
        def bat(volume, file, lines):
            put(volume, file, ("\r\n".join(lines) + "\r\n").encode("ascii"))

        put(vol, "PROBE.EXE", probe.read_bytes())
        # The placement-only client has no nested calls or large buffers. Give its MZ
        # a 512-byte stack so a 5 KiB UMB can accommodate the executable and its stack.
        client = bytearray(placement.read_bytes())
        struct.pack_into("<HH", client, 10, 32, 32)
        struct.pack_into("<H", client, 16, 512)
        put(vol, "PLACE.EXE", client)
        cfg = ["FILES=32", "BUFFERS=15"]
        if drivers:
            original = mtool("mcopy", boot, "::COMMAND.COM", "-").stdout
            if hashlib.sha256(original).hexdigest() != "65fa71b0a34e91feffc31297fb5e24d40767c72fb513e46aaac721bda7836110":
                raise ValueError("Use the original MS-DOS 6.22 boot disk")
            for file in ("HIMEM.SYS", "EMM386.EXE"):
                put(vol, file, (drivers / file).read_bytes())
            cfg += [r"DEVICE=C:\HIMEM.SYS",
                    r"DEVICE=C:\EMM386.EXE NOEMS I=C800-CFFF X=D000-D1FF I=D200-DFFF", "DOS=HIGH,UMB"]
        else:
            cfg += ["DEVICE=EMM386.SYS", "DOS=UMB"]
        bat(boot, "CONFIG.SYS", cfg)
        entry = r"C:\TEST.BAT"
        if command:
            if drivers:
                put(vol, "SHELL.EXE", command.read_bytes())
                entry = r"C:\SHELL.EXE /C C:\TEST.BAT"
            else:
                put(boot, "COMMAND.COM", command.read_bytes())
        bat(boot, "AUTOEXEC.BAT", ["@echo off", "C:", entry])
        lines = ["@echo off", "PATH=C:\\"]
        lines.extend(["PROBE G", "PROBE M>MCB.OUT"])
        for key, opt in cases:
            prefix = "%COMSPEC% /C " if drivers and not command else ""
            lines.append((prefix + opt + " PLACE.EXE>" + key + ".OUT").strip())
        bat(vol, "KEY.IN", ["q"])
        lines += ["PAUSE <KEY.IN >PAUSE.OUT", "PROBE I1", "CTTY NUL", "PROBE I2", "ECHO HIDDEN-NULL", "CTTY CON",
                  "PROBE I3", "ECHO RETURNED-CON", "PROBE I6", "PAUSE >WAIT.OUT", "PROBE I7", "MD EXAMPLE", "CD EXAMPLE", "PROMPT MARK$Q$N$G$P$G",
                  "ECHO ON", r"C:\PROBE V4", "@ECHO OFF", "CD..", "CLS", "PROBE V5",
                  "PROBE T>IOCTL.OUT", "ECHO DONE>DONE.OUT"]
        bat(vol, "TEST.BAT", lines)
        serial = proof / "serial.log"
        serial.unlink(missing_ok=True)
        with (proof / "emulator.log").open("wb") as log, subprocess.Popen([
                "qemu-system-i386", "-machine", "pc,accel=tcg", "-cpu", "486", "-m", "16",
                "-nic", "none", "-display", "none", "-monitor", "stdio", "-serial", "file:" + str(serial),
                "-boot", "a", "-drive", f"if=floppy,format=raw,file={boot}",
                "-drive", f"if=ide,format=raw,file={disk}"], stdin=subprocess.PIPE, stdout=log, stderr=log) as vm:
            try:
                captured = set()
                resumed = False
                for attempt in range(200):
                    if mtool("mtype", vol, "::DONE.OUT", check=False).stdout.strip() == b"DONE":
                        break
                    if not resumed and mtool("mdir", vol, "::WAIT.OUT", check=False).returncode == 0:
                        time.sleep(0.25)
                        if mtool("mdir", vol, "::STATE7.OUT", check=False).returncode == 0:
                            raise AssertionError("PAUSE returned before a keyboard event")
                        vm.stdin.write(b"sendkey spc\n")
                        vm.stdin.flush()
                        resumed = True
                    for number in (4, 5):
                        if number not in captured and len(mtool("mtype", vol, f"::STATE{number}.OUT", check=False).stdout) == 2000:
                            dest = proof / f"screen{number}.ppm"
                            vm.stdin.write(f"screendump {dest}\n".encode())
                            vm.stdin.flush()
                            time.sleep(0.2)
                            vm.stdin.write(b"sendkey spc\n")
                            vm.stdin.flush()
                            captured.add(number)
                    time.sleep(0.15)
                else:
                    (proof / "files.txt").write_bytes(mtool("mdir", vol, "::").stdout)
                    vm.stdin.write(("info registers\nscreendump " + str(proof / "failure.ppm") + "\n").encode())
                    vm.stdin.flush()
                    time.sleep(0.2)
                    raise RuntimeError("Batch did not finish")
            finally:
                if vm.poll() is None:
                    vm.stdin.write(b"quit\n")
                    vm.stdin.flush()
                    vm.wait(10)
        for file in [key for key, _ in cases] + ["PAUSE", "WAIT", "MCB", "IOCTL"] + ["STATE" + str(x) for x in range(1, 8)]:
            data = mtool("mcopy", vol, "::" + file + ".OUT", "-", check=False).stdout
            (proof / (file.lower() + ".out")).write_bytes(data)


def verify(proof, two_regions):
    def read(file):
        return (proof / (file.lower() + ".out")).read_bytes()

    invalid = {"badopt", "nolist", "nosize", "nonum", "trail", "absent", "badsize"}
    if not two_regions:
        invalid |= {"two", "dupl", "later", "nest"}
    conventional = {"base", "low", "mix", "toobig", "restore"}
    reports = {}
    for key, options in cases:
        key = key.lower()
        data = read(key)
        if key in invalid:
            if data:
                raise AssertionError(f"{options} launched a program: {data!r}")
            continue
        m = re.fullmatch(rb"PSP=(\w{4}) SIZE=(\w{4}) FREE=(\w{4}) LINK=(\w{4})\r\n", data)
        if not m:
            raise AssertionError(f"{key} did not return a process report: {data!r}")
        seg, size, free, link = (int(x, 16) for x in m.groups())
        if (seg < 0xa000) != (key in conventional):
            raise AssertionError(f"{key} PSP {seg:04x} occupies the wrong region")
        if key not in ("base", "restore") and link != (key not in conventional):
            raise AssertionError(f"{key} incorrect UMB linking state {link}")
        reports[key] = (seg, size, free, link)
    if reports["base"] != reports["restore"]:
        raise AssertionError("LOADHIGH leaked memory or changed the caller's allocation policy")
    if two_regions and not (reports["one"][0] < 0xd000 <= reports["two"][0]):
        raise AssertionError("LH /L:2 selected the wrong region")
    before = sum(reports["min"][1:3])
    after = sum(reports["shrink"][1:3])
    if not (after < before and after <= 5120 // 16) or reports["shrink"] != reports["srev"]:
        raise AssertionError(f"/S did not bound the child's available memory: {before} -> {after}")
    if read("pause") != b"Press any key to continue . . .\r\n\r\n" or read("wait") != read("pause"):
        raise AssertionError("PAUSE output differs between real keyboard and redirected input")
    if read("ioctl") != b"0001 0001 0001 000D 0000 44C4 80A0\r\n":
        raise AssertionError("4401h did not reject a regular file or invalid DH")
    def handles(name):
        return [int(x,16) for x in read(name).split()]
    if (len(handles("state1")) != 3 or handles("state1") != handles("state3")
            or not all(x & 0x83 == 0x83 for x in handles("state1"))
            or not all(x & 0x87 == 0x87 for x in handles("state2"))):
        raise AssertionError("CTTY did not set/restore all standard device handles")
    prompt = (proof / "serial.log").read_bytes() + read("state4")
    if b"MARK=C>C:\\EXAMPLE>" not in prompt or b"RETURNED-CON" not in prompt or b"HIDDEN-NULL" in prompt:
        raise AssertionError("PROMPT expansion or redirected CTTY console output is incorrect")
    def pixels(i):
        header, _, body = (proof / f"screen{i}.ppm").read_bytes().partition(b"255\n")
        if not header.startswith(b"P6") or not body:
            raise AssertionError("Missing QEMU framebuffer")
        return body
    before_cls, after_cls = pixels(4), pixels(5)
    if before_cls.count(0) + 1000 >= len(before_cls) or after_cls.count(0) + 1024 < len(after_cls):
        raise AssertionError("CLS did not clear the rendered display")
    print(f"PASS {len(cases)} placement/syntax cases, CTTY, PROMPT, PAUSE, CLS; "
          f"/S reduced visible UMB from {before*16} to {after*16} bytes")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("image", "ram-dir", "proof"):
        parser.add_argument("--" + name, required=True, type=Path)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--toolchain", type=Path, help="Build both Rust verification clients with rust16")
    source.add_argument("--probe", type=Path, help="Prebuilt command_state.exe")
    parser.add_argument("--placement", type=Path, help="Compiled umb_placement.exe (defaults to the probe directory)")
    parser.add_argument("--drivers", type=Path, help="HIMEM.SYS and EMM386.EXE for the optional MS-DOS 6.22 reference run")
    parser.add_argument("--command", type=Path)
    args = parser.parse_args()
    if args.toolchain:
        probe = build_example(args.toolchain, "command_state")
        placement = build_example(args.toolchain, "umb_placement")
    else:
        probe, placement = args.probe, args.placement or args.probe.with_name("umb_placement.exe")
    run(args.image, probe, placement, args.ram_dir, args.proof.resolve(), args.drivers, args.command)
    verify(args.proof, bool(args.drivers))

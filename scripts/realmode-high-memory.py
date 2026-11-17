"""Install native DOS on a RAM disk and check real high physical bytes from QEMU."""
import argparse
import importlib.util
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("installer", ROOT / "scripts/realmode-install-test.py")
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


def physical_bytes(session, directory, addr, name):
    dump = directory / (name + ".bin")
    session.monitor(f'pmemsave {addr:#x} 4 "{dump}"')
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if dump.is_file() and dump.stat().st_size == 4:
            return dump.read_bytes()
        time.sleep(0.05)
    raise RuntimeError(f"QEMU could not capture physical address {addr:#x}")


def dosbox_fallback(directory, image, proof, emulator):
    boot = directory / "fallback.img"
    shutil.copyfile(image, boot)
    subprocess.run(["mcopy", "-o", "-i", boot, "-", "::AUTOEXEC.BAT"],
                   input=b"MEMSTAT /HIGH\r\n", check=True)
    for cpu in (286, 386):
        log = (proof / f"dosbox-{cpu}.txt").resolve()
        log.unlink(missing_ok=True)
        conf = directory / "fallback.conf"
        conf.write_text("[sdl]\noutput=surface\n[dosbox]\nshow start banner=false\nquit warning=false\n"
                        f"[cpu]\ncore=normal\ncputype={cpu}\ncycles=fixed 30000\n[mixer]\nnosound=true\n"
                        f"[dos]\nems=false\nxms=false\n[serial]\nserial1=file file:{log} timeout:0\n"
                        "[autoexec]\nboot fallback.img\n")
        env = dict(os.environ, TMPDIR=str(directory), SDL_VIDEODRIVER="dummy", SDL_AUDIODRIVER="dummy")
        with subprocess.Popen([emulator, "-conf", conf, "-nogui", "-nomenu"], cwd=directory,
                              env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) as guest:
            try:
                until = time.monotonic() + 90
                while time.monotonic() < until and guest.poll() is None:
                    data = log.read_bytes() if log.is_file() else b""
                    if b"A:\\> " in data:
                        break
                    time.sleep(0.1)
                else:
                    raise RuntimeError(f"DOSBox-X did not boot its {cpu}, see {log}")
                if f"DOS / 80{cpu}".encode() not in data or b"No high physical RAM available" not in data:
                    raise RuntimeError(f"incorrect DOSBox-X {cpu} fallback in {log}")
            finally:
                if guest.poll() is None:
                    guest.terminate()
                    try:
                        guest.wait(3)
                    except subprocess.TimeoutExpired:
                        guest.kill()
        print("PASS DOSBox-X", cpu, flush=True)


def cold_boot(directory, disk, proof, arch, legacy=False, boundary_qemu=None, firmware=None):
    tag = arch + ("-bit47" if boundary_qemu else "-legacy" if legacy else "-pae")
    serial = proof / (tag + ".txt")
    serial.unlink(missing_ok=True)
    cpu = "max,vendor=GenuineIntel,phys-bits=48" if boundary_qemu else "486" if legacy else "max"
    command = [str(boundary_qemu or ("qemu-system-" + arch)), "-machine", "pc,max-ram-below-4g=64M",
               "-accel", "tcg", "-cpu", cpu,
               "-m", "16M" if legacy else "96M", "-no-reboot", "-nic", "none",
               "-display", "none", "-monitor", "stdio", "-serial", "file:" + str(serial),
               "-drive", "if=ide,format=raw,file=" + str(disk), "-boot", "c"]
    if firmware:
        command.extend(["-L", str(firmware)])
    with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL) as proc:
        session = installer.Session(proc, serial, proof)
        try:
            session.expect("C:\\> ")
            if not legacy:
                base = 1 << (47 if boundary_qemu else 32)
                low_before = physical_bytes(session, directory, 0x1ffc, tag + "-alias-before")
                before = physical_bytes(session, directory, base + 0x1ffc, tag + "-before")
            session.type("memstat /high\n")
            session.expect("No high physical RAM available" if legacy else
                           "PASS: 16-bit high page transfer, isolation, bounds and stale handles")
            session.expect("C:\\> ")
            if not legacy:
                page = int(re.search(rb"Second physical page 0x([0-9A-F]{12})", serial.read_bytes())[1], 16)
                if page != base + 4096:
                    raise RuntimeError(f"DOS allocated its second page at unexpected {page:#x}")
                after = physical_bytes(session, directory, page + 4092, tag + "-after")
                low_after = physical_bytes(session, directory, 0x1ffc, tag + "-alias-after")
                if before == after or after != bytes.fromhex("48 69 a5 3c") or low_before != low_after:
                    raise RuntimeError("QEMU physical RAM disagrees with DOS readback")
                (proof / (tag + "-physical.txt")).write_text(
                    f"QEMU pmemsave {page + 4092:#014x}: {before.hex()} -> {after.hex()}\n"
                    f"QEMU pmemsave 0x000000001ffc: {low_before.hex()} -> {low_after.hex()}\n")
            session.type("memstat /test\n")
            session.expect("PASS: page reuse, VCPI ownership and pool balance")
            session.expect("C:\\> ")
        finally:
            if proc.poll() is None:
                session.monitor("quit")
                proc.wait(10)
    print("PASS", tag, flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", type=Path, required=True, help="full native installer floppy")
    parser.add_argument("--ram-dir", type=Path, required=True)
    parser.add_argument("--proof", type=Path, required=True)
    parser.add_argument("--bit47-qemu", type=Path, help="optional QEMU with its ordinary high RAM relocated to 1<<47")
    parser.add_argument("--firmware-dir", type=Path, help="firmware for the optional high-boundary machine")
    parser.add_argument("--dosbox-x", type=Path, help="optional DOSBox-X binary for actual 286/386 CPU models")
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=args.ram_dir) as tmp:
        directory = Path(tmp)
        disk = installer.disk_file(directory, "native-c.img", 8)
        with installer.vm(args.image.resolve(), [disk], args.proof, "install") as session:
            session.wizard("setup", 1, 1)
            session.key("ret")
            session.expect("Installation complete")
        for name in ("EMM386.SYS", "MEMSTAT.EXE"):
            packaged = subprocess.check_output(["mcopy", "-i", args.image, "::" + name, "-"])
            installed = subprocess.check_output(["mcopy", "-i", str(disk) + "@@1048576", "::" + name, "-"])
            if installed != packaged:
                raise RuntimeError(f"SETUP installed different bytes for {name}")
        for arch in ["i386", "x86_64"]:
            cold_boot(directory, disk, args.proof, arch, False)
        cold_boot(directory, disk, args.proof, "i386", True)
        if args.bit47_qemu:
            cold_boot(directory, disk, args.proof, "x86_64", boundary_qemu=args.bit47_qemu.resolve(),
                      firmware=args.firmware_dir)
        if args.dosbox_x:
            dosbox_fallback(directory, args.image.resolve(), args.proof, args.dosbox_x.resolve())


if __name__ == "__main__":
    main()

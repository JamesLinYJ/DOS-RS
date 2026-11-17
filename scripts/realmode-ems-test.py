"""Cold boot the native install media and run Borland, Jemm and DOS-RS memory clients on RAM disks."""
import argparse
import hashlib
import importlib.util
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile
import time

HERE = Path(__file__).resolve().parent
CLIENT_SHA256 = "eb811fd7c038e34bcb7699c10c629ed30c5d7cb15ddf0bb5b7fbf5ad734dea73"


def read_dos(image, name):
    return subprocess.check_output(["mcopy", "-i", image, "::" + name, "-"])


def check_pages(log):
    # EMSTEST overwrites the on-screen counter with backspaces. Verify the last
    # logical page as well as the client's own success marker for both loops.
    pages = None
    for operation in (b"Initializing", b"Checking"):
        row = re.search(operation + rb" (\d+) pages:([^\r\n]+)", log)
        if not row:
            raise ValueError(f"missing {operation!r} pass")
        n = int(row[1])
        if n < 2 or (pages is not None and n != pages):
            raise ValueError("inconsistent EMSTEST page counts")
        if not re.search(str(n - 1).encode() + rb"\x08+OK\.$", row[2]):
            raise ValueError(f"incomplete {operation!r} pass")
        pages = n
    for marker in (b"Window Segment Address: D000", b"Restoring page map: OK.",
                   f"De-allocating {pages} pages EMS memory: OK.".encode()):
        if marker not in log:
            raise ValueError(f"missing {marker!r}")
    if b"corrupted" in log.lower() or b"Program halted" in log:
        raise ValueError("EMSTEST reported a failure")


def check_display(picture):
    # A blinking cursor on an otherwise empty text page also yields a valid PPM.
    magic, size, maximum, pixels = picture.read_bytes().split(b"\n", 3)
    width, height = map(int, size.split())
    if (magic, maximum) != (b"P6", b"255") or len(pixels) != width * height * 3:
        raise ValueError("incomplete QEMU screen capture")
    background = pixels[-3:]
    ink = sum(pixels[i:i+3] != background for i in range(0, len(pixels), 3))
    if ink < 1024:
        raise ValueError("the EMS result appeared on serial but the display is empty")


def check_import(session):
    session.type("memstat /gemmis\n")
    session.expect("GEMMIS 1.0 physical address ")
    session.expect("C:\\> ")
    match = re.search(rb"GEMMIS 1\.0 physical address (\d+)\r\n", session.log.read_bytes())
    if not match:
        raise ValueError("missing DOS device IOCTL result")
    physical = int(match[1])
    def physical_bytes(address, size, name):
        dump = session.proof / name
        dump.unlink(missing_ok=True)
        session.monitor(f'pmemsave {address:#x} {size} "{dump}"')
        until = time.monotonic() + 5
        while (not dump.is_file() or dump.stat().st_size != size) and time.monotonic() < until:
            time.sleep(0.05)
        return dump.read_bytes()

    # MEMSTAT snapshots its two logical pages while all free XMS is held.
    size = 0x18d + 32
    data = physical_bytes(physical, size, "ems-import.bin")
    if struct.unpack_from("<HH", data, 2) != (size, 1) or data[0x18c] != 2:
        raise ValueError("invalid physical EMM import header or handle count")
    for slot in range(4):
        start = 10 + 6 * (0xd0000 // 16384 + slot)
        if data[start] != 3 or data[start + 4] != slot:
            raise ValueError("incorrect EMS frame in the native import table")
    pages, page_list = struct.unpack_from("<HI", data, 0x18d + 16 + 10)
    if pages != 2 or not page_list:
        raise ValueError("missing active EMS pages in snapshot")
    entries = struct.unpack("<8I", physical_bytes(page_list, 32, "ems-import-pages.bin"))
    for page in range(2):
        first = entries[page * 4] & ~0xfff
        for sub in range(4):
            if entries[page * 4 + sub] != (first + 4096 * sub) | 7:
                raise ValueError("invalid EMS physical page-table entries")
        if physical_bytes(first, 1, f"ems-import-data{page}.bin") != bytes([0x61 + page]):
            raise ValueError("imported EMS page does not contain the client's data")


# Unmodified clients from Jemm Test/, Tools/XMSSTAT and Tools/VCPI.
CASES = (
    ("EMS4E", "", (b"size to save all pages: 16", b"Ok")),
    ("EMS56", "", (b"ax=5600h ok",)),
    ("EMS57", "", (b"move to expanded memory ok", b"move from expanded memory ok")),
    ("XMSSTAT", "", (b"XMS version: 3.0", b"XMS handle table at", b"XMS handle array at",
                     b"segment of largest UMB:", b"size of largest UMB")),
    ("XMSTEST", "", (b"src+len < dest, ok", b"src+len > dest, ok", b"src > dest+len, ok", b"src < dest+len, ok")),
    ("VCPI", " -q", (b"VCPI version: 1.0", b"free pages in pm, status ah=00")),
    ("TESTVCPI", "", (b"hello from pm",)),
    ("MEMSTAT", " /TEST", (b"XMS handle table allocation, lock, resize and release verified",
                          b"XMS + EMS + UMB data verified", b"DOS high allocation, resize and free verified",
                          b"PASS: page reuse, VCPI ownership and pool balance")),
)


def add_clients(args, image):
    for name, _, _ in CASES:
        if name != "MEMSTAT":
            subprocess.run(["mcopy", "-o", "-i", image, args.clients / (name + ".EXE"),
                            "::" + name + ".EXE"], check=True)


def check_clients(log):
    for name, _, markers in CASES:
        start, end = ("BEGIN-" + name + "\r\n").encode(), ("END-" + name + "\r\n").encode()
        if start not in log or end not in log:
            raise ValueError(f"{name} did not return to DOS")
        output = log.split(start, 1)[1].split(end, 1)[0]
        for marker in markers:
            if marker not in output:
                raise ValueError(f"{name} missing {marker!r} in {output!r}")
        if name == "EMS56":
            stack = re.search(rb"SP=([0-9a-f]+).*?SP after call=([0-9a-f]+)", output, re.S)
            if not stack or stack[1] != stack[2]:
                raise ValueError("EMS mapped call returned on a different stack")
        if name == "XMSSTAT" and not re.search(rb"paragraphs\): [1-9a-f][0-9a-f]*", output):
            raise ValueError("no UMB reported")


def dosbox(args, work, cpu, enabled, jemm=False):
    boot = work / "boot.img"
    shutil.copyfile(args.disk1, boot)
    clients = work / "clients.img"
    subprocess.run(["mformat", "-C", "-f", "1440", "-i", clients, "::"], check=True)
    commands = ["@ECHO OFF"]
    # Only Borland reads the keyboard. Its scheduled Escape keys must not
    # reach the shell or another client in the unattended Jemm run.
    if jemm:
        add_clients(args, clients)
        for name, tail, _ in CASES:
            drive = "A:" if name == "MEMSTAT" else "B:"
            commands.extend(["ECHO BEGIN-" + name, drive + "\\" + name + ".EXE" + tail, "ECHO END-" + name])
    else:
        subprocess.run(["mcopy", "-i", clients, args.client, "::EMSTEST.COM"], check=True)
        commands.append(r"B:\EMSTEST.COM")
    subprocess.run(["mcopy", "-o", "-i", boot, "-", "::AUTOEXEC.BAT"],
                   input=("\r\n".join([*commands, "ECHO EMS-BATCH-END", ""])).encode(), check=True)
    if not enabled:
        subprocess.run(["mdel", "-i", boot, "::CONFIG.SYS"], check=True)
    tag = f"dosbox-{cpu}"
    if jemm:
        tag += "-jemm"
    elif not enabled:
        tag += "-without-device"
    log = args.proof / (tag + ".txt")
    log.unlink(missing_ok=True)
    (work / "machine.conf").write_text(
        "[sdl]\noutput=surface\n[dosbox]\nshow start banner=false\nquit warning=false\n"
        f"[cpu]\ncore=normal\ncputype={cpu}\ncycles=fixed 30000\n"
        "[mixer]\nnosound=true\n[dos]\nems=false\nxms=false\n"
        f"[serial]\nserial1=file file:{log} timeout:0\n[autoexec]\n"
        + ("autotype -w 14 -p 8 space esc esc esc\n" if cpu == 386 and enabled and not jemm else "")
        + "imgmount 1 clients.img -t floppy -fs none\nboot boot.img\n"
    )
    env = dict(os.environ, TMPDIR=str(work), SDL_VIDEODRIVER="dummy", SDL_AUDIODRIVER="dummy")
    with subprocess.Popen([args.dosbox_x, "-conf", "machine.conf", "-nogui", "-nomenu"],
                          cwd=work, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) as guest:
        try:
            started = last_output = time.monotonic()
            length = 0
            while time.monotonic() - started < 300 and time.monotonic() - last_output < 90 and guest.poll() is None:
                data = log.read_bytes() if log.exists() else b""
                if len(data) != length:
                    length, last_output = len(data), time.monotonic()
                if b"\nEMS-BATCH-END\r\nA:\\> " in data:
                    break
                time.sleep(0.1)
            else:
                elapsed = round(time.monotonic() - started)
                idle = round(time.monotonic() - last_output)
                raise RuntimeError(f"{tag} did not finish in {elapsed}s ({idle}s since serial output), see {log}")
        finally:
            if guest.poll() is None:
                guest.terminate()
                try:
                    guest.wait(4)
                except subprocess.TimeoutExpired:
                    guest.kill()
    if f"DOS / 80{cpu}".encode() not in data:
        raise ValueError("wrong native kernel")
    if jemm:
        check_clients(data)
    elif cpu == 386 and enabled:
        check_pages(data)
    elif b"Expanded Memory Manager Interrupt (INT 67H) not installed." not in data:
        raise ValueError("EMSTEST failed to demonstrate absence of the driver")
    print(f"PASS {tag}", flush=True)


def installed(args, work):
    spec = importlib.util.spec_from_file_location("installer", HERE / "realmode-install-test.py")
    wizard = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(wizard)
    disk = wizard.disk_file(work, "installed.img", 16)
    with wizard.vm(args.image, [disk], args.proof, "qemu-setup") as session:
        session.wizard("setup", 1, 1)
        session.key("ret")
        session.expect("Installation complete")
    partition = str(disk) + "@@1048576"
    for name in ("EMM386.SYS", "MEMSTAT.EXE"):
        if read_dos(partition, name) != read_dos(args.image, name):
            raise ValueError(f"installed {name} differs from the installation floppy")
    add_clients(args, partition)
    if b"DEVICE=EMM386.SYS" not in read_dos(partition, "CONFIG.SYS").splitlines():
        raise ValueError("installed CONFIG.SYS does not load the driver")
    subprocess.run(["mcopy", "-i", partition, args.client, "::EMSTEST.COM"], check=True)
    with wizard.vm(None, [disk], args.proof, "qemu-hard-boot") as session:
        session.type("emstest.com\n")
        session.expect("Press Esc")
        session.key("spc")
        session.expect("pages EMS memory: OK.")
        session.expect("Press Esc")
        session.picture("ems-pages")
        session.key("esc")
        session.expect("C:\\> ")
        for name, tail, markers in CASES:
            session.type((name + ".exe" + tail + "\n").lower())
            for marker in markers:
                session.expect(marker.decode())
            session.expect("C:\\> ")
        session.picture("memory-services")
        check_import(session)
        session.type("vcpi -a 1\n")
        session.expect("alloced 1 page(s) in pm, last status ah=00")
        session.expect("C:\\> ")
        allocation = re.findall(rb"\(first\) page=([a-fA-F0-9]{8})", session.log.read_bytes())
        if not allocation:
            raise ValueError("VCPI client did not report the physical page it allocated")
        session.type("vcpi -f " + allocation[-1].decode().lower() + "\n")
        session.expect("freed 1 page(s) in pm, last status ah=00")
        session.expect("C:\\> ")
    check_pages((args.proof / "qemu-hard-boot.txt").read_bytes())
    check_display(args.proof / "memory-services.ppm")
    check_display(args.proof / "ems-pages.ppm")
    print("PASS QEMU installed C: cold boot, EMS, XMS, UMB and protected-mode VCPI", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("image", "disk1", "client", "ram-dir", "proof"):
        parser.add_argument("--" + name, type=Path, required=True)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--clients", type=Path, help="directory of prebuilt Jemm test executables")
    source.add_argument("--jemm", type=Path, help="build the clients from an upstream Jemm checkout")
    parser.add_argument("--jwasm", default="jwasm", help="host JWasm executable")
    parser.add_argument("--dosbox-x", default="dosbox-x")
    args = parser.parse_args()
    args.proof = args.proof.resolve()
    args.proof.mkdir(parents=True, exist_ok=True)
    if hashlib.sha256(args.client.read_bytes()).hexdigest() != CLIENT_SHA256:
        parser.error("expected Borland C++ EMSTEST.COM 2.0 (1991), SHA-256 " + CLIENT_SHA256)
    with tempfile.TemporaryDirectory(dir=args.ram_dir) as tmp:
        work = Path(tmp).resolve()
        if args.jemm:
            args.clients = work / "clients"
            args.clients.mkdir()
            assembler = shutil.which(args.jwasm)
            if not assembler:
                parser.error("cannot find host JWasm")
            for name, _, _ in CASES:
                if name == "MEMSTAT":
                    continue
                directory = ("Tools/" + name) if name in ("VCPI", "XMSSTAT") else "Test"
                subprocess.run([str(Path(assembler).resolve()), "-mz", "-nologo",
                                "-Fo" + str(args.clients / (name + ".EXE")), name + ".ASM"],
                               cwd=args.jemm / directory, check=True)
        for cpu, enabled in ((386, True), (286, True), (386, False)):
            dosbox(args, work, cpu, enabled)
        dosbox(args, work, 386, True, jemm=True)
        installed(args, work)


if __name__ == "__main__":
    main()

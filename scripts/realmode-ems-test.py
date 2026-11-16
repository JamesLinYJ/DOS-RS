"""Cold boot the native install media and exercise Borland EMSTEST 2.0 on RAM disks."""
import argparse
import hashlib
import importlib.util
import os
from pathlib import Path
import re
import shutil
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
    for operation in (b"Initializing", b"Checking"):
        row = re.search(operation + rb" 128 pages:[^\r\n]+", log)
        if not row or not row[0].endswith(b"127\b\b\bOK."):
            raise ValueError(f"missing complete {operation!r} pass")
    for marker in (b"Window Segment Address: D000", b"Restoring page map: OK.",
                   b"De-allocating 128 pages EMS memory: OK."):
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


def dosbox(args, work, cpu, enabled):
    boot = work / "boot.img"
    shutil.copyfile(args.disk1, boot)
    subprocess.run(["mcopy", "-o", "-i", boot, args.client, "::EMSTEST.COM"], check=True)
    subprocess.run(["mcopy", "-o", "-i", boot, "-", "::AUTOEXEC.BAT"],
                   input=b"EMSTEST.COM\r\n", check=True)
    if not enabled:
        subprocess.run(["mdel", "-i", boot, "::CONFIG.SYS"], check=True)
    tag = f"dosbox-{cpu}" + ("" if enabled else "-without-device")
    log = args.proof / (tag + ".txt")
    log.unlink(missing_ok=True)
    (work / "machine.conf").write_text(
        "[sdl]\noutput=surface\n[dosbox]\nshow start banner=false\nquit warning=false\n"
        f"[cpu]\ncore=normal\ncputype={cpu}\ncycles=fixed 30000\n"
        "[mixer]\nnosound=true\n[dos]\nems=false\nxms=false\n"
        f"[serial]\nserial1=file file:{log} timeout:0\n[autoexec]\n"
        + ("autotype -w 12 -p 6 space esc\n" if cpu == 386 and enabled else "")
        + "boot boot.img\n"
    )
    env = dict(os.environ, TMPDIR=str(work), SDL_VIDEODRIVER="dummy", SDL_AUDIODRIVER="dummy")
    with subprocess.Popen([args.dosbox_x, "-conf", "machine.conf", "-nogui", "-nomenu"],
                          cwd=work, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) as guest:
        try:
            until = time.monotonic() + 90
            while time.monotonic() < until and guest.poll() is None:
                data = log.read_bytes() if log.exists() else b""
                if b"A:\\> " in data:
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError(f"{tag} failed to return to the prompt, see {log}")
        finally:
            if guest.poll() is None:
                guest.terminate()
                try:
                    guest.wait(4)
                except subprocess.TimeoutExpired:
                    guest.kill()
    if f"DOS / 80{cpu}".encode() not in data:
        raise ValueError("wrong native kernel")
    if cpu == 386 and enabled:
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
    if read_dos(partition, "EMM386.SYS") != read_dos(args.image, "EMM386.SYS"):
        raise ValueError("installed driver bytes differ from the installation floppy")
    if b"DEVICE=EMM386.SYS" not in read_dos(partition, "CONFIG.SYS").splitlines():
        raise ValueError("installed CONFIG.SYS does not load the driver")
    subprocess.run(["mcopy", "-i", partition, args.client, "::EMSTEST.COM"], check=True)
    with wizard.vm(None, [disk], args.proof, "qemu-hard-boot") as session:
        session.type("emstest.com\n")
        session.expect("Press Esc")
        session.key("spc")
        session.expect("De-allocating 128 pages EMS memory: OK.")
        session.expect("Press Esc")
        session.picture("ems-pages")
        session.key("esc")
        session.expect("C:\\> ")
    check_pages((args.proof / "qemu-hard-boot.txt").read_bytes())
    check_display(args.proof / "ems-pages.ppm")
    print("PASS QEMU installed C: cold boot and EMS page readback", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("image", "disk1", "client", "ram-dir", "proof"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--dosbox-x", default="dosbox-x")
    args = parser.parse_args()
    args.proof = args.proof.resolve()
    args.proof.mkdir(parents=True, exist_ok=True)
    if hashlib.sha256(args.client.read_bytes()).hexdigest() != CLIENT_SHA256:
        parser.error("expected Borland C++ EMSTEST.COM 2.0 (1991), SHA-256 " + CLIENT_SHA256)
    with tempfile.TemporaryDirectory(dir=args.ram_dir) as tmp:
        work = Path(tmp).resolve()
        for cpu, enabled in ((386, True), (286, True), (386, False)):
            dosbox(args, work, cpu, enabled)
        installed(args, work)


if __name__ == "__main__":
    main()

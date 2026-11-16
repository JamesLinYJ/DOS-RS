"""Measure the native floppy from emulator launch to its first interactive prompt."""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time

def measure(args, work, run):
    boot = work / "boot.img"
    shutil.copyfile(args.image, boot)
    welcome = None
    if args.welcome:
        welcome = subprocess.check_output(["mtype", "-i", boot, "::WELCOME.TXT"])
        subprocess.run(["mcopy", "-o", "-i", boot, "-", "::AUTOEXEC.BAT"],
                       input=b"VER\r\nTYPE WELCOME.TXT\r\n", check=True)
    env = dict(os.environ, TMPDIR=str(work), SDL_AUDIODRIVER="dummy")
    if not args.window:
        env["SDL_VIDEODRIVER"] = "dummy"
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as uart:
        uart.bind(("127.0.0.1", 0))
        uart.listen(1)
        uart.settimeout(0.1)
        port = uart.getsockname()[1]
        if args.cpu == "qemu":
            command = ["qemu-system-i386", "-m", "16", "-boot", "a", "-display", "none",
                       "-nic", "none", "-monitor", "none", "-serial", f"tcp:127.0.0.1:{port}",
                       "-drive", "if=floppy,format=raw,readonly=on,file=boot.img"]
        else:
            (work / "guest.conf").write_text(
                "[sdl]\noutput=surface\n[dosbox]\nshow start banner=false\nquit warning=false\n"
                f"[cpu]\ncore=normal\ncputype={args.cpu}\ncycles=fixed 30000\n"
                "[mixer]\nnosound=true\n[serial]\n"
                f"serial1=nullmodem server:127.0.0.1 port:{port} transparent:1\n"
                "[autoexec]\nboot boot.img\n")
            command = [args.dosbox_x, "-conf", "guest.conf", "-nogui", "-nomenu"]
        prefix = args.proof / f"{args.cpu}-{run}"
        with prefix.with_suffix(".trace").open("wb") as trace:
            start = time.monotonic()
            guest = subprocess.Popen(command, cwd=work, env=env, stdout=trace, stderr=trace)
            data = bytearray()
            times = {}
            try:
                while guest.poll() is None and time.monotonic() - start < 60:
                    try:
                        peer, _ = uart.accept()
                        break
                    except socket.timeout:
                        continue
                else:
                    raise RuntimeError("guest did not connect its serial port")
                with peer:
                    peer.settimeout(0.1)
                    while guest.poll() is None and time.monotonic() - start < 120:
                        try:
                            chunk = peer.recv(4096)
                        except socket.timeout:
                            continue
                        if not chunk:
                            break
                        data.extend(chunk)
                        elapsed = round(time.monotonic() - start, 3)
                        if b"DOS / 80" in data:
                            times.setdefault("banner_seconds", elapsed)
                        if b"A:\\> " in data:
                            times["prompt_seconds"] = elapsed
                            break
            finally:
                if guest.poll() is None:
                    guest.terminate()
                    try:
                        guest.wait(4)
                    except subprocess.TimeoutExpired:
                        guest.kill()
                        guest.wait()
                prefix.with_suffix(".serial").write_bytes(data)
            cpu = "386" if args.cpu == "qemu" else args.cpu
            if f"DOS / 80{cpu}".encode() not in data or "prompt_seconds" not in times:
                raise RuntimeError(f"missing native banner or prompt; see {prefix}.serial")
            if welcome is not None and (not welcome or welcome not in data):
                raise RuntimeError(f"missing welcome text; see {prefix}.serial")
            record = dict(cpu=args.cpu, welcome=args.welcome, **times)
            prefix.with_suffix(".json").write_text(json.dumps(record) + "\n")
            print(record, flush=True)


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--image", required=True, type=Path)
    cli.add_argument("--ram-dir", required=True, type=Path)
    cli.add_argument("--proof", required=True, type=Path)
    cli.add_argument("--cpu", choices=["286", "386", "qemu"], default="286")
    cli.add_argument("--dosbox-x", default="dosbox-x")
    cli.add_argument("--window", action="store_true")
    cli.add_argument("--welcome", action="store_true", help="run VER and TYPE WELCOME.TXT on a copy")
    cli.add_argument("--repeat", type=int, default=2)
    args = cli.parse_args()
    if args.repeat < 1:
        cli.error("--repeat must be positive")
    args.proof.mkdir(parents=True, exist_ok=True)
    for run in range(1, args.repeat + 1):
        with tempfile.TemporaryDirectory(dir=args.ram_dir) as scratch:
            measure(args, Path(scratch), run)

if __name__ == "__main__":
    main()

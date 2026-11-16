"""Run native Edit on old CPU and display models and read the saved bytes from disk."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def run(*args):
    subprocess.run(args, check=True)


def main():
    c = argparse.ArgumentParser(description=__doc__)
    for option in ("image", "ram-dir", "proof"):
        c.add_argument("--" + option, type=Path, required=True)
    c.add_argument("--dosbox-x", default="dosbox-x")
    a = c.parse_args()
    proof = a.proof.resolve()
    proof.mkdir(exist_ok=True, parents=True)
    with tempfile.TemporaryDirectory(dir=a.ram_dir) as tmp:
        d = Path(tmp)
        boot = d / "boot.img"
        shutil.copyfile(a.image, boot)
        (d / "AUTOEXEC.BAT").write_bytes(b"C:\r\nA:\\EDIT.EXE DOC.TXT\r\n")
        run("mcopy", "-o", "-i", boot, d / "AUTOEXEC.BAT", "::AUTOEXEC.BAT")
        disk = d / "drive.img"
        with disk.open("wb") as f:
            f.truncate(80 * 16 * 63 * 512)
        run(
            "mformat",
            "-i",
            disk,
            "-F",
            "-c",
            "1",
            "-t",
            "80",
            "-h",
            "16",
            "-n",
            "63",
            "::",
        )
        for cpu, machine in [
            ("286", "ega"),
            ("386", "cga"),
            ("286", "hercules"),
            ("386", "svga_s3"),
        ]:
            log = proof / (cpu + "-" + machine + ".txt")
            log.unlink(missing_ok=True)
            (d / "DOC.TXT").write_bytes(b"Native Edit")
            run("mcopy", "-o", "-i", disk, d / "DOC.TXT", "::DOC.TXT")
            conf = d / "machine.conf"
            conf.write_text(
                f"[sdl]\noutput=surface\n[dosbox]\nmachine={machine}\nshow start banner=false\nquit warning=false\n[cpu]\ncore=normal\ncputype={cpu}\ncycles=fixed 50000\n[mixer]\nnosound=true\n[serial]\nserial1=file file:{log} timeout:0\n[autoexec]\nmount d .\nimgmount 2 d:\\drive.img -t hdd -fs none -size 512,63,16,80\nautotype -w 5 -p 0.35 end x f2 d o c period t x t enter\nboot boot.img\n"
            )
            with (proof / (cpu + "-" + machine + "-host.txt")).open("wb") as trace:
                p = subprocess.Popen(
                    [a.dosbox_x, "-conf", conf, "-nogui", "-nomenu"],
                    cwd=d,
                    stdout=trace,
                    stderr=trace,
                    env=dict(
                        os.environ, SDL_VIDEODRIVER="dummy", SDL_AUDIODRIVER="dummy"
                    ),
                )
                try:
                    end = time.monotonic() + 80
                    while time.monotonic() < end:
                        if log.exists() and b"Saved" in log.read_bytes():
                            break
                        if p.poll() is not None:
                            break
                        time.sleep(0.1)
                finally:
                    if p.poll() is None:
                        p.terminate()
                        try:
                            p.wait(4)
                        except subprocess.TimeoutExpired:
                            p.kill()
                            p.wait()
            transcript = log.read_bytes()
            assert (
                ("DOS / 80" + cpu).encode() in transcript
                and b"C: FAT32" in transcript
                and b"Saved" in transcript
            ), transcript[-600:]
            assert (
                subprocess.check_output(["mcopy", "-i", disk, "::DOC.TXT", "-"])
                == b"Native Editx"
            )
            print(cpu, machine, "opened, edited and saved", flush=True)


if __name__ == "__main__":
    main()

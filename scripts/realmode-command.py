#!/usr/bin/env python3
"""Run the same built-in command workload against a supplied BIOS DOS and a private FAT16 disk."""

import argparse
import hashlib
import re
from pathlib import Path
import os
import shutil
import struct
import subprocess
import tempfile
import time

LABEL = "CMDTEST"
PAYLOAD = bytes(range(256)) * 313
# Each documented name/alias is in COMMAND.COM's own help or is probed on the reference disk.
COMMANDS = (
    "BREAK CALL CD CHDIR CHCP CLS COPY CTTY DATE DEL ERASE DIR ECHO EXIT FOR GOTO IF "
    "LH LOADHIGH MD MKDIR PATH PAUSE PROMPT RD RMDIR REM REN RENAME SET SHIFT "
    "TIME TYPE VER VERIFY VOL TRUENAME"
).split()


def mtool(name, disk, *args, data=None, check=True):
    return subprocess.run(
        [name, "-i", str(disk), *args], input=data, capture_output=True, check=check
    )


def mkfs():
    return shutil.which("mkfs.fat") or "/opt/homebrew/opt/dosfstools/sbin/mkfs.fat"


def image(path):
    sectors = 32 * 16 * 63
    start = 63
    with path.open("wb") as f:
        f.truncate(sectors * 512)
        mbr = bytearray(512)
        mbr[446:462] = struct.pack(
            "<BBBBBBBBII", 0, 1, 1, 0, 6, 15, 63, 31, start, sectors - start
        )
        mbr[510:] = b"\x55\xaa"
        f.write(mbr)
    subprocess.run(
        [
            mkfs(),
            "-I",
            "-F",
            "16",
            "-s",
            "2",
            "-g",
            "16/63",
            "-h",
            str(start),
            "--offset",
            str(start),
            "-n",
            LABEL,
            "-i",
            "9B6A231F",
            path,
        ],
        check=True,
    )
    return str(path) + "@@" + str(512 * start)


def run(floppy, ram, proof, reference, command=None, cpu="386", dosbox=None):
    proof.mkdir(exist_ok=True, parents=True)
    if reference or command:
        original = mtool("mcopy", floppy, "::COMMAND.COM", "-").stdout
        if (
            hashlib.sha256(original).hexdigest()
            != "65fa71b0a34e91feffc31297fb5e24d40767c72fb513e46aaac721bda7836110"
        ):
            raise ValueError(
                "expected the original English COMMAND.COM from MS-DOS 6.22 disk 1"
            )
    with tempfile.TemporaryDirectory(dir=ram) as folder:
        work = Path(folder)
        boot = work / "boot.img"
        shutil.copyfile(floppy, boot)
        volume = image(work / "target.img")
        mtool(
            "mcopy", boot, "-o", "-", "::CONFIG.SYS", data=b"FILES=32\r\nBUFFERS=15\r\n"
        )
        if command:
            mtool("mcopy", volume, command, "::SHELL.EXE")
        entry = b"C:\\SHELL.EXE /C C:\\CASE.BAT" if command else b"C:\\CASE.BAT"
        mtool(
            "mcopy",
            boot,
            "-o",
            "-",
            "::AUTOEXEC.BAT",
            data=b"@ECHO OFF\r\nC:\r\n" + entry + b"\r\n",
        )
        for name, data in [
            ("SOURCE.BIN", PAYLOAD),
            ("ONE.DAT", b"A\x1aB"),
            ("TWO.DAT", b"CD"),
            ("KEEP.TXT", b"Keep\r\n"),
            ("HIDDEN.TXT", b"Hidden\r\n"),
            ("NO.TXT", b"N\r\n"),
        ]:
            mtool("mcopy", volume, "-", "::" + name, data=data)
        mtool("mattrib", volume, "+h", "::HIDDEN.TXT")
        lines = [
            "@ECHO OFF",
            "VER >VER.OUT",
            "VOL C: >VOL.OUT",
            "BREAK ON",
            "BREAK >BREAK.OUT",
            "VERIFY ON",
            "VERIFY >VERIFY.OUT",
            "MD BOX",
            "MKDIR BOX\\SUB",
            "CD BOX",
            "CHDIR >CWD.OUT",
            "CD..",
            "COPY SOURCE.BIN BOX\\LARGE.BIN >COPY.OUT",
            "COPY /B ONE.DAT+TWO.DAT BOX\\BOTH.BIN >>COPY.OUT",
            "COPY /A ONE.DAT+TWO.DAT BOX\\TEXT.BIN >>COPY.OUT",
            "COPY KEEP.TXT BOX\\ONE.TXT >>COPY.OUT",
            "COPY KEEP.TXT BOX\\TWO.TXT >>COPY.OUT",
            "REN BOX\\*.TXT *.LOG",
            "RENAME BOX\\TWO.LOG KEEP.DAT",
            "TYPE BOX\\KEEP.DAT >TYPE.OUT",
            "DIR BOX /B /O:N >DIR.OUT",
            "DIR /A:H /B >HIDDEN.OUT",
            "DIR /S /B *.LOG >RECUR.OUT",
            "ERASE BOX\\*.LOG",
            "COPY KEEP.TXT BOX\\TRASH.TMP",
            "DEL BOX\\TRASH.TMP",
            "RMDIR BOX\\SUB",
            "MD EMPTY",
            "RD EMPTY",
            "TRUENAME BOX\\KEEP.DAT >TRUE.OUT",
            "DATE <NO.TXT >DATE.OUT",
            "TIME <NO.TXT >TIME.OUT",
            "CHCP >CHCP.OUT",
            "MD USAGE",
            *[
                name + " /? >USAGE\\" + name + ".HLP"
                for name in COMMANDS
                if name != "TRUENAME"
            ],
            "ECHO COMMAND-DONE >DONE.OUT",
            "ECHO COMMAND-DONE >COM1",
        ]
        # Empty CR input accepts the existing date/time without leaving an interactive prompt.
        mtool("mcopy", volume, "-o", "-", "::NO.TXT", data=b"\r\n")
        mtool(
            "mcopy",
            volume,
            "-",
            "::CASE.BAT",
            data=("\r\n".join(lines) + "\r\n").encode(),
        )
        serial = proof / "console.log"
        serial.unlink(missing_ok=True)
        cmd = [
            "qemu-system-i386",
            "-machine",
            "pc,accel=tcg",
            "-cpu",
            "486",
            "-m",
            "16",
            "-nic",
            "none",
            "-display",
            "none",
            "-monitor",
            "stdio",
            "-serial",
            "file:" + str(serial),
            "-boot",
            "a",
            "-drive",
            f"if=floppy,format=raw,file={boot}",
            "-drive",
            f"if=ide,index=0,format=raw,file={work / 'target.img'}",
        ]
        if cpu == "286":
            config = work / "dosbox.conf"
            config.write_text(
                "[sdl]\noutput=surface\n[dosbox]\nshow start banner=false\nquit warning=false\n"
                "[cpu]\ncore=normal\ncputype=286\ncycles=fixed 50000\n[mixer]\nnosound=true\n"
                f"[serial]\nserial1=file file:{serial} timeout:0\n[autoexec]\nmount d .\n"
                "imgmount 2 d:\\target.img -t hdd -fs none -size 512,63,16,32\nboot boot.img\n"
            )
            cmd = [dosbox or "dosbox-x", "-conf", str(config), "-nogui", "-nomenu"]
        with (
            (proof / "vm.log").open("wb") as trace,
            subprocess.Popen(
                cmd,
                cwd=work,
                stdin=subprocess.PIPE,
                stdout=trace,
                stderr=trace,
                env=dict(
                    os.environ,
                    TMPDIR=str(work),
                    SDL_VIDEODRIVER="dummy",
                    SDL_AUDIODRIVER="dummy",
                ),
            ) as vm,
        ):
            try:
                limit = time.monotonic() + 95
                while vm.poll() is None and time.monotonic() < limit:
                    if serial.exists() and b"COMMAND-DONE" in serial.read_bytes():
                        break
                    if (
                        mtool("mtype", volume, "::DONE.OUT", check=False).stdout.strip()
                        == b"COMMAND-DONE"
                    ):
                        break
                    time.sleep(0.3)
                else:
                    raise RuntimeError(f"guest did not finish; see {serial}")
            finally:
                if vm.poll() is None:
                    if cpu == "286":
                        vm.terminate()
                    else:
                        vm.stdin.write(b"quit\n")
                        vm.stdin.flush()
                    vm.wait(10)
        outputs = {}
        for name in [
            "VER",
            "VOL",
            "BREAK",
            "VERIFY",
            "DIR",
            "HIDDEN",
            "RECUR",
            "TYPE",
            "DATE",
            "TIME",
            "TRUE",
            "COPY",
            "CHCP",
            "BOX/CWD",
        ]:
            val = mtool("mtype", volume, "::" + name + ".OUT", check=False).stdout
            outputs[name] = val
            (proof / (name.replace("/", "-") + ".txt")).write_bytes(val)
        failures = []
        for name, expected in {
            "BOX/BOTH.BIN": b"A\x1aBCD",
            "BOX/TEXT.BIN": b"ACD\x1a",
            "BOX/KEEP.DAT": b"Keep\r\n",
            "BOX/LARGE.BIN": PAYLOAD,
        }.items():
            data = mtool("mcopy", volume, "::" + name, "-", check=False).stdout
            (proof / (name.replace("/", "-"))).write_bytes(data)
            if data != expected:
                failures.append(f"{name}: {len(data)} bytes differ")
        for name in ["BOX/TRASH.TMP", "BOX/SUB", "BOX/ONE.LOG", "BOX/TWO.LOG", "EMPTY"]:
            if mtool("mdir", volume, "::" + name, check=False).returncode == 0:
                failures.append(f"{name} still exists")
        exact = {
            "BOX/CWD": b"C:\\BOX\r\n",
            "BREAK": b"BREAK is on\r\n",
            "VERIFY": b"VERIFY is on\r\n",
            "TYPE": b"Keep\r\n",
            "DIR": b"BOTH.BIN\r\nCWD.OUT\r\nKEEP.DAT\r\nLARGE.BIN\r\nONE.LOG\r\nSUB\r\nTEXT.BIN\r\n",
            "RECUR": b"C:\\BOX\\ONE.LOG\r\n",
            "HIDDEN": b"HIDDEN.TXT\r\n",
            "TRUE": b"\r\nC:\\BOX\\KEEP.DAT\r\n",
        }
        for name, expected in exact.items():
            if outputs[name] != expected:
                failures.append(f"{name}: {outputs[name]!r}")
        for name, pattern in {
            "VOL": rb"Volume in drive C is CMDTEST *\r\n Volume Serial Number is 9B6A-231F",
            "VER": rb"DOS Version [0-9]+\.[0-9]{2}",
            "DATE": rb"Current date is (Sun|Mon|Tue|Wed|Thu|Fri|Sat) (0[1-9]|1[0-2])-[0-3][0-9]-[0-9]{4}",
            "TIME": rb"Current time is [ 1][0-9]:[0-5][0-9]:[0-5][0-9]\.[0-9]{2}[ap]",
            "CHCP": rb"Active code page: [0-9]+",
        }.items():
            if not re.search(pattern, outputs[name]):
                failures.append(f"{name}: {outputs[name]!r}")
        # TRUENAME is undocumented and does not have a /? page in this release.
        counts = []
        for name in COMMANDS:
            if name == "TRUENAME":
                continue
            response = mtool(
                "mcopy", volume, "::USAGE/" + name + ".HLP", "-", check=False
            ).stdout
            counts.append(f"{name} {len(response)}")
            if not response.strip() or b"Bad command or file name" in response:
                failures.append(name + " /? did not respond")
        (proof / "help-catalogue.txt").write_text("\n".join(counts) + "\n")
        if failures:
            raise AssertionError("\n".join(failures))
        print("COMMAND completed", proof, flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True, type=Path)
    parser.add_argument("--ram-dir", required=True, type=Path)
    parser.add_argument("--proof", required=True, type=Path)
    parser.add_argument("--reference", action="store_true")
    parser.add_argument(
        "--command",
        type=Path,
        help="execute this interpreter as a child of the original MS-DOS kernel",
    )
    parser.add_argument("--cpu", choices=("286", "386"), default="386")
    parser.add_argument("--dosbox", help="DOSBox-X executable for the 80286 CPU model")
    opt = parser.parse_args()
    run(
        opt.image.resolve(),
        opt.ram_dir.resolve(),
        opt.proof.resolve(),
        opt.reference,
        opt.command,
        opt.cpu,
        opt.dosbox,
    )

#!/usr/bin/env python3
"""Validate DOS COMMAND.COM through a booted guest, its PSP, and a stopped FAT disk."""
import argparse
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

spec = importlib.util.spec_from_file_location("commands", Path(__file__).with_name("realmode-command.py"))
commands = importlib.util.module_from_spec(spec)
spec.loader.exec_module(commands)


def run(image, executable, ram, proof, dosbox):
    proof.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=ram) as tmp:
        work = Path(tmp)
        floppy = work / "boot.img"
        disk = work / "c.img"
        shutil.copyfile(image, floppy)
        volume = commands.image(disk)
        if executable:
            commands.mtool("mcopy", volume, "-", "::SHELL.COM", data=executable.read_bytes())
            shell = r"C:\SHELL.COM"
        else:
            shell = r"A:\COMMAND.COM"
        commands.mtool("mmd", volume, "::TOOLS")
        commands.mtool("mmd", volume, "::WINDOWS")

        def bat(drive, name, lines):
            data = ("\r\n".join(lines) + "\r\n").encode("ascii")
            commands.mtool("mcopy", drive, "-o", "-", "::" + name, data=data)

        bat(floppy, "CONFIG.SYS", ["FILES=32"])
        bat(floppy, "AUTOEXEC.BAT", ["@echo off", "C:", f"{shell} /C RUN.BAT >C:\\RESULT.OUT",
                                    "echo DONE>C:\\DONE.OUT", "CD \\WINDOWS"])
        bat(volume, "ONE.TXT", ["one"])
        bat(volume, "TWO.TXT", ["two"])
        bat(volume, "TOOLS/ARGS.BAT", ["@echo off", "echo CHILD=%1", "shift",
                                       "echo SHIFT=%1", "set RETURNED=%1"])
        bat(volume, "TOOLS/CHAIN.BAT", ["@echo off", "NEXT.BAT", "echo BAD-CHAIN"])
        bat(volume, "TOOLS/NEXT.BAT", ["@echo off", "echo CHAIN=OK"])
        bat(volume, "CMD.IN", ["ECHO PIPELINE=OK", "EXIT"])
        bat(volume, "RUN.BAT", [
            "@echo off", "set NAME=Parent", "path=C:\\TOOLS", "call ARGS first second",
            "if %RETURNED%==second echo ENV=%NAME%", "if not lower==LOWER echo CASE=OK",
            "if exist *.TXT echo EXIST=OK", "if exist *.NOP echo BAD-EXIST",
            "if errorlevel 0 echo ERRORLEVEL=OK", "for %%i in (a b) do echo LOOP=%%i",
            "for %%i in (*.TXT) do echo WILD=%%i", f"{shell} /C set NAME=Child",
            "echo PARENT=%NAME%", "call CHAIN", "goto END", "echo BAD-GOTO",
            *["rem cross the cache and sector boundary"] * 24, ":END",
            "echo FIRST>R.OUT", "echo SECOND>>R.OUT", "TYPE R.OUT", f"TYPE CMD.IN|{shell}",
            f"{shell} <CMD.IN", "echo FINISHED",
        ])
        log = proof / "serial.txt"
        log.unlink(missing_ok=True)
        if dosbox:
            # Extend only the unpartitioned tail to full DOSBox-X CHS cylinders.
            with disk.open("r+b") as stream:
                stream.truncate(33 * 16 * 63 * 512)
            config = work / "dosbox.conf"
            config.write_text("[sdl]\noutput=surface\n[dosbox]\nshow start banner=false\nquit warning=false\n"
                              "[cpu]\ncore=normal\ncputype=286\ncycles=fixed 50000\n[mixer]\nnosound=true\n"
                              f"[serial]\nserial1=file file:{log} timeout:0\n[autoexec]\nmount d .\n"
                              "imgmount 2 d:\\c.img -t hdd -fs none -size 512,63,16,33\nboot boot.img\n")
            args = [dosbox, "-conf", config, "-nogui", "-nomenu"]
        else:
            args = ["qemu-system-i386", "-machine", "pc,accel=tcg", "-cpu", "486", "-m", "16",
                    "-display", "none", "-nic", "none", "-monitor", "stdio", "-serial", "file:" + str(log),
                    "-boot", "a", "-drive", f"if=floppy,format=raw,file={floppy}",
                    "-drive", f"if=ide,format=raw,file={disk}"]
        with (proof / "emulator.log").open("wb") as trace, subprocess.Popen(
            args, cwd=work, stdin=subprocess.PIPE, stdout=trace, stderr=trace,
            env=dict(os.environ, TMPDIR=str(work), SDL_VIDEODRIVER="dummy", SDL_AUDIODRIVER="dummy"),
        ) as vm:
            try:
                deadline = time.monotonic() + 90
                while time.monotonic() < deadline and vm.poll() is None:
                    if commands.mtool("mtype", volume, "::DONE.OUT", check=False).stdout.strip() == b"DONE":
                        if executable or (log.exists() and b"C:\\WINDOWS>" in log.read_bytes()):
                            break
                    time.sleep(0.15)
                else:
                    raise RuntimeError("Guest did not complete the batch file or display the current directory prompt")
                if not executable and not dosbox:
                    # Type a command after AUTOEXEC returned to the persistent interpreter.
                    for key in ["e","c","h","o","spc","l","i","v","e","shift-dot","l","i","v","e","dot","t","x","t","ret"]:
                        vm.stdin.write(f"sendkey {key} 30\n".encode())
                        vm.stdin.flush()
                        time.sleep(0.07)
                    for _ in range(40):
                        typed = commands.mtool("mtype", volume, "::WINDOWS/LIVE.TXT", check=False)
                        if typed.stdout == b"live\r\n":
                            break
                        time.sleep(0.1)
                    else:
                        raise AssertionError("interactive keyboard command did not write WINDOWS/LIVE.TXT")
            finally:
                if vm.poll() is None:
                    if dosbox:
                        vm.terminate()
                    else:
                        vm.stdin.write(b"quit\n")
                        vm.stdin.flush()
                    try:
                        vm.wait(6)
                    except subprocess.TimeoutExpired:
                        vm.kill()
                        vm.wait()
        actual = commands.mtool("mcopy", volume, "::RESULT.OUT", "-").stdout
        (proof / "result.txt").write_bytes(actual)
        expected = [b"CHILD=first", b"SHIFT=second", b"ENV=Parent", b"CASE=OK", b"EXIST=OK",
                    b"ERRORLEVEL=OK", b"LOOP=a", b"LOOP=b", b"WILD=ONE.TXT", b"WILD=TWO.TXT",
                    b"PARENT=Parent", b"CHAIN=OK", b"FIRST", b"SECOND", b"PIPELINE=OK",
                    b"PIPELINE=OK", b"FINISHED"]
        if actual != b"\r\n".join(expected) + b"\r\n":
            raise AssertionError(actual)
        listing = commands.mtool("mdir", volume, "-b", "::").stdout.upper()
        if b".TMP" in listing:
            raise AssertionError("A pipe temporary file survived the command")
        print("PASS COMMAND environment, batch control flow, EXEC, pipes and redirection", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("image", "ram-dir", "proof"):
        parser.add_argument("--" + option, type=Path, required=True)
    parser.add_argument("--command", type=Path, help="Launch this DOS-RS shell under a reference DOS boot disk")
    parser.add_argument("--dosbox", type=Path, help="Use DOSBox-X with a 286 CPU")
    opts = parser.parse_args()
    run(opts.image.resolve(), opts.command, opts.ram_dir.resolve(), opts.proof.resolve(), opts.dosbox)

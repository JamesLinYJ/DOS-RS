"""Run native DOS handle/driver requests after a RAM C: installation."""
import argparse
import importlib.util
from pathlib import Path
import subprocess
import tempfile

from realmode_probe import build_example

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("installer", ROOT / "scripts/realmode-install-test.py")
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("toolchain", "image", "ram-dir", "proof"):
        parser.add_argument("--" + option, type=Path, required=True)
    args = parser.parse_args()
    args.proof.mkdir(parents=True, exist_ok=True)
    tools = args.toolchain.resolve()
    target = ROOT / "target/native16"
    probe = build_example(tools, "char_device")
    with tempfile.TemporaryDirectory(dir=args.ram_dir) as tmp:
        work = Path(tmp)
        driver = work / "ECHO.SYS"
        subprocess.run(["cargo", "+stable", "run", "--locked", "--release", "--manifest-path",
                        tools / "arch386/Cargo.toml", "--target-dir", target / "emm/host",
                        "--example", "echo_device", "--", driver], check=True)
        disk = installer.disk_file(work, "native-c.img", 8)
        with installer.vm(args.image.resolve(), [disk], args.proof, "install") as session:
            session.wizard("setup", 1, 1)
            session.key("ret")
            session.expect("Installation complete")
        volume = str(disk) + "@@1048576"
        for file, name in [(driver, "ECHO.SYS"), (probe, "DEVTEST.EXE")]:
            subprocess.run(["mcopy", "-o", "-i", volume, file, "::" + name], check=True)
        for name, extra in [("CONFIG.SYS", b"DEVICE=ECHO.SYS"), ("AUTOEXEC.BAT", b"DEVTEST\r\nMEMSTAT /GEMMIS")]:
            current = subprocess.check_output(["mcopy", "-i", volume, "::" + name, "-"])
            subprocess.run(["mcopy", "-o", "-i", volume, "-", "::" + name],
                           input=current.rstrip(b"\r\n\x1a") + b"\r\n" + extra + b"\r\n", check=True)
        for arch in ("i386", "x86_64"):
            with installer.vm(None, [disk], args.proof, arch, arch=arch) as session:
                session.expect("GEMMIS 1.0 physical address")
                session.expect("C:\\> ")
            log = (args.proof / (arch + ".txt")).read_bytes()
            for marker in [b"PASS: ten independent CON handles", b"PASS: DOS character device I/O",
                           b"GEMMIS 1.0 physical address"]:
                if marker not in log:
                    raise RuntimeError(f"{arch} missing {marker!r} in {log[-500:]!r}")
            print("PASS", arch, flush=True)


if __name__ == "__main__":
    main()

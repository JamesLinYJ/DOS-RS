#!/usr/bin/env python3
"""Boot the native shell and verify its executable paths and redirected files on a stopped RAM disk."""
import argparse
import hashlib
import json
import os
import shutil
from pathlib import Path
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell

DATA = bytes(range(256)) * 83 + b"DOS-RS"
HERE_DATA = b"0123456789abcdef" * 1025 + b"\n"
READ_DATA = (b"   alpha  beta  gamma  \n" b"  x\\ y:z\\:q:  end \n"
             b"left\\\nright tail\n" b"a\\b c\\ d\n"
             b"field1:field2::field4:|two words|abc\0last fragment")



def exercise(args, arch, mode, root, serial, dual_volume=False, job_control=False, grammar=False, std_tools=False):
    vm = Shell(args, serial, root)
    try:
        vm.wait(b"A:\\> ", timeout=180)
        vm.command("c:")
        if dual_volume:
            vm.command("fdcheck.elf", expect=b"unlink, shared offset, pipe EOF OK")
        vm.command("sh", prompt=b"$ ")
        vm.command("cat a.txt | wc -c > count.txt", prompt=b"$ ")
        vm.command("cat count.txt", prompt=b"$ ", expect=str(len(DATA)).encode() + b"\r\n")
        vm.command("cat a.txt | cat > copy.bin", prompt=b"$ ", timeout=180 if dual_volume else 40)
        vm.command("absent", prompt=b"$ ", expect=b"command not found")
        vm.command("echo $?", prompt=b"$ ", expect=b"127\r\n")
        if std_tools:
            vm.command("base64 a.txt > encoded.txt", prompt=b"$ ", timeout=240)
            vm.command("base64 -d encoded.txt > decoded.bin", prompt=b"$ ", timeout=240)
        if grammar:
            prefix = "/dos/c/" if dual_volume else "./"
            vm.command(prefix + "dirprobe", prompt=b"$ ", expect=b"directory: enumeration, openat, fstatat, fchdir and root parent passed")
            vm.command(prefix + "execprog", prompt=b"$ ", expect=b"exec: ENOEXEC, PATH, argv and environment passed")
            vm.command('sh control.sh one "two words" > subst.out', prompt=b"$ ", timeout=300 if dual_volume else 120)
            vm.command("echo $? > subst.rc", prompt=b"$ ")
            vm.command("sh expand.sh > exp.out", prompt=b"$ ", timeout=300 if dual_volume else 120)
            vm.command("echo $? > exp.rc", prompt=b"$ ")
            vm.command("sh ifs.sh > ifs.out", prompt=b"$ ")
            vm.command("echo $? > ifs.rc", prompt=b"$ ")
            vm.command("sh read.sh > read.out", prompt=b"$ ", timeout=120)
            vm.command("echo $? > read.rc", prompt=b"$ ")
            vm.command("sh here.sh > here.out", prompt=b"$ ", timeout=300 if dual_volume else 180)
            vm.command("echo $? > here.rc", prompt=b"$ ")
            vm.command("sh getopts.sh > opts.out", prompt=b"$ ", timeout=180)
            vm.command("echo $? > opts.rc", prompt=b"$ ")
            vm.command("sh vars.sh > vars.out", prompt=b"$ ", timeout=300 if dual_volume else 180)
            vm.command("echo $? > vars.rc", prompt=b"$ ")
            vm.command("readonly retained=original", prompt=b"$ ")
            vm.command("retained=changed", prompt=b"$ ", expect=b"retained: readonly variable")
            vm.command("echo $?", prompt=b"$ ", expect=b"1\r\n")
            vm.command("read retained < read.dat", prompt=b"$ ", expect=b"retained: readonly variable")
            vm.command("getopts a retained -a", prompt=b"$ ", expect=b"retained: readonly variable")
            vm.command("echo $?", prompt=b"$ ", expect=b"2\r\n")
            vm.command('echo "$retained"', prompt=b"$ ", expect=b"original\r\n")
        if job_control:
            offset = serial.stat().st_size
            vm.enter("cat")
            vm.wait(b"path=/bin/cat", offset)
            vm.key("ctrl-z")
            vm.wait(b"Stopped cat", offset)
            vm.command("jobs", prompt=b"$ ", expect=b"Stopped cat")
            offset = serial.stat().st_size
            vm.enter("fg")
            vm.key("ctrl-c")
            vm.wait(b"$ ", offset)
            vm.command("echo $?", prompt=b"$ ", expect=b"130\r\n")
        vm.command("exit 7")
        vm.command("echo $?", expect=b"7\r\n")
        vm.command("a:", prompt=b"A:\\> ")
        vm.command("hello.com", prompt=b"A:\\> ", expect=b"GUEST HELLO COM")
    finally:
        vm.close()
    qemu.check_native_boot(serial, arch, mode)
    trace = serial.read_bytes()
    for path in (b"/bin/sh", b"/bin/cat", b"/bin/wc", b"/bin/echo"):
        if b"path=" + path + b"\r\n" not in trace:
            raise RuntimeError(f"{arch}: missing executed path {path!r}")
    print(f"{arch}/{mode}: guest ran /bin/sh, /bin/cat, /bin/wc and /bin/echo", flush=True)


PROBES = {"execprog": "exec_script.c", "dirprobe": "working_dir.c"}


def build_probe(arch, output):
    subprocess.run([sys.executable, qemu.ROOT / "ports/musl/cc.py", "--arch", arch,
                    "-o", output, qemu.ROOT / "ports/musl/tests" / PROBES[output.name]], check=True)


def script_fixture(fixture, work):
    (fixture / "SUB").mkdir()
    (fixture / "SUB/ONE.TX").write_bytes(b"one")
    (fixture / "SUB/TWO.TX").write_bytes(b"two")
    (fixture / "read.dat").write_bytes(READ_DATA)
    (fixture / "large.sh").write_bytes(b"cat <<'END'\n" + HERE_DATA + b"END\n")
    for name in ("control.sh", "expand.sh", "ifs.sh", "read.sh", "here.sh", "getopts.sh", "plain"):
        shutil.copyfile(qemu.ROOT / "tests/user-shell" / name, fixture / name)
    shutil.copyfile(qemu.ROOT / "tests/user-shell/variables.sh", fixture / "vars.sh")
    (fixture / "plain").chmod(0o755)
    reference_shell = os.environ.get("REFERENCE_SH") or shutil.which("dash") or "/bin/sh"
    with tempfile.TemporaryDirectory(dir=work) as temporary:
        refdir = Path(temporary)
        shutil.copy2(fixture / "plain", refdir / "plain")
        shutil.copy2(fixture / "large.sh", refdir / "large.sh")
        shutil.copytree(fixture / "SUB", refdir / "SUB")
        expected = {}
        for script, args, output, status, code in [
            ("control.sh", ["one", "two words"], "subst.out", "subst.rc", 13),
            ("expand.sh", [], "exp.out", "exp.rc", 17),
            ("here.sh", [], "here.out", "here.rc", 37),
        ]:
            ref = subprocess.run([reference_shell, fixture / script, *args], cwd=refdir,
                                 env=dict(os.environ, LC_ALL="C"), capture_output=True)
            if ref.returncode != code or ref.stderr:
                raise RuntimeError(f"host shell reference failed: {ref!r}")
            expected[output] = ref.stdout
            expected[status] = f"{code}\n".encode()
        if (refdir / "here.bin").read_bytes() != HERE_DATA:
            raise RuntimeError("host shell truncated the large here-document")
        expected["here.bin"] = HERE_DATA
    # dash and bash disagree about adjacent IFS bytes from separate expansions.
    # Keep the Issue 8 examples independent of the chosen host interpreter.
    expected["ifs.out"] = b"2:[x]:[y]\n3:[x]:[:]:[y]\n"
    expected["ifs.rc"] = b"23\n"
    expected["read.out"] = (
        b"space:0:[alpha]:[beta  gamma]\nescaped:0:[x y]:[z:q]:[end]\n"
        b"continued:0:[leftright]:[tail]\nraw:0:[a\\b c\\ d]:[]\n"
        b"remainder:0:[field1]:[field2::field4:]\nnext:0:[two words]\n"
        b"nul:0:[abc]\npartial:1:[last]:[fragment]\neof:1:[]:[]\n"
        b"pipe:[piped]:[value]\nremain:1:positional\n")
    expected["read.rc"] = b"31\n"
    # Issue 8 requires an unset OPTARG and status >1 for assignment errors.
    # The host dash and bash differ on these cases and on diagnostic identity.
    expected["opts.out"] = (qemu.ROOT / "tests/user-shell/getopts.expected").read_bytes()
    expected["opts.rc"] = b"43\n"
    expected["vars.out"] = (qemu.ROOT / "tests/user-shell/variables.expected").read_bytes()
    expected["vars.rc"] = b"47\n"
    return expected


def check(arch, root, proof, kernel=None, programs=None):
    mode = "bios" if arch == "i686" else "uefi"
    source_kernel = kernel or qemu.build(arch, qemu.ROOT / "target", features="exec-trace")
    kernel = root / "kernel.efi"
    shutil.copyfile(source_kernel, kernel)
    if programs is None:
        programs = root / "bin"
        subprocess.run([sys.executable, qemu.ROOT / "scripts/build-rust-tools.py", "--arch", arch,
                        "--output", programs], check=True)
    staged = root / "programs"
    staged.mkdir()
    for name in ("sh", "cat", "wc", "echo"):
        shutil.copyfile(programs/name, staged/name)
    programs = staged
    disk = root / "c.img"
    with disk.open("wb") as f:
        f.truncate(8 << 20)
    subprocess.run(["mformat", "-i", disk, "::"], check=True)
    subprocess.run(["mmd", "-i", disk, "::bin"], check=True)
    subprocess.run(["mcopy", "-i", disk,
                    *(programs/name for name in ("sh", "cat", "wc", "echo")), "::bin/"], check=True)
    fixture = root / "fixture"
    fixture.mkdir()
    reference = script_fixture(fixture, root)
    (fixture / "a.txt").write_bytes(DATA)
    for name in PROBES:
        build_probe(arch, fixture / name)
    subprocess.run(["mcopy", "-s", "-i", disk, *sorted(fixture.iterdir()), "::"], check=True)
    serial = proof / (arch+".serial")
    args=qemu.boot_args(kernel, arch, root / "boot", 4, mode, esp_mib=36)
    exercise(args + ["-drive", f"if=ide,format=raw,file={disk}"], arch, mode, root, serial, job_control=True, grammar=True)
    with (proof / (arch+"-fsck.log")).open("wb") as log:
        subprocess.run(["fsck.fat", "-n", disk], stdout=log, stderr=subprocess.STDOUT, check=True)
    files = {}
    expected_files = {"count.txt": f"{len(DATA)}\n".encode(), "copy.bin": DATA, **reference}
    recovered = root / "recovered"
    recovered.mkdir()
    subprocess.run(["mcopy", "-i", disk,
                    *("::" + name for name in expected_files), recovered], check=True)
    for name, expected in expected_files.items():
        result = (recovered / name).read_bytes()
        (proof/(arch+"-"+name)).write_bytes(result)
        if result != expected:
            (proof/(arch+"-"+name+".expected")).write_bytes(expected)
            raise RuntimeError(f"{arch}: stopped {name} differs")
        digest = hashlib.sha256(result).hexdigest()
        files[name] = {"bytes": len(result), "sha256": digest}
        print(f"{arch}/{mode} {name}: {len(result)} bytes sha256 {digest}")
    manifest = {"arch": arch, "kernel_bits": 32 if arch == "i686" else 64, "firmware": mode,
                "kernel_sha256": hashlib.sha256(kernel.read_bytes()).hexdigest(),
                "programs": {name: hashlib.sha256((programs/name).read_bytes()).hexdigest()
                             for name in ("sh", "cat", "wc", "echo")}, "files": files}
    (proof / (arch+".json")).write_text(json.dumps(manifest, indent=2)+"\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ram-dir", required=True, type=Path)
    parser.add_argument("--proof", type=Path, default=qemu.ROOT/"target/user-shell-proof")
    parser.add_argument("--arch", choices=qemu.ARCH, action="append")
    parser.add_argument("--kernel", type=Path, help="use this kernel for exactly one --arch")
    parser.add_argument("--programs", type=Path, help="directory containing that architecture's sh, cat, wc and echo")
    options=parser.parse_args()
    if (options.kernel or options.programs) and len(options.arch or ()) != 1:
        parser.error("prebuilt inputs require exactly one --arch")
    options.proof.mkdir(parents=True, exist_ok=True)
    for arch in options.arch or qemu.ARCH:
        with tempfile.TemporaryDirectory(dir=options.ram_dir) as tmp:
            check(arch,Path(tmp),options.proof,options.kernel,options.programs)

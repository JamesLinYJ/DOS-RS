#!/usr/bin/env python3
"""Compare disk-loaded Rust utilities with an identified native GNU coreutils 9.12 build."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell

FIXTURES = {
    "a.txt": bytes(range(256)) * 83 + b"DOS-RS",
    "b.txt": b"\n\r\nalpha\tomega\n\n\nB\x01C\xff\n\r",
    "c.txt": b"next\n\n\nend\r\n",
    "z.txt": b"",
    "utf8.txt": ("a" * 4095 + "横滨\t韓文・français\n" + "e\u0301\u00a0😀\n").encode()
    + b"\xff x\xc3B\xf0\x9f",
    "cr.txt": b"\r",
    "lf.txt": b"\n",
    "-q": b"file starting with a dash\n",
    "long.txt": b"q" * 4095 + b"\r\n" + b"b" * 5901 + b"\t!",
}


def catalog():
    cases = []

    def add(tool, args=(), stdin="z.txt", locale="C"):
        cases.append((tool, list(args), stdin, locale))

    for args, stdin in [
        ([], "a.txt"),
        (["-"], "b.txt"),
        (["a.txt"], "z.txt"),
        (["-u", "a.txt"], "z.txt"),
        (["b.txt", "-", "c.txt"], "b.txt"),
        (["--", "-q"], "z.txt"),
        (["missing", "b.txt"], "z.txt"),
        (["-Q"], "z.txt"),
        (["--unexpected"], "z.txt"),
    ]:
        add("cat", args, stdin)
    for option in [
        "-A",
        "-bE",
        "-n",
        "-s",
        "-v",
        "-T",
        "-t",
        "-e",
        "-ns",
        "--show-all",
        "--number-nonblank",
    ]:
        add("cat", [option, "b.txt", "c.txt"])
    add("cat", ["-E", "long.txt"])
    add("cat", ["-E", "cr.txt", "lf.txt"])
    add("cat", ["-vET", "a.txt"])
    for args, stdin in [
        ([], "a.txt"),
        (["-"], "b.txt"),
        (["-c"], "a.txt"),
        (["-wcmlL"], "a.txt"),
        (["-lw", "-", "b.txt"], "c.txt"),
        (["-l"], "long.txt"),
        (["-m", "a.txt"], "z.txt"),
        (["--", "-q"], "z.txt"),
        (["-Z"], "z.txt"),
        (["--bogus"], "z.txt"),
        (["missing", "a.txt"], "z.txt"),
        (["a.txt", "z.txt", "b.txt"], "z.txt"),
        (["-cm", "z.txt"], "z.txt"),
        (["-L", "long.txt"], "z.txt"),
    ]:
        add("wc", args, stdin)
    for total in ["auto", "never", "always", "only"]:
        add("wc", ["--total=" + total, "a.txt", "b.txt"])
    for option in ["--lines", "--words", "--chars", "--bytes", "--max-line-length"]:
        add("wc", [option, "b.txt"])
    for name in ["true", "false"]:
        for args in [[], ["ignored"], ["-n", "--"]]:
            add(name, args)
    for args in [
        [],
        ["a", "b"],
        ["-n", "a", "b"],
        ["-e", r"a\tb\n"],
        ["-e", r"\c", "unused"],
        ["-E", r"\t"],
        ["-ne", r"\0101\x42\0777"],
        ["-x", "-e", "two"],
        ["-", "--"],
    ]:
        add("echo", args)
    for args in [
        [],
        [""],
        ["a/b/"],
        ["a"],
        ["////"],
        ["a//b/c/"],
        ["-z", "/a/b", "//"],
        ["-Q"],
    ]:
        add("dirname", args)
    for args in [
        [],
        [""],
        ["//"],
        ["a/b/"],
        ["a/b.c", ".c"],
        ["a", "a"],
        ["-a", "a//b", "c/"],
        ["-za", "x", "//"],
        ["-s.c", "foo.c", "/a/bar.c"],
        ["--suffix=.c", "a/b.c"],
        ["-Q"],
    ]:
        add("basename", args)
    for flag in ["-cwmlL", "-m", "-L", "-w"]:
        add("wc", [flag], "utf8.txt", "C.UTF-8")
    for name in ["head", "tail"]:
        for options in [
            [],
            ["-n", "2"],
            ["-n", "-2"],
            ["-n", "+2"],
            ["-c3"],
            ["--bytes=-3"],
            ["-c", "+5"],
            ["-1"],
            ["-n", "0"],
            ["-n", "900"],
            ["-v"],
            ["-z", "-n", "2"],
        ]:
            add(name, options, "b.txt" if "-z" not in options else "a.txt")
        for options in [["-q", "-n", "2"], ["-v", "-c", "2"], ["--lines=3"]]:
            add(name, options + ["b.txt", "c.txt"])
        add(name, ["-c", "-4097", "long.txt"])
        add(name, ["-n", "-1", "long.txt"])
        add(name, ["-n", "x"])
        add(name, ["-n"])
        add(name, ["missing", "-"], "b.txt")
    add("cat", ["@stdout"])
    return cases


def digest(data):
    return hashlib.sha256(data).hexdigest()


def on_disk(disk, name):
    return subprocess.check_output(["mcopy", "-i", disk, "::" + name, "-"])


def stage(disk, name, data):
    subprocess.run(["mcopy", "-i", disk, "-", "::" + name], input=data, check=True)


def run(arch, root, proof, gnubin):
    root.mkdir(parents=True, exist_ok=True)
    proof.mkdir(parents=True, exist_ok=True)
    root = root.resolve()
    disk = root / "tools.img"
    mode = "bios" if arch == "i686" else "uefi"
    kernel = qemu.build(arch, qemu.ROOT / "target", features="exec-trace")
    with disk.open("wb") as f:
        f.truncate(16 << 20)
    subprocess.run(["mformat", "-i", disk, "::"], check=True)
    subprocess.run(["mmd", "-i", disk, "::bin"], check=True)
    subprocess.run(
        [
            sys.executable,
            qemu.ROOT / "scripts/build-rust-tools.py",
            "--tools-only",
            "--arch",
            arch,
            "--output",
            root / "bin",
        ],
        check=True,
    )
    driver = root / "probe.elf"
    subprocess.run(
        [
            sys.executable,
            qemu.ROOT / "scripts/native-cc.py",
            "--arch",
            arch,
            "-o",
            driver,
            qemu.ROOT / "tests/native-coreutils/driver.c",
        ],
        check=True,
    )
    subprocess.run(["mcopy", "-i", disk, driver, "::"], check=True)
    suite = catalog()
    tools = sorted(set(name for name, _, _, _ in suite))
    env = {"LC_ALL": "C", "LANG": "C", "TZ": "UTC", "PATH": str(gnubin)}
    build_info = {}
    for name in tools:
        reference = gnubin / name
        version = (
            subprocess.check_output([str(reference), "--version"], env=env)
            if name != "false"
            else subprocess.run(
                [str(reference), "--version"], env=env, stdout=subprocess.PIPE
            ).stdout
        )
        if b"GNU coreutils) 9.12\n" not in version.splitlines(keepends=True)[:1][0]:
            raise RuntimeError(f"{reference} is not GNU 9.12")
        build_info[name] = {
            "reference": digest(reference.read_bytes()),
            "dosrs": digest((root / "bin" / name).read_bytes()),
        }
        subprocess.run(
            ["mcopy", "-i", disk, root / "bin" / name, "::bin/" + name], check=True
        )
    for name, data in FIXTURES.items():
        stage(disk, name, data)
    config = bytearray()
    expected = []
    with tempfile.TemporaryDirectory(dir=root) as sandbox:
        temp = Path(sandbox)
        for name, data in FIXTURES.items():
            (temp / name).write_bytes(data)
        for i, (name, args, stdin, locale) in enumerate(suite):
            stdout_name = f"q{i:03}.out"
            append = "@stdout" in args
            args = [stdout_name if a == "@stdout" else a for a in args]
            if append:
                seed = b"preserve the shared input and append target\n"
                stage(disk, stdout_name, seed)
                (temp / stdout_name).write_bytes(seed)
            with (
                (temp / stdin).open("rb") as input_file,
                (temp / stdout_name).open("ab" if append else "wb") as output_file,
            ):
                result = subprocess.run(
                    [name, *args],
                    executable=gnubin / name,
                    stdin=input_file,
                    env=dict(env, LC_ALL=locale),
                    cwd=temp,
                    stdout=output_file,
                    stderr=subprocess.PIPE,
                )
            result.stdout = (temp / stdout_name).read_bytes()
            expected.append(result)
            config.extend((len(args) + 1, int(append)))
            for field in [stdin, "LC_ALL=" + locale, "C:/bin/" + name, name, *args]:
                config.extend(os.fsencode(field) + b"\0")
        for name, data in FIXTURES.items():
            if (temp / name).read_bytes() != data:
                raise RuntimeError("reference modified " + name)
    stage(disk, "cases.bin", config + b"\0")
    serial = proof / (arch + ".serial")
    vm = Shell(
        qemu.boot_args(kernel, arch, root / "boot", 2, mode, esp_mib=36)
        + ["-drive", f"if=ide,format=raw,file={disk}"],
        serial,
        root,
    )
    try:
        vm.wait(b"A:\\> ")
        vm.command("c:")
        vm.command("probe.elf", expect=b"coreutils cases finished")
    finally:
        vm.close()
    statuses = on_disk(disk, "status.bin")
    if len(statuses) != len(suite):
        raise RuntimeError(
            f"{arch}: only {len(statuses)} wait statuses, expected {len(suite)}"
        )
    checks = []
    for i, ((tool, args, stdin, locale), ref) in enumerate(zip(suite, expected)):
        out, err = [on_disk(disk, f"q{i:03}.{ext}") for ext in ("out", "err")]
        passed = (out, err, statuses[i]) == (ref.stdout, ref.stderr, ref.returncode)
        checks.append(
            dict(
                id=i,
                tool=tool,
                argv=[f"q{i:03}.out" if a == "@stdout" else a for a in args],
                append_existing="@stdout" in args,
                stdin=stdin,
                locale=locale,
                stdout=digest(out),
                stderr=digest(err),
                status=statuses[i],
                passed=passed,
            )
        )
        if not passed:
            for side, data in [
                ("actual.out", out),
                ("expected.out", ref.stdout),
                ("actual.err", err),
                ("expected.err", ref.stderr),
            ]:
                (proof / f"{arch}-{i}-{side}").write_bytes(data)
            print(
                f"{arch} case {i}: {tool} {args!r}: exit {statuses[i]} expected {ref.returncode}",
                flush=True,
            )
    for name, data in FIXTURES.items():
        if on_disk(disk, name) != data:
            raise RuntimeError("guest modified " + name)
    traces = serial.read_bytes()
    for name in tools:
        if (b"path=C:/bin/" + name.encode() + b"\r\n") not in traces:
            raise RuntimeError("missing guest execve trace for " + name)
    with (proof / (arch + "-fsck.txt")).open("wb") as log:
        subprocess.run(
            [os.environ.get("FSCK_FAT", "fsck.fat"), "-n", disk], check=True, stdout=log
        )
    result = dict(
        arch=arch,
        firmware=mode,
        tools=build_info,
        locales=sorted({case[3] for case in suite}),
        timezone="UTC",
        kernel=digest(kernel.read_bytes()),
        fixtures={n: digest(d) for n, d in FIXTURES.items()},
        cases=checks,
    )
    (proof / (arch + ".json")).write_text(json.dumps(result, indent=2) + "\n")
    failures = sum(not c["passed"] for c in checks)
    if failures:
        raise RuntimeError(
            f"{failures} of {len(checks)} differential cases failed on {arch}"
        )
    print(
        f"{arch}/{mode}: {len(checks)} GNU 9.12 byte/status comparisons passed",
        flush=True,
    )


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch", action="append", choices=list(qemu.ARCH))
    cli.add_argument(
        "--ram-dir",
        required=True,
        type=Path,
        help="write all guest disks and reference fixtures here",
    )
    cli.add_argument("--proof", type=Path, default=qemu.ROOT / "target/coreutils-proof")
    cli.add_argument(
        "--gnu-bin",
        type=Path,
        required=True,
        help="GNU 9.12 binaries under their unprefixed names",
    )
    args = cli.parse_args()
    for arch in args.arch or qemu.ARCH:
        run(arch, args.ram_dir / arch, args.proof, args.gnu_bin.resolve())

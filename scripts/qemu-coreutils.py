#!/usr/bin/env python3
"""Compare disk-loaded Rust utilities with an identified native GNU coreutils 9.12 build."""

import argparse
import base64
import hashlib
import json
import os
import shutil
from pathlib import Path
import subprocess
import sys
import tempfile
import time

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


def upstream_cases():
    cases = []
    for tool in ("base32", "base64"):
        fixture = tool + ".txt"
        cases.extend((tool, args, stdin, "C") for args, stdin in [
            ([], "a.txt"), (["b.txt"], "z.txt"), (["-w0", "a.txt"], "z.txt"),
            (["--wrap=13", "-"], "long.txt"), (["--decode"], fixture),
            (["-d", fixture], "z.txt"), (["-di"], "junk" + tool[-2:] + ".txt"),
            (["-d"], "bad.txt"), (["missing"], "z.txt"), ([], "z.txt"),
        ])
    return cases


def upstream_text_cases():
    groups = {
        "comm": [(["s1.txt", "s2.txt"], "z.txt"), (["-12", "s1.txt", "s2.txt"], "z.txt")],
        "cut": [(["-b", "2-5"], "b.txt"), (["-d", ":", "-f2", "-s"], "cols.txt"), (["-c1-2", "s1.txt"], "z.txt")],
        "expand": [([], "cols.txt"), (["-t4", "b.txt"], "z.txt")],
        "fold": [(["-w", "5"], "cols.txt"), (["-b", "-w3", "s1.txt"], "z.txt")],
        "nl": [([], "s1.txt"), (["-ba", "-w3", "-s", "|"], "cols.txt")],
        "paste": [(["s1.txt", "s2.txt"], "z.txt"), (["-s", "-d", ":,"], "s1.txt")],
        "printf": [(["%s:%03d\\n", "val", "9"], "z.txt"), (["%b", r"x\n\x41"], "z.txt")],
        "seq": [(["1", "5"], "z.txt"), (["-s", ",", "-2", "2", "4"], "z.txt"), (["-w", "8", "11"], "z.txt")],
        "sort": [([], "cols.txt"), (["-nr", "nums.txt"], "z.txt"), (["-u", "s1.txt"], "z.txt")],
        "tr": [(["a-z", "A-Z"], "cols.txt"), (["-d", "0-9"], "nums.txt"), (["-s", "a"], "s1.txt")],
        "unexpand": [([], "cols.txt"), (["-a", "-t4"], "b.txt")],
        "uniq": [([], "s1.txt"), (["-c"], "s1.txt"), (["-d", "s1.txt"], "z.txt")],
    }
    return [(name, args, stdin, "C") for name, cases in groups.items() for args, stdin in cases]


def upstream_data_cases():
    cases = []
    for tool in ("b2sum", "md5sum", "sha1sum", "sha224sum", "sha256sum", "sha384sum", "sha512sum"):
        for args, stdin in [([], "a.txt"), (["z.txt"], "z.txt"), (["-b", "s1.txt", "s2.txt"], "z.txt")]:
            cases.append((tool, args, stdin, "C"))
    groups = {
        "basenc": [(["--base16", "-w0"], "a.txt"), (["--base64url"], "b.txt")],
        "cksum": [([], "a.txt"), (["s1.txt", "s2.txt"], "z.txt")],
        "sum": [([], "a.txt"), (["-s", "z.txt", "nums.txt"], "z.txt")],
        "expr": [(["8", "+", "9"], "z.txt"), (["7", ">", "11"], "z.txt"), (["substr", "hello", "2", "3"], "z.txt")],
        "factor": [(["91", "120", "65537"], "z.txt"), ([], "facts.txt")],
        "fmt": [(["-w", "12"], "cols.txt"), (["-s", "-w5", "s1.txt"], "z.txt")],
        "numfmt": [(["--to=iec", "1048576", "2048"], "z.txt"), (["--from=si", "3k"], "z.txt")],
        "od": [(["-An", "-tx1", "-N16", "a.txt"], "z.txt"), (["-An", "-tu1", "-j1"], "b.txt")],
        "tac": [([], "b.txt"), (["-s", ":", "cols.txt"], "z.txt")],
        "tsort": [([], "edges.txt"), (["z.txt"], "z.txt")],
    }
    return cases + [(name, args, stdin, "C") for name, pairs in groups.items() for args, stdin in pairs]


UPSTREAM_CASES = {"encoding": upstream_cases, "text": upstream_text_cases, "data": upstream_data_cases}


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


def namespace_cases():
    cases = []

    def add(name, args):
        cases.append((name, args, "z.txt", "C"))

    for args in [
        [],
        ["a.txt"],
        ["missing", "absent"],
        ["a.txt", "a.txt"],
        ["b.txt", "linked.txt"],
    ]:
        add("link", args)
    add("cat", ["linked.txt"])
    add("unlink", ["linked.txt"])
    add("cat", ["linked.txt"])
    for args in [[], ["a.txt", "b.txt"], ["missing"]]:
        add("unlink", args)
    for args in [
        [],
        ["-m"],
        ["-m", "bad", "absent"],
        ["-m", "0711", "dirmode"],
        ["-m", "a=rx,u+w", "symbolic"],
        ["-pv", "nest/inner"],
        ["-pv", "nest/inner"],
        ["--", "a.txt"],
        ["-p", "a.txt"],
        ["foo", "bar"],
    ]:
        add("mkdir", args)
    for args in [
        [],
        ["nest"],
        ["--ignore-fail-on-non-empty", "nest"],
        ["-pv", "nest/inner"],
        ["a.txt"],
        ["foo", "bar"],
        ["missing"],
    ]:
        add("rmdir", args)
    return cases


NAMESPACE_PATHS = [
    "absent",
    "linked.txt",
    "dirmode",
    "symbolic",
    "nest",
    "nest/inner",
    "foo",
    "bar",
]


def namespace_state(tree, archive=None):
    # btrfs restore -m records Linux permission bits in the capture archive.
    # Host extraction need not apply guest directory modes to a macOS RAM disk.
    modes = {}
    if archive is not None:
        modes = {row["path"]: row["mode"] for row in json.loads(archive.read_text())}
    result = {}
    for name in NAMESPACE_PATHS:
        path = tree / name
        if not path.exists():
            result[name] = None
        else:
            result[name] = dict(
                kind="directory" if path.is_dir() else "file",
                mode=oct((modes["files/" + name] if archive is not None else path.stat().st_mode) & 0o7777),
            )
            if path.is_file():
                result[name]["sha256"] = digest(path.read_bytes())
    return result


def digest(data):
    return hashlib.sha256(data).hexdigest()


def btrfs(command, disk, options, helper_args, log):
    helper = qemu.ROOT / "scripts/linux-btrfs.py"
    with log.open("wb") as output:
        subprocess.run(
            [sys.executable, helper, command, "--image", disk, *options, *helper_args],
            check=True,
            stdout=output,
            stderr=subprocess.STDOUT,
        )


def format_image(disk, tree, fs, helper_args, log):
    if fs == "btrfs":
        btrfs("create", disk, ["--root", tree], helper_args, log)
        return
    with disk.open("xb") as out:
        total = sum(entry.stat().st_size for entry in tree.rglob("*") if entry.is_file())
        out.truncate((16 << 20) + ((total + 511) // 512) * 512)
    subprocess.run(["mformat", "-i", disk, "::"], check=True)
    subprocess.run(
        ["mcopy", "-i", disk, "-s", *sorted(tree.iterdir()), "::"], check=True
    )


def stopped_files(disk, fs, destination, helper_args, log):
    if fs == "btrfs":
        btrfs("check", disk, ["--output", destination], helper_args, log)
        shutil.copyfile(destination / "check.log", log.with_suffix(".check"))
        return lambda name: (destination / "files" / name).read_bytes()
    with log.open("wb") as out:
        subprocess.run(
            [os.environ.get("FSCK_FAT", "fsck.fat"), "-n", disk], check=True, stdout=out
        )
    return lambda name: subprocess.check_output(["mcopy", "-i", disk, "::" + name, "-"])


def run_cases(vm):
    start = vm.log.stat().st_size
    vm.enter("probe.elf")
    previous = b""
    last_event = time.monotonic()
    while vm.vm.poll() is None:
        progress = vm.log.read_bytes()[start:]
        if progress != previous:
            previous = progress
            last_event = time.monotonic()
        if b"C:\\> " in progress:
            if b"coreutils cases finished" not in progress:
                raise RuntimeError(f"guest returned early: {progress[-300:]!r}")
            return
        if time.monotonic() - last_event > 90:
            raise RuntimeError(f"no guest progress in 90 s: {progress[-300:]!r}")
        time.sleep(0.15)
    raise RuntimeError("QEMU exited before the differential suite finished")


def run(arch, root, proof, gnubin, fs, helper_args, namespace_only, upstream, upstream_set):
    proof.mkdir(parents=True, exist_ok=True)
    tree = root / "seed"
    (tree / "bin").mkdir(parents=True)
    disk = root / "tools.img"
    mode = "bios" if arch == "i686" else "uefi"
    kernel = root / "kernel.efi"
    shutil.copyfile(qemu.build(arch, qemu.ROOT / "target", features="exec-trace"), kernel)
    if upstream:
        upstream_suite = UPSTREAM_CASES[upstream_set]()
        requested_tools = sorted({case[0] for case in upstream_suite})
        subprocess.run([
            sys.executable, qemu.ROOT / "scripts/build-uutils.py", "--arch", arch,
            "--output", root / "bin", "--archive", upstream, "--offline", *requested_tools
        ], check=True)
    else:
        subprocess.run([
            sys.executable, qemu.ROOT / "scripts/build-rust-tools.py", "--tools-only",
            "--arch", arch, "--output", root / "bin",
        ], check=True)
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
    shutil.copy2(driver, tree / "probe.elf")
    base = catalog()
    if upstream:
        suite = upstream_suite
        id_offset = len(base) + len(namespace_cases())
        for group, cases in UPSTREAM_CASES.items():
            if group == upstream_set:
                break
            id_offset += len(cases())
    elif namespace_only:
        suite = namespace_cases()
        id_offset = len(base)
    else:
        suite = base + (namespace_cases() if fs == "btrfs" else [])
        id_offset = 0
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
        shutil.copy2(root / "bin" / name, tree / "bin" / name)
    fixtures = dict(FIXTURES)
    if upstream:
        fixtures.update({
            "s1.txt": b"a\naa\naa\nbb\nd\n", "s2.txt": b"aa\ncc\nd\nz\n",
            "cols.txt": b"a:9\tb\nxy:22\n  a    z\t   a\n\n",
            "nums.txt": b"20\n-1\n4\n2\n12\n",
            "facts.txt": b"42\n97\n", "edges.txt": b"a b\nb c\nc d\n",
        })
        fixtures["bad.txt"] = b"!@#\n"
        for name, encode in [("32", base64.b32encode), ("64", base64.b64encode)]:
            encoded = encode(fixtures["a.txt"])
            fixtures["base" + name + ".txt"] = encoded
            fixtures["junk" + name + ".txt"] = b"!" + encoded[:500] + b"\n@" + encoded[500:]
    for name, data in fixtures.items():
        (tree / name).write_bytes(data)
    config = bytearray()
    expected = []
    expected_effects = {}
    check_namespace = fs == "btrfs" and not upstream
    guest_bin = "/bin/" if fs == "btrfs" else "C:/bin/"
    with tempfile.TemporaryDirectory(dir=root) as sandbox:
        temp = Path(sandbox)
        for name, data in fixtures.items():
            (temp / name).write_bytes(data)
        for i, (name, args, stdin, locale) in enumerate(suite):
            stdout_name = f"q{i:03}.out"
            append = "@stdout" in args
            args = [stdout_name if a == "@stdout" else a for a in args]
            if append:
                seed = b"preserve the shared input and append target\n"
                (tree / stdout_name).write_bytes(seed)
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
                    umask=0o022,
                    cwd=temp,
                    stdout=output_file,
                    stderr=subprocess.PIPE,
                )
            result.stdout = (temp / stdout_name).read_bytes()
            expected.append(result)
            config.extend((len(args) + 1, int(append)))
            for field in [stdin, "LC_ALL=" + locale, guest_bin + name, name, *args]:
                config.extend(os.fsencode(field) + b"\0")
        if check_namespace:
            expected_effects = namespace_state(temp)
        for name, data in fixtures.items():
            if (temp / name).read_bytes() != data:
                raise RuntimeError("reference modified " + name)
    (tree / "cases.bin").write_bytes(config + b"\0")
    format_image(disk, tree, fs, helper_args, proof / (arch + "-format.log"))
    serial = proof / (arch + ".serial")
    vm = Shell(
        qemu.boot_args(kernel, arch, root / "boot", 2, mode, esp_mib=36)
        + (["-device", "virtio-rng-pci"] if upstream else [])
        + ["-drive", f"if=ide,format=raw,file={disk}"],
        serial,
        root,
    )
    try:
        vm.wait(b"A:\\> ")
        vm.command("c:")
        run_cases(vm)
    finally:
        vm.close()
    read_file = stopped_files(
        disk, fs, root / "restored", helper_args, proof / (arch + "-fsck.txt")
    )
    statuses = read_file("status.bin")
    signals = read_file("signals.bin")
    if len(statuses) != len(suite) or len(signals) != len(suite):
        raise RuntimeError(f"{arch}: incomplete wait records for {len(suite)} guest processes")
    statuses = [-signal if signal else code for code, signal in zip(statuses, signals)]
    checks = []
    for i, ((tool, args, stdin, locale), ref) in enumerate(zip(suite, expected)):
        out, err = [read_file(f"q{i:03}.{ext}") for ext in ("out", "err")]
        passed = (out, err, statuses[i]) == (ref.stdout, ref.stderr, ref.returncode)
        checks.append(
            dict(
                id=i + id_offset,
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
    for name, data in fixtures.items():
        if read_file(name) != data:
            raise RuntimeError("guest modified " + name)
    traces = serial.read_bytes()
    for name in tools:
        if (b"path=" + guest_bin.encode() + name.encode() + b"\r\n") not in traces:
            raise RuntimeError("missing guest execve trace for " + name)
    if check_namespace:
        capture = root / "restored"
        effects = namespace_state(capture / "files", capture / "archive.json")
        (proof / (arch + "-namespace.json")).write_text(json.dumps(effects, indent=2) + "\n")
        if effects != expected_effects:
            raise RuntimeError(
                f"namespace effects differ: {effects} != {expected_effects}"
            )
    result = dict(
        namespace=expected_effects,
        arch=arch,
        firmware=mode,
        filesystem=fs,
        tools=build_info,
        upstream_archive=digest(upstream.read_bytes()) if upstream else None,
        locales=sorted({case[3] for case in suite}),
        timezone="UTC",
        kernel=digest(kernel.read_bytes()),
        rustc=subprocess.check_output(["rustc", "-Vv"], text=True).strip(),
        qemu=subprocess.check_output(["qemu-system-" + qemu.ARCH[arch][0], "--version"], text=True).splitlines()[0],
        fixtures={n: digest(d) for n, d in fixtures.items()},
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
    cli.add_argument("--filesystem", choices=["fat", "btrfs"], default="fat")
    cli.add_argument(
        "--linux-kernel",
        type=Path,
        help="Linux ARM64 kernel for offline Btrfs utilities",
    )
    cli.add_argument(
        "--linux-cache",
        type=Path,
        help="cache for verified Linux Btrfs utility archives",
    )
    cli.add_argument(
        "--namespace-only",
        action="store_true",
        help="run the Btrfs link/directory cases in isolation",
    )
    selection = cli.add_mutually_exclusive_group()
    for flag, name in [("--text-tools","text"), ("--data-tools","data")]:
        selection.add_argument(flag, action="store_const", const=name, dest="upstream_set")
    cli.add_argument("--upstream-archive", type=Path, help="test Rust std utilities built from the pinned uutils archive")
    args = cli.parse_args()
    if args.upstream_set and not args.upstream_archive:
        cli.error("upstream test groups require --upstream-archive")
    if args.upstream_set == "data" and args.filesystem != "btrfs":
        cli.error("this utility group needs the POSIX Btrfs root")
    if args.upstream_archive and args.namespace_only:
        cli.error("select either upstream encoders or namespace tools")
    if args.namespace_only and args.filesystem != "btrfs":
        cli.error("namespace tests require the Btrfs root")
    helper_args = []
    if args.linux_kernel:
        helper_args += ["--kernel", args.linux_kernel]
    if args.linux_cache:
        helper_args += ["--cache", args.linux_cache]
    for arch in args.arch or qemu.ARCH:
        with tempfile.TemporaryDirectory(
            prefix="coreutils-" + arch + "-", dir=args.ram_dir
        ) as tmp:
            run(
                arch,
                Path(tmp),
                args.proof,
                args.gnu_bin.resolve(),
                args.filesystem,
                helper_args,
                args.namespace_only,
                args.upstream_archive,
                args.upstream_set or "encoding",
            )

#!/usr/bin/env python3
"""Build pinned libc-test cases as separate static DOS-RS ELF programs."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
PIN = json.loads((Path(__file__).parent / "upstream.json").read_text())
DEFAULT = tuple("functional/" + name for name in (
    "basename", "dirname", "env", "fnmatch", "iconv_open", "inet_pton", "memstream",
    "qsort", "random", "search_hsearch", "search_insque", "search_lsearch", "search_tsearch",
    "setjmp", "snprintf", "sscanf", "sscanf_long", "string", "string_memcpy", "string_memmem",
    "string_memset", "string_strchr", "string_strcspn", "string_strstr", "strtod", "strtod_simple",
    "strtof", "strtol", "strtold", "swprintf", "tgmath", "udiv", "ungetc", "wcsstr", "wcstol",
))
UNSUPPORTED = {
    "functional/dlopen": "The native loader currently accepts static ELF executables.",
    "functional/tls_align_dlopen": "Runtime loading of shared ELF objects is unavailable.",
    "functional/tls_init_dlopen": "Runtime loading of shared ELF objects is unavailable.",
    "functional/ipc_msg": "System V message queue system calls are unavailable.",
    "functional/ipc_sem": "System V semaphore system calls are unavailable.",
    "functional/ipc_shm": "System V shared memory system calls are unavailable.",
}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source(archive):
    if sha(archive) != PIN["archive_sha256"]:
        raise ValueError("libc-test archive SHA-256 mismatch")
    cache = ROOT / "target/libc-test/source"
    cache.mkdir(parents=True, exist_ok=True)
    result = cache / PIN["archive_prefix"].rstrip("/")
    with (cache / "source.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if not result.exists():
            with tempfile.TemporaryDirectory(dir=cache) as temporary:
                subprocess.run(["tar", "xzf", archive], cwd=temporary, check=True)
                (Path(temporary) / result.name).rename(result)
    return result


def build(arch, archive, output, cases, jobs, linker_dir):
    src = source(archive.resolve())
    output.mkdir(parents=True, exist_ok=True)
    common = output / "common"
    common.mkdir(exist_ok=True)
    sysroot = ROOT / "target/musl" / arch / "sysroot"
    frozen = output / "runtime"
    frozen.mkdir(exist_ok=True)
    with (sysroot.parent / "build.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_SH)
        for name in ("libc.a", "crt1.o"):
            shutil.copyfile(sysroot / "lib" / name, frozen / name)
        shutil.copyfile(ROOT / "ports/musl/user.ld", frozen / "user.ld")
    gcc = arch + "-elf-gcc"
    builtin = subprocess.check_output([gcc, "-print-file-name=include"], text=True).strip()
    options = [gcc, "-O2", "-fno-pie", "-nostdinc", "-isystem", str(sysroot / "include"),
               "-isystem", builtin, "-I" + str(src / "src/common")]
    if arch == "x86_64":
        options.append("-mno-red-zone")
    common_sources = [file for file in sorted((src / "src/common").glob("*.c"))
                      if file.name != "runtest.c"]
    command = options + ["-c", *map(str, common_sources)]
    with (output / "common-build.log").open("wb") as log:
        subprocess.run(command, cwd=common, stdout=log, stderr=subprocess.STDOUT, check=True)
    library = common / "libtest.a"
    subprocess.run([os.environ.get("DOSRS_AR", arch + "-elf-ar"), "rcs", library,
                    *sorted(common.glob("*.o"))], check=True)
    link_options = [] if linker_dir is None else ["-B" + str(linker_dir) + "/", "-fuse-ld=lld"]
    selected = set(cases or DEFAULT)
    available = {str(path.relative_to(src / "src").with_suffix("")): path
                 for group in ("functional", "regression", "math", "api")
                 for path in (src / "src" / group).glob("*.c")}
    if selected - available.keys():
        raise ValueError("unknown upstream cases: " + repr(sorted(selected - available.keys())))

    def compile_one(item):
        name, path = item
        row = {"case": name, "source_sha256": sha(path), "status": "not_selected", "selected": name in selected}
        if name in UNSUPPORTED:
            return dict(row, status="unsupported", reason=UNSUPPORTED[name])
        if name not in selected:
            return row
        if path.with_suffix(".mk").exists():
            return dict(row, status="unsupported", reason="This case needs its upstream multi-file or shared-object build rule.")
        binary = output / (name.replace("/", "-") + ".elf")
        cmd = options + link_options + ["-static", "-nostdlib", "-Wl,-z,max-page-size=4096",
            "-Wl,-z,noexecstack", "-Wl,-T," + str(frozen / "user.ld"),
            str(frozen / "crt1.o"), str(path), str(library), "-Wl,--start-group",
            str(frozen / "libc.a"), "-lgcc", "-Wl,--end-group", "-o", str(binary)]
        with binary.with_suffix(".build.log").open("wb") as log:
            result = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT)
        row.update(command=cmd, build_status=result.returncode)
        if result.returncode:
            row["status"] = "build_failed"
        else:
            row.update(status="built", elf=str(binary), elf_sha256=sha(binary))
        return row

    with ThreadPoolExecutor(max_workers=jobs) as pool:
        results = list(pool.map(compile_one, sorted(available.items())))
    manifest = {"upstream": PIN, "arch": arch, "kernel_bits": 32 if arch == "i686" else 64,
                "compiler": subprocess.check_output([gcc, "--version"], text=True).splitlines()[0],
                "linker_options": link_options,
                "runtime_sha256": {name: sha(frozen / name) for name in ("libc.a", "crt1.o", "user.ld")},
                "cases": results}
    (output / "build.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"{arch}: {sum(row['status'] == 'built' for row in results)} independent libc-test ELF files built", flush=True)
    return manifest


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--arch", required=True, choices=("i686", "x86_64"))
    cli.add_argument("--archive", required=True, type=Path)
    cli.add_argument("--output", required=True, type=Path)
    cli.add_argument("--jobs", default=2, type=int, choices=range(1, 9))
    cli.add_argument("--linker-dir", type=Path)
    cli.add_argument("cases", nargs="*")
    args = cli.parse_args()
    manifest = build(args.arch, args.archive, args.output.resolve(), args.cases, args.jobs, args.linker_dir)
    if any(row["status"] == "build_failed" for row in manifest["cases"]):
        raise SystemExit(1)

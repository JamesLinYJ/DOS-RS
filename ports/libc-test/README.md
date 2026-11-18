# Upstream libc-test on DOS-RS

The source is pinned to Szabolcs Nagy's `libc-test` commit `63e36e934e8f2bedf645303a90b64098e2af6c8f`, dated 2026-11-01. The musl project identifies this repository as its libc-agnostic test suite. The exact commit, tree and archive digest are recorded in `upstream.json`. [musl test documentation](https://wiki.musl-libc.org/writing-tests)

Generate the fixed archive from the upstream Git repository.

```sh
git clone https://repo.or.cz/libc-test.git target/upstream/libc-test-git
git -C target/upstream/libc-test-git archive --format=tar.gz --prefix=libc-test-63e36e9/ 63e36e934e8f2bedf645303a90b64098e2af6c8f > target/upstream/libc-test-63e36e9.tar.gz
```

Build the DOS-RS musl sysroot first. Each selected upstream C file keeps its original `main` and assertions and becomes its own static ELF. The build records compiler commands, source and ELF hashes, and a frozen copy of the linked musl library, CRT and linker script. An optional `--linker-dir` selects the directory containing `ld.lld` through GCC's `-B` and `-fuse-ld=lld` options.

```sh
python3 ports/libc-test/build.py --arch i686 --archive target/upstream/libc-test-63e36e9.tar.gz --output target/libc-test/i686
python3 ports/libc-test/build.py --arch x86_64 --archive target/upstream/libc-test-63e36e9.tar.gz --output target/libc-test/x86_64
```

The initial selection contains 35 functional cases. Additional cases can be named after the options, for example `functional/pthread_cond`. `build.json` also lists every upstream functional, regression, math and API source outside that selection. It distinguishes build failures, unselected cases and unsupported dependencies. Shared-object build rules and System V IPC dependencies have explicit reasons.

Build the upstream Rust `timeout` tool for each architecture with `scripts/build-uutils.py`. Pass the directory containing its `i686/timeout` and `x86_64/timeout` binaries to the guest runner. Each test has a separate process and a deadline. The runner uses a Btrfs root and puts every writable guest image in the supplied RAM directory.

```sh
python3 scripts/qemu-libc-test.py --arch i686 --arch x86_64 --build-dir target/libc-test --timeout-dir target/uutils --ram-dir /path/to/ram --proof target/libc-test-proof
```

`--kernel` accepts one frozen kernel with exactly one `--arch`. Every result identifies the executing 32-bit i686 BIOS or 64-bit x86_64 UEFI kernel. The 16-bit DOS line has its own acceptance suite.

The test root uses the same `/dev/null` device seeder as the installed system. For cases that invoke other programs, pass `--programs DIR` with executable dependencies at `DIR/i686/PROGRAM` and `DIR/x86_64/PROGRAM`. The runner records their SHA-256 values. The `popen`, `spawn` and `vfork` cases use the installed Rust `sh`, `echo`, `test` and `kill` executables.

The guest records stdout, stderr and wait status for every case. After shutdown, Linux btrfs-progs checks and recovers the disk. A passing case must have zero exit status, empty output as required by upstream, and a trace confirming its own ELF path. Failures and timeouts remain in the result alongside successful cases. Issue 8 ledger updates require inspection of the assertions and their actual guest results. The full POSIX inventory remains the completion criterion.

The upstream archive carries `COPYRIGHT` and `AUTHORS`. Its principal license is MIT. Some math-vector directories carry additional BSD or GPL notices, which remain in the pinned source archive.

The first 35 selected programs passed on the 32-bit i686 BIOS and 64-bit x86_64 UEFI kernels. The upstream long-input scanner processes an 8 MiB string with an enforced 100 KiB stack limit. The initial run exposed ENOSYS from getrlimit on both targets, and the final run uses the process-owned RLIMIT_STACK implementation. Both stopped Btrfs disks passed the Linux check. The inventory has 426 source entries, with 35 selected, six explicit unsupported entries and 385 unselected entries.

# Upstream Rust utilities

Build static ELF programs for DOS-RS from the [uutils 0.12.0 source archive](https://github.com/uutils/coreutils/archive/refs/tags/0.12.0.tar.gz). The builder verifies SHA-256 `4fb327655cb4ffcbf2f16550cf9234079ffe839692f7aa1a6eda104af684e122` and uses the release Cargo.lock. The project pins Rust in rust-toolchain.toml. Install its rust-src component before building the target standard library.

The i686 and x86_64 tools link Rust std, the DOS-RS musl startup object and the DOS-RS syscall implementation at static link time. Rustix calls the target libc. The repository includes the upstream license and the small GNU 9.12 diagnostics patch used for decoding failures.

Build the musl sysroot first. A first online build populates Cargo's dependency cache. Use `--offline` for subsequent builds on a machine with that cache.

```sh
python3 ports/musl/build.py --arch i686 --archive "$MUSL_ARCHIVE"
python3 scripts/build-uutils.py --arch i686 --archive "$UUTILS_ARCHIVE" --output target/posix-root/bin base32 base64
python3 scripts/build-rust-tools.py --arch i686 --uutils-archive "$UUTILS_ARCHIVE" --output target/posix-root/bin
```

Use `--arch x86_64` for the 64-bit root. A 16-bit DOS boot volume uses the separate native command line and DOS executable format. The differential test saves results per ELF, target, argument list, locale and input under the chosen proof directory.

```sh
python3 scripts/qemu-coreutils.py --ram-dir "$RAM_DIR" --gnu-bin "$GNU_BIN" --upstream-archive "$UUTILS_ARCHIVE" --proof target/uutils-proof
```

The text utility differential set covers comm, cut, expand, fold, nl, paste, printf, seq, sort, tr, unexpand and uniq. Add `--text-tools` to the differential command to run this set. Named input to sort exercises access(2) as well as file I/O. The encoders and these twelve utilities supply independent ELF entry points on each target.

Use `--filesystem btrfs --data-tools` for 42 GNU comparisons covering 17 additional numerical, text and checksum commands. The default installer selection includes these programs. The entropy-dependent commands use native getrandom. The Btrfs suite verifies long executable names on disk.

The arch and uname ELF programs query the running DOS-RS kernel. The uname OS field comes from that kernel, the machine field follows its 32-bit or 64-bit build, and the nodename can be updated by root for the current boot. scripts/qemu-uname.py checks the fields independently with musl, then verifies GNU field ordering.

Use `--filesystem btrfs --file-tools` for the 27 cases that exercise chmod, cp, du, install, ln, ls, mv, readlink, rm, split, stat, tee, touch and truncate. Both target kernels run the individual ELF programs. A Linux recovery guest reads the stopped Btrfs volume, and the host compares modes, file bytes, symlink targets and the explicit modification timestamp with GNU's results. On i686 the Rust libc dependency uses musl 1.2's 64-bit time layout.

The nproc ELF uses the running kernel's online CPU mask. scripts/qemu-affinity.py boots each ELF architecture with one and four vCPUs, verifies musl's sched_getaffinity and sysconf, queries another process and its worker TID, then checks nproc, --all and --ignore. The affinity API reports all online scheduler slots for each live task.

The DOS-RS `df` port enumerates real VFS mounts through native syscall 164. Each result contains four NUL-terminated strings for the source, canonical mount point, filesystem type and mount options. The index starts at zero. The syscall returns the packed byte length, zero after the final mount, or `ERANGE` for a short buffer. The Btrfs source uses its on-disk UUID. DOS A: and B: address the same boot volume and appear as a single mount in df. Applications can open both names under /dos.

To compare capacity with Linux, build the pinned GNU coreutils 9.12 source using Zig, LLVM ar/ranlib and GNU make, then run the native DOS-RS and Linux guests on independent RAM copies of a pre-mounted GPT fixture. [GNU release archive](https://ftp.gnu.org/gnu/coreutils/coreutils-9.12.tar.xz)

```sh
python3 scripts/build-gnu-df.py --archive "$GNU_COREUTILS_ARCHIVE" --output target/gnu-df
python3 scripts/qemu-df.py --archive "$UUTILS_ARCHIVE" --gnu-df target/gnu-df --ram-dir "$RAM_DIR"
```

The JSON records both versions of df and both kernels' statvfs counters. FAT capacity, used bytes, available bytes and use percentage match across the kernels. Btrfs total size matches. Linux includes a running global reservation in used space. The DOS-RS allocator excludes the physical MiB containing the mirrored superblock when claiming a new data chunk. These filesystem policies affect the available-space figures reported by their respective tools.

Run `--filesystem btrfs --tree-tools` for 20 comparisons across nested directories. The cases cover recursive copy, symlink traversal, rename and removal, byte offsets in dd, portable-name checks and shred's logical zero pass. The stopped volume supplies the file hashes and permissions. The 32-bit and 64-bit stack regressions replay the exact previously built shred executables via `--replay-elf-dir`.

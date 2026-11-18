# Native userspace Shell fixtures

Run `python3 scripts/qemu-user-shell.py --ram-dir /path/to/ramdisk --proof /path/to/proof` from the repository root. The runner boots the i686 BIOS kernel and the x86_64 UEFI kernel, executes the independent `/bin/sh` ELF, checks job control and a DOS COM, then compares files from the stopped FAT disk and runs `fsck.fat -n`. Use `--arch`, `--kernel` and `--programs` together to select one architecture's fixed binaries. Use `--probes` to reuse that same architecture's `execprog` and `dirprobe` ELF files; their hashes are recorded too. Record the source revision and hashes of all reused inputs.

These fixtures exercise the 32-bit and 64-bit userspace Shell. The native 16-bit `COMMAND.COM` has separate regressions in `scripts/realmode-command.py` and `scripts/realmode-shell.py`.

| Fixture | Main checks | Exit status |
| --- | --- | --- |
| `source.sh` | Dot PATH search, shared variables/functions/directory/positionals, empty files, last status, nested return, stdin separation, close-on-exec descriptors, eval, errors and 256 repeated loads | 53 |
| `source-args.sh` | Optional dot arguments, preserved `$0`, nested argument scopes and restoration after return | 59 |
| `source-errors.sh` | Fatal special built-in redirection errors and nonfatal ordinary command errors | 61 |
| `command.sh` | Function bypass, current-shell builtins, command categories and default PATH including readonly PATH | 65 |
| `command-state.sh` | Declaration expansion, temporary assignments, dot/eval/return/exit, permanent exec redirection, pipelines, background execution, path queries and lookup failures | 67 |
| `command-errors.sh` | Wrapped builtin errors, invalid command options and nested evaluation error recovery | 71 |

PATH tests use `/bin/echo` explicitly while the search path excludes `/bin`; the native echo utility is an independent executable.

The runner installs `source-args.sh` and `source-errors.sh` as `srcargs.sh` and `srcerr.sh` for FAT short names. It also loads a 16 KiB here-document with dot, checks interactive error recovery, and retains every stopped output when a comparison fails.

Dot and eval follow [POSIX.1-2024 Shell Command Language](https://pubs.opengroup.org/onlinepubs/9799919799/utilities/V3_chap02.html#tag_19_19). Dot streams complete commands; eval parses the complete joined argument string. Syntax errors terminate a non-interactive shell. Dot does not implicitly search the current directory when PATH omits it. Optional dot arguments use the KornShell extension: they temporarily replace positional parameters while keeping `$0`, including across nested dot calls. Without optional operands, `set` and `shift` update the caller's parameters.

The source fixture's expected output is checked against a host dash with **child `sh` pinned to the same interpreter**. Keep raw differences: macOS's supplied dash returns success for dot with no operand and for a directory; DOS-RS reports errors for both. The argument extension is compared with KornShell; macOS Bash has different restoration behavior after `set --`. These reference differences must not be normalized into a claim of full Shell conformance. The native Shell still needs separate coverage for traps and other incomplete POSIX requirements.

The command fixtures follow [Issue 8 command](https://pubs.opengroup.org/onlinepubs/9799919799/utilities/command.html), including recursive declaration handling. They run on disk as `command.sh`, `cmdstate.sh` and `cmderror.sh`. Lookup uses `/bin` for `-p`; it leaves the caller's PATH and the external program's environment unchanged. Queries identify existing shell functions, builtins, reserved words and executable files. Multiple query operands are an extension; any missing name gives a nonzero result. Conflicting `-v`/`-V` and missing query operands return 127. Without operands, plain `command` returns zero.

Reference implementations differ on relative query output, wrapped failed `exec`, option errors, default PATH and declaration expansion. Keep those outputs instead of deriving golden files from one host shell. The fixtures require an absolute `-v` pathname and separate an error in wrapped dot/eval from an explicit `exit` inside that evaluation. Alias definitions and locale-specific diagnostics remain unimplemented; the FAT suite does not exercise a full executable-permission matrix on Btrfs.

The runner preloads the empty directory `execdir/plaincmd` for the raw libc exec probe. Native POSIX `mkdir` currently returns ENOTSUP on FAT, so executable-directory tests must not depend on creating that fixture through this syscall. The probe checks its type, rejects directories with EACCES through direct `execve`, then searches past the directory to a regular text script. Keep the Btrfs-root run as a separate filesystem acceptance result. Query behavior across symlink loops and I/O failures still needs its own matrix.

The initial native Shell does not yet reliably initialize `PWD` when the DOS launcher provides no value. Directory fixtures save their starting location with `pwd`; this does not count as acceptance of the inherited PWD rules.

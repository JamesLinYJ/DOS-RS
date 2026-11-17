# Static musl for DOS-RS

The i686 and x86_64 kernels can load static musl 1.2.6 executables. The sysroot contains the upstream C and POSIX headers, crt1.o and libc.a. The native 16-bit DOS kernel uses its existing COM and MZ program ABI.

Download [musl 1.2.6](https://musl.libc.org/releases/musl-1.2.6.tar.gz). The build checks SHA-256 `d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a`. [COPYRIGHT](COPYRIGHT) preserves the upstream notices. The build applies the overlay to an extracted copy under `target/musl/<arch>`. It needs the project's prepared CPU libraries, GNU Make and the corresponding ELF cross compiler.

```sh
python3 ports/musl/build.py --arch i686 --archive /path/to/musl-1.2.6.tar.gz
python3 ports/musl/cc.py --arch i686 -o hello.elf hello.c
python3 ports/musl/test.py --archive /path/to/musl-1.2.6.tar.gz --ram-dir /path/to/ram-volume
```

Use `--arch x86_64` for ELF64. The test defaults to both targets, with i686 legacy BIOS and x86_64 UEFI. It boots four vCPUs, runs an independently linked C application from C:, checks its exit status, then compares the contents written through stdio after the VM stops.

The loader supplies argv, envp and the ELF program headers in the auxiliary vector. Upstream musl initializes static TLS and the C runtime. The native thread stores the i686 GS or x86_64 FS base across scheduling, fork and signal delivery. Musl's pthread implementation uses kernel threads, futexes, guard-page protection and thread-exit notification. System calls use the public DOS-RS INT 40h ABI.

The executable test covers initialized and zero-filled TLS, distinct pthread errno, condition variables, deferred cancellation with cleanup handlers, heap allocation, stdio, file metadata, fork/exec/wait with an environment and close-on-exec descriptors, monotonic and realtime clocks, timed waits, signal interruption of sleep, and poll/ppoll including atomic signal-mask restoration. The Btrfs fixture's `files.c` exercises directories, permission bits, hard and symbolic links, and unlink of an open file. The static port returns an error for interfaces without a corresponding kernel service. Dynamic ELF loading and shared objects are outside the static sysroot.

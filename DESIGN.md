## Problem

The first pull request boots one `no_std` kernel on `i686-unknown-none` and on `x86_64-unknown-none`, and it serves INT 21h AH=30h with the DOS 4.0 register contract. The CPU trap frame is a different shape on each target. i686 ring 0 pushes EFLAGS, CS, and EIP. x86_64 pushes SS, RSP, RFLAGS, CS, and RIP in 8-byte slots. The DOS contract is one set of registers on both. QEMU `isa-debug-exit` cannot return process status 0, so the smoke script maps the device status.

## Usage (caller's view)

You boot the kernel. The copied multiboot trampoline calls `kmain`. That function is the only boot entry in the binary. It masks both PIC controllers, loads an IDT, prints a banner on COM1, and executes `int 0x21` with AH set to `0x30`.

```rust
let gate = arch::gate_addr();
mask_pic();
load_idt(gate);
puts("dos4-rs\n");
invoke_int21();
puts_result();
debug_exit();
```

`invoke_int21` sets carry, then executes `int 0x21`. It does not call `int21::entry`. The gate maps the trap frame, calls `int21::entry`, and maps the logical frame back. After the instruction, the low 16 bits of AX, BX, and CX are `0x0004`, `0xFF00`, and `0x0000`. Carry is still set. The serial line prints those three words and CF. The smoke script treats QEMU status 33 as success. That status is what a write of `0x10` to port `0xF4` produces.

You add the next INT 21h function by replacing one empty slot in `int21`. The index is the function number. You do not record that number anywhere else.

```rust
fn print_string(frame: &mut LogicalFrame) {
    frame.set_al(Al::ZERO);
}

slots[0x09] = print_string;
```

`MAXCOM` stays `DISPATCH.len() - 1`. The const assert still requires that derived value to equal `0x6C`. A 16-bit result is a `DosWord`. The store methods do not accept a `u64`.

You add a BIOS file by creating the path the checklist already names. This pull request does not create the file, and it does not add a crate. `kmain` stays the only boot entry. The new file is a private module that a later `kmain` step can call. It does not install its own IDT gate.

```rust
pub(crate) fn con_write(bytes: &[u8]) {
    let _ = bytes;
}
```

That function is the future body of `src/bios/mscon.rs`.

Each of the 1280 paths in `/tmp/msdos-files.txt` maps to one future module path string. Drop the `v4.0/src/` prefix. Lowercase the remaining path. Replace the extension with `.rs`. A name with no extension gains `.rs`. `v4.0/src/DOS/GETSET.ASM` maps to `src/dos/getset.rs`. `v4.0/src/BIOS/MSCON.ASM` maps to `src/bios/mscon.rs`. `v4.0/src/BIOS/LOCSCR` maps to `src/bios/locscr.rs`. `v4.0/src/SETENV.BAT` maps to `src/setenv.rs`. The string is not a file in this pull request. AH=30h stays in `src/int21.rs` until a later change moves that body onto its checklist path. The table slot stays in `int21`.

## Shape

```
Cargo.toml                 package dos4-rs, edition 2021, core only
rust-toolchain.toml        nightly-2026-11-08, copied from the spike
i686-unknown-none.json     custom i686 target, copied from the spike
.cargo/config.toml         build-std and the x86_64 static flags, copied
build.rs                   linker script argument, copied
link.ld                    one multiboot image at 1 MiB, copied
src/main.rs                kmain, PIC mask, IDT, COM1, debug exit
src/arch.rs                trampoline, trap frames, two mappers, naked gate
src/int21.rs               logical frame, dispatch table, AH=30h, both stubs
```

`LogicalFrame` is the `user_environ` analogue. The arch gate builds one local for the duration of the interrupt. Slots are `u64` on both targets. The fields are AX, BX, CX, DX, SI, DI, BP, DS, and ES. There is no flags field. Carry remains in the trap frame, so a handler cannot clear it.

`DosWord`, `Al`, `Ah`, and `ErrorCode` are not `u64` values. `store_ax` takes a `DosWord`. `set_al` takes an `Al`. No store takes a raw `u64`. An `ErrorCode` does not fit in `store_ax`.

`arch` owns the copied multiboot header, the copied `_start` trampoline, and the two functions `logical_from_trap` and `logical_into_trap`, per boundary-discipline. The i686 mapper zero-extends each 32-bit GPR and copies DS and ES. The x86_64 mapper copies each GPR as `u64` and zero-extends DS and ES. Neither mapper copies EFLAGS, RFLAGS, the return address, CS, or the x86_64 interrupt stack pointer. Writeback on i686 stores the low 32 bits of each GPR. Writeback on x86_64 stores the full `u64`. Neither writeback stores flags.

`GateToken` is minted only in `arch`. `int21::entry` requires one. `kmain` therefore cannot call `int21::entry`. The only invocation is `int 0x21`. The IDT target is a naked stub. A Rust prologue would clobber the GPRs the mapper reads. `gate_addr` returns that stub as an integer so `kmain` can install it without naming the trap type.

`int21` owns the logical frame, the table, AH=30h, the in-range stub, and the out-of-range check. `DISPATCH` is one array of function pointers. Every slot starts as `in_range_stub`. Slot `0x30` is `get_version`. `MAXCOM` is `DISPATCH.len() - 1`. A const assert requires `MAXCOM == 0x6C`. That assert is the DOS 4.0 bound. It is not a second catalog. AH above `MAXCOM` sets AL to 0 and returns. That path does not build an `ErrorCode` and cannot touch INDOS, because this module has no INDOS byte. Every empty slot points at the same stub, which also sets AL to 0.

`get_version` ignores AL. It stores `0x0004`, `0xFF00`, and `0x0000` in the low 16 bits of AX, BX, and CX. It does not write DX, SI, DI, BP, DS, or ES.

`kmain` owns the PIC step. It writes `0xFF` to port `0x21` and to port `0xA1` before `lidt`. Port `0x21` is the master PIC mask. Vector `0x21` is the IDT index. A second pass writes the same masks and the same gate, so the boot step converges if it runs again, per make-operations-idempotent.

The linked binary exports `kmain`. Inside the crate, `kmain` sees the gate only as an integer address. Callers do not see the version constants, the table, COM1, the IDT bytes, or the trap struct. A translator adding a function sees one array slot and the frame stores.

This pull request does not translate BIOS, MSDOS.SYS, COMMAND.COM, or the utilities. It does not create those files. It does not walk a FAT, and it does not implement the version lie.

## Synthesis decision

The base is the one-table sketch from `grok-4.7-xhigh`. `kmain` and vector 0x21 are the only entries, and `GateToken` stops `kmain` from calling the dispatcher. The layered sketch from `cursor-grok-4.6-xhigh` built both targets. It lost because it re-exported `int21` and `Registers`, so a later edit could skip the interrupt, and its checklist was a second list of directory prefixes. Grafted from that sketch: the caller sets carry before `int 0x21` and the serial line prints CF. The cross-judge did not run. Claude, GPT, Gemini, and Muse were rejected for weekly usage, and this parent is already on Grok.

## Tradeoffs accepted

- We accept a crate-visible frame type in exchange for one definition the mapper and `int21::entry` can both name. Rust cannot reveal a type to one sibling and hide it from the parent module. `GateToken` blocks the call from `kmain`.
- We accept i686 bits 32 through 63 living only inside `LogicalFrame` in exchange for one internal width on both targets. Writeback keeps the low 32 bits, which are the real register.
- We accept a fully masked PIC for this boot in exchange for keeping IRQ1 off vector `0x21` without choosing a new vector base.
- We accept hardcoded AX `0x0004`, BX `0xFF00`, and CX `0x0000` in exchange for leaving the version lie and a programmable serial out of this pull request.
- We accept checklist strings with no files behind them in exchange for one row per upstream path without creating modules this pull request does not translate.

## Alternatives considered

Three crates named `bios`, `dos`, and `cmd` would each publish an init and a copy of the register file, and the boot crate would publish the trap frame so the others could finish one syscall. A caller would have to know the trap layout, the version word, and which crate owns AH=30h. That is a larger set of public functions hiding less. The table shape keeps one boot entry and one gate, and it keeps the trap layout in `arch`.

## Open questions and risks

- Should the smoke script treat a write of `0x10`, and QEMU status 33, as the only success status?
- The upstream image leaves the 24-bit `USERNUM` at zero. Should CX stay `0x0000` for the life of the port?
- Masking both PICs blocks every IRQ, not only IRQ1. Is that acceptable until a later pull request needs a device interrupt?
- Headers and `.INC` files each receive their own future path. Should a header row name the module that includes it?

## Next implementation step

Fill in the PIC mask, the IDT, the naked `int 0x21` gate, COM1, and the `0x10` debug exit, then add the smoke script that maps QEMU status 33 to process status 0.

## Console slice

`src/bios/mscon.rs` is `BIOS/MSCON.ASM`. `CON$WRIT` executes `int 0x29` for each byte. That gate is `OUTCHR` from `BIOS/MSBIO1.ASM`. `OUTCHR` executes `int 0x10` with AH=`0x0E` and BX=7. AH=`0x0E` writes AL to COM1. Packet offsets come from `BIOS/MSEQU.INC`. Media is 13, the transfer address is 14, and the count is 18. After AH=30h the kernel writes `BIOS` and CR LF through command 8. `BIOS/MSCON.ASM` is translated. `BIOS/MSBIO1.ASM` is partial, because only `OUTCHR` is taken from it.

## Guest 8086

Both boot targets run the same interpreter. x86_64 has no virtual-8086 mode, so a second engine on i686 would split the DOS contract. Compat16 already runs guest code on a host that is not the guest CPU. This kernel takes that split and leaves the rest.

Taken from [Compat16](https://github.com/JamesLinYJ/Compat16), read-only, and not modified:

- `crates/guest` `Registers` is one owned struct. Guest numbers are never host registers. The kernel stores the same eight words, four selectors, the instruction pointer, and the flags in `CpuState`. The field names here are roles, because register names stay in `src/arch.rs`.
- `crates/cpu` `RealModeSegments` forms a linear address as `segment * 16 + offset`, with a 16-bit offset. A failed add is an error, not a wrapped host pointer. `GuestMemory` checks the range before it reads or writes. `src/guest/memory.rs` keeps the whole real-mode megabyte. The twentieth address bit stays undriven, so a sum past `0xFFFFF` folds back into that megabyte. Conventional memory is the first 640KiB: a new program prefix is told that memory ends at paragraph `0xA000`, and allocation stops there. The bytes from `0xA0000` through `0xFFFFF` stay addressable for segment arithmetic and are not offered as free DOS memory.
- `StepOutcome::Interrupt` means the instruction pointer already sits past the interrupt instruction. The owner, not the decoder, performs the service. `INT 21h` in `src/guest/run.rs` is that owner: print one byte, print a dollar-terminated string, or stop with the low half of the accumulator as the return code.
- `read_register` / `write_register` select a word by a three-bit index and a width. Byte indices 0 through 3 are the low half. Indices 4 through 7 are the high half. The same index map is in `CpuState`.

Not taken: the `iced-x86` decoder, the Cranelift JIT, protected mode, x87, and the Windows host. Those need `std` and do not fit this `no_std` image. Operand reads that cross offset `0xFFFF` wrap inside the segment. Compat16 adds the 16-bit offset once and then reads a linear word, which steps into the next paragraph. Programs in the guest image do not rely on that boundary.

`disks/guest12.img` is a FAT12 volume. The directory entry `HELLO   COM` is cluster 2. The loader copies those bytes to offset `0x100` in a program segment prefix and runs them. The program adds 6 to the letter A, prints that byte, then prints the rest of the line. The serial check is `GUEST HELLO COM`.

`HELLO   EXE` is cluster 3. The loader places the image 16 paragraphs above the prefix and adds that load segment to each relocation word. The program then copies the fixed word into the data selector and prints `HELLO EXE`.

`RUN.COM` opens `DATA.TXT`, reads one byte, writes it after `FILE `, then loads `CHILD.COM` with function `4Bh`. The child stops with function `4Ch` and code `7`. The parent reads that code with function `4Dh` and prints `FILE Z EXEC 7`. Handles, the saved return code, and the child register file are owned by the guest session. The parent register file is left in place.

`COMPRESS.COM` is the SELECT panel compressor from Microsoft MS-DOS 4.0, file `v4.0/src/TOOLS/COMPRESS.COM` in the [MIT-licensed](https://github.com/microsoft/MS-DOS/blob/main/LICENSE) `microsoft/MS-DOS` tree (Copyright (c) Microsoft Corporation). The guest image also carries `SEL-PAN.DAT`, five letter A bytes. The program opens that file, writes `SELECT.DAT`, and the kernel prints `COMPRESS FF0541`. The shell also launches `COMPRESS.COM` by name. That run creates the same file: the batch lists it as `CMD DIR SELECT.DAT` and types the bytes `FF 05 41`.

`EXE2BIN.EXE` is the same tree's converter, `v4.0/src/TOOLS/EXE2BIN.EXE`, under that same MIT license. With an empty command tail it prints `File not found`. The kernel writes `EXE2BIN ` first, so the serial check is `EXE2BIN File not found`.

`EXEC.EXE` is the Microsoft C 5.10 launcher from that tree, `v4.0/src/TOOLS/EXEC.EXE`, under the same MIT license. The prefix carries `PATH=\` and the tail ` HELLO.COM`. Its child exec loads that program, and the serial log shows `GUEST HELLO COM` from the child.

`DBOF.EXE` is the dump-binary tool from that same tree, `v4.0/src/TOOLS/DBOF.EXE`, under the same MIT license. With an empty command tail it prints `DBOF Syntax: DBOF Source-file Target-file Hex-start Hex-length`.

`EXEFIX.EXE` is the header fixer from that same tree, `v4.0/src/TOOLS/EXEFIX.EXE`, under the same MIT license. With an empty command tail it prints `usage: exefix <file> <max-alloc> <min-alloc>`.

`CONVERT.EXE` is the file converter from that same tree, `v4.0/src/TOOLS/CONVERT.EXE`, under the same MIT license. With an empty command tail it prints `Usage: Convert [file]`.

`ATTRIB.EXE` is the attribute utility from that same tree, `v4.0/src/TOOLS/ATTRIB.EXE`, under the same MIT license. The tail ` HELLO.COM` makes it read that file's attribute and print `  A       A:\\HELLO.COM`.

`EXEMOD.EXE` is the header utility from that same tree, `v4.0/src/TOOLS/EXEMOD.EXE`, under the same MIT license. The tail ` HELLO.EXE /h` makes it print that file's header, including `.EXE size (bytes)                     3D              61`.

Opcode `8D` with a register source is not a memory operand. Compat16's `execute_lea` returns an error for that form. The 8086 manual only defines `8D` for a memory source. On 8086 and 8088 silicon the same encoding copies the most recent effective-address offset into the destination, because the address unit does not replace that offset when the source is a register. The guest follows the silicon.

`INDEXMAE.SYS` in `v4.0/src/DEV/XMAEM` is the remaining unused upstream binary. It is not a device image SYSINIT can call. The file is 1189 bytes, the attribute word `0C040h` and the name `386XMAEM` are not a header at offset 0, and the apparent message pointer is past the end of the file. The driver's own init, after any banner, checks for a PS/2 model 80, reads the machine status word, and then builds protected-mode tables for XMA hardware this guest does not have. No success line is invented for it. Guest programs do call INT 21h function 0Eh to select a drive. Drives 0 and 1 name this volume; any other number leaves the current drive unchanged. The call returns two usable drive letters and does not set carry. Function 19h reports the drive that 0Eh selected. Function 29h parses a file name into an FCB: a drive letter is stored as 1 for A, `*` fills the rest of that field with `?`, and the result is 1 when a wildcard was present. Function 33h reads and stores the control-break flag, stores the extended break flag, and reports drive 1 as the boot drive. `PARSE.COM` prints `PARSE 2 HELLO    COM 0`, `WILD 1 ???????? ???`, and `CTRL 0 1 BOOT 1`. Function 59h returns the extended error recorded by the last failed call: class, action, and locus come from the DOS error table, and a missing file is code 2, class 8, action 3, locus 2. Function 62h returns the program segment prefix of the program that is running. Function 46h forces one open handle onto another, closing the target first when the two differ. Function 87h is not a DOS 4.0 call, so it returns invalid function with carry set. `CALLS.COM` prints `EXT 0002 08 03 02`, `PSP 0B00`, `DUP2 OK`, and `FN87 0001 1`.

`EXEPACK.EXE` is the compressor from that same tree, `v4.0/src/TOOLS/EXEPACK.EXE`, under the same MIT license. The shell runs `EXEPACK.EXE EXEFIX.EXE PACKED.EXE`. The written file is 0x2A00 bytes and begins with the MZ mark, and it contains the packer's stub text. The serial line is `PACKED 2A00 4D5A OK`. The directory lists `CMD DIR PACKED.EXE`. A far allocation that used to alias the program image now keeps those instruction bytes, so the compressor returns to the shell. The shell does not print `CMD ERRORLEVEL /`. The shell then runs `PACKED.EXE` with an empty tail. The stub unpacks it and the program prints `usage: exefix <file> <max-alloc> <min-alloc>`. It also copies `HELLO.EXE` to `FIX2.EXE` and runs `PACKED.EXE FIX2.EXE 0ABC 0020`. The unpacked image rewrites that header. The serial line is `PACKFIX 0ABC 0020`, and the directory line is `CMD DIR FIX2.EXE`. The unpacked tool's `EXEFIX 1234 0010` on `PACKME.EXE` stays.

`BUILDIDX.EXE` is the message-index builder from that same tree, `v4.0/src/TOOLS/BUILDIDX.EXE`, under the same MIT license. The shell runs it with an empty tail, and it prints `Incorrect number of parms  given to BUILDIDX.`

`BUILDMSG.EXE` is the message builder from that same tree, `v4.0/src/TOOLS/BUILDMSG.EXE`, under the same MIT license. The shell runs it with an empty tail. It prints `BuildMsg - PSP at 0b00` and `Allocatable memory size is 606208`, then returns status 1. The segment is the program prefix function 62h returns.

`MENUBLD.EXE` is the menu builder from that same tree, `v4.0/src/TOOLS/MENUBLD.EXE`, under the same MIT license. The shell runs it with an empty tail. It prints `Couldn't open  menu creation files.` and returns status 1.

`NOSRVBLD.EXE` is the message-file builder from that same tree, `v4.0/src/TOOLS/NOSRVBLD.EXE`, under the same MIT license. The file allocation table is two sectors, so this tool still fits beside the packed output. The shell runs it with an empty tail. It prints `At lest one filespec ( for .SKL ) needed.` and returns status 1.

Repeated word string operations `F3 AB`, `F3 A7`, `F3 AD`, and `F3 AF` store, compare, load, and scan a word on each count. `CL.EXE` is the Microsoft C 5.10 compiler driver from that same tree, `v4.0/src/TOOLS/CL.EXE`, under the same MIT license. The shell runs it with an empty tail. It prints `usage: cl [ option... ] filename... [ -link linkoption... ]` and returns status 1.

INT 21h function 36h reports free and total allocation units. DOS `DISK_INFO` counts zero entries from cluster 2 through the highest cluster, and the total is one less than that highest cluster. Function 36h swaps those results, so the free count and the total are separate. The sectors-per-cluster count stays in the low half of the accumulator word. A highest cluster of 4086 or more is a 16-bit table; below that the entries are 12-bit. Drive 0 and drive 1 both name this volume. Any other drive number returns FFFF. The guest scans the mutable table, so a volume larger than 512 sectors still reports the clusters that are actually free, and EXEPACK still writes `PACKED 2A00 4D5A OK`.

`LIB.EXE` is the library manager from that same tree, `v4.0/src/TOOLS/LIB.EXE`, under the same MIT license. The shell runs `LIB.EXE LIBH.LIB, LIB.LST` on the real library `v4.0/src/TOOLS/BLD/LIB/LIBH.LIB`. It prints `Microsoft (R) Library Manager  Version 3.10`. A missing file opened with the create flag is created with the caller's attribute, and the new handle can be duplicated. The runtime then writes `LIB.LST`. The directory line is `CMD DIR LIB.LST`, and the listing begins `LIB LST __aFFaldiv`.

`ASC2HLP.EXE` is the help builder from that same tree, `v4.0/src/TOOLS/ASC2HLP.EXE`, under the same MIT license. The shell runs `ASC2HLP.EXE USA.TXT O.HLP` on the real help source `v4.0/src/SELECT/USA.TXT`. A child releases its allocation when it exits, so this builder can take the 64000-byte block it asks for. A missing output file is created with function 5Bh and is left untouched when that name already exists. It prints `The number topic(s) and text string(s) written = 57`.

The guest volume is 2513 sectors. Eight sectors of FAT hold the table, and the highest cluster stays below 4086, so the entries stay 12-bit. The extra clusters hold `MASM.EXE` and the object it writes, after the packed file, the library listing, and the help output. A shift writes the zero, sign, and parity flags from its result and leaves the carry flag as the bit that shifted out. `LINK.EXE` is the overlay linker from that same tree, `v4.0/src/TOOLS/LINK.EXE`, under the same MIT license. The shell runs `LINK.EXE SAY.OBJ,SAY.EXE;` and then `SAY.EXE`. A program larger than 32KB keeps its conventional allocations past the loaded image, the minimum extra paragraphs, and a full 64KB beyond the header stack, so the near data segment is not reused as a far heap. The linker prints `Microsoft (R) Overlay Linker  Version 3.65` and `Copyright (C) Microsoft Corp 1983-1988.  All rights reserved.` A console write returns every byte of that message. Function 47h returns the current directory without a leading slash, and the root is an empty string. `ATTRIB.EXE HELLO.COM` therefore prints `A:\HELLO.COM`. Function 4Fh continues the find started by function 4Eh. `NEXT.COM` searches `*.COM` and prints the second match, `NEXT CHILD.COM`. The found name is an ASCIIZ string; when the 13-byte field has room, a carriage return, line feed, and `$` follow the terminator so a function 09h print of that field stops after the name. `DOSCALL` therefore prints `INT FIND HELLO.COM` and then `INT CWD ROOT` on the next line. A directory entry with no date is reported as 1 January 1980, the same day function 2Ah returns, so a blank stamp is not mistaken for a missing file. The linked program prints `LINKED OK`. `EXE2BIN.EXE` then converts that program and the serial line is `EXE2BIN 0018 0E1F OK`.

`NMAKE.EXE` is the program maintenance utility from that same tree, `v4.0/src/TOOLS/NMAKE.EXE`, under the same MIT license. Images larger than 64KB are stored straight through conventional memory, so the packed stub is not wrapped over the front of the load. The shell runs `NMAKE.EXE -f MAKEFILE`. `TOOLS.INI` is the initialization file from that same tree, so the utility no longer prints `U4013`. The makefile copies `MAKEIN.TXT` to `MAKEOUT.TXT`. The utility prints `Microsoft (R) Program Maintenance Utility   Version 1.00.05`, and `TYPE MAKEOUT.TXT` prints `nmake-wrote`.

`MASM.EXE` is the macro assembler from that same tree, `v4.0/src/TOOLS/MASM.EXE`, under the same MIT license. A byte shift by one covers rotate, rotate-through-carry, logical shift, and arithmetic shift, in either direction. The shell runs `MASM.EXE MASMIN,MASMOUT,,;` on `MASMIN.ASM`, then `LINK.EXE MASMOUT.OBJ,MASMOK.EXE;` and `MASMOK.EXE`. `MASMIN.ASM` defines a stack segment, so that link does not warn `L4021`, and the linked program still prints `MASM OK`. `SAY.OBJ` stays without a stack segment. That is why `EXE2BIN` can still write the 24-byte binary, and why that link still warns `L4021: no stack segment`. The assembler prints `Microsoft (R) Macro Assembler Version 5.10`.

`C1.EXE` is the C front end from that same tree, `v4.0/src/TOOLS/C1.EXE`, under the same MIT license. A guest can keep twenty files open at once, and each open file can be as large as the small-model library. The shell runs `C1.EXE -f TINY.C`. The flag names the source, and `TYPE sy` shows the symbol `c1ok` from that compile.

The volume is 3632 sectors. Sixteen FAT sectors and 256 root entries stay under the 4086-cluster FAT12 limit, which leaves room for the library and the compiler's temporary files. `C2.EXE` reads the front end's `ex`, `sy`, and `in` files and writes `pr`, `gs`, and `ls`. `C3.EXE -c TINY.OBJ` turns those into an object. Its floating-point emulator loads an operand with `ES:LODSW` from the extra segment. `LINK.EXE TINY.OBJ,C1OK.EXE;` pulls `SLIBCE.LIB`, and `C1OK.EXE` prints `C2 OK`. The same load in `CL.EXE` is part of the large compile, and `LARGE.EXE` still prints `L1 OK`. The launch buffer holds `C2.EXE`.

`LIB2.EXE` is the library manager from `v4.0/src/LIB/LIB.EXE`, under the same MIT license. It is not the 3.10 tool. The shell runs `LIB2.EXE LIBH.LIB, LIB2.LST;`. It reads the library and writes the cross-reference. The banner is `IBM Library Manager/2 Version 1.01`, and the new file is `CMD DIR LIB2.LST`.

`DBOF.EXE` still prints its syntax line when launched with an empty tail. Later the shell runs `DBOF.EXE MAKEIN.TXT DBOFOUT.TXT 0 B`. That reads eleven bytes of `nmake-wrote` and writes them as `db` bytes. `TYPE` shows `06EH,06DH,061H,06BH,065H`.

`BUILDIDX.EXE` still prints `Incorrect number of parms  given to BUILDIDX.` when the tail is empty. Later the shell runs `BUILDIDX.EXE USAMS.MSG` on `USA-MS.MSG`. It reads the message file, rewrites it, and writes the index. The lines are `Message file updated.` and `Index file updated.` A child keeps only the handles it inherited, and a file write is stored when the handle is closed.

`NOSRVBLD.EXE` still prints `At lest one filespec ( for .SKL ) needed.` when the tail is empty. Later the shell runs `NOSRVBLD.EXE MORE.SKL USAMS.MSG`. The skeleton names one class and then ends, because a `:util` line makes this builder repeat that line. A skeleton read stops at the first newline so the buffered position stays on the next line. It writes `MORE.CLA`, the directory line is `CMD DIR MORE.CLA`, and the builder prints `Message files for MORE.SKL completed.`

`C1L.EXE` is the large-model C front end from that same tree, `v4.0/src/TOOLS/C1L.EXE`, under the same MIT license. The shell runs `C1L.EXE -f LARGE.C`. `TYPE sy` then shows the symbol `largeok`. The same `C2.EXE` and `C3.EXE -c LARGE.OBJ` passes turn that into an object. `LINK.EXE LARGE.OBJ,LARGE.EXE;` writes the program, and `LARGE.EXE` prints `L1 OK`.

`CL.EXE` still prints `usage: cl [ option... ] filename... [ -link linkoption... ]` when the tail is empty. A nested program load can hold the compiler passes, and the child's environment pointer keeps `MSC_CMD_FLAGS`. The shell deletes `LARGE.OBJ` and runs `CL.EXE -c LARGE.C`. CL starts C1, C2, and C3. The banner is `Microsoft (R) C Optimizing Compiler Version 5.10`, the new object is `CMD DIR LARGE.OBJ`, and the object names `_largeok`.

`MENUBLD.EXE` still prints `Couldn't open  menu creation files.` when the tail is empty. The shell then runs `MENUBLD.EXE MENU.MNU USAMS.MSG`. It reads the menu, the message file, and `USAMS.IDX`, and writes `MENUM.C`. The line is `All four files have been opened for menu creation.`, the directory line is `CMD DIR MENUM.C`, and the file contains `menu-ok`.

`BUILDMSG.EXE` still prints `Allocatable memory size is 606208` and returns status 1 when the tail is empty. The shell then runs `BUILDMSG.EXE USAMS BMSG.SKL`. The message file has a `BMSG` class, so the index contains it. The builder writes `BMSG.CTL` and the include files. The line is `Created include file [BMSG.CLA]`, the directory line is `CMD DIR BMSG.CTL`, and the file begins `$M_NUM_CLS  EQU 1`. The `:DEF` handler records the message number and writes the length line from that number. It does not copy the quoted skeleton text into the message table, so that quote is not added to `BMSG.CLA`.

Function 44h subfunction 0 returns the device word. Handles 0 through 2 that are still the console stay `80D3`. A disk file on drive A that has not been written returns `0040`. A write clears that clean bit, so the word is `0000`. `IOCHK.COM` writes one byte and prints `IOCTL 0000`.

A byte shifted by CL follows the same rotate, through-carry, logical, and arithmetic operations as a byte shifted once. `SAR.COM` arithmetic-shifts `80h` once and prints `SAR C0`.

Parity and overflow jumps follow the flags. `JP.COM` clears AL, which has even parity, and prints `JP Y`.

A one-bit shift sets the overflow flag from the bits that moved. `OF.COM` shifts `40h` left once, so the two high bits differ, and prints `OF Y`.

A 16-bit add, subtract, add-with-carry, or subtract-with-borrow takes parity from the low byte. `WP.COM` increments zero and prints `WP 0`, because `1` has odd parity.

A multiply sets carry and overflow when the product does not fit in the low half. `MUL.COM` multiplies `FFh` by itself and prints `MUL 1`.

Decimal adjust turns a binary sum of two decimal digits into a packed decimal byte. `DA.COM` adds `1` to `19h` and prints `DA 20`.

An immediate count shifts a word the same way a one-bit word shift does. `SH.COM` shifts `1` left by four and prints `SH 0010`.

A signed multiply by a 16-bit immediate stores the low product. `IM.COM` multiplies `3` by `5` and prints `IM 000F`.

An immediate count rotates a byte the same way a one-bit rotate does. `RL.COM` rotates `1` left by four and prints `RL 10`.

ASCII adjust after multiply splits a byte by the immediate base. `AM.COM` divides `15` by `10` and prints `AM 0105`.

ASCII adjust before division joins the unpacked digits. `AD.COM` combines `01h` and `05h` with base `10` and prints `AD 000F`.

ASCII adjust after addition turns a binary sum of two digits into unpacked decimal. `AA.COM` adds `'9'` and `'1'` and prints `AA 0100`.

ASCII adjust after subtraction borrows from the high digit. `AS.COM` subtracts `1` from unpacked `10` and prints `AS 0009`.

Decimal adjust after subtraction turns a binary difference of two packed digits into a packed decimal byte. `DS.COM` subtracts `1` from `20h` and prints `DS 19`.

Complement carry flips the carry flag. `CMC.COM` sets carry, complements it, and prints `CMC 0`.

Operation 6 of a shift by CL is a left shift, the same as operation 4. `S6.COM` shifts `1` left once that way and prints `S6 02`.

A repeated byte load reads through the source index. `LD.COM` repeats a load of one byte and prints `LD Q`.

A word compare reads both strings and sets the flags from their difference. `CW.COM` compares two copies of `1234h` and prints `CW Y`.

A repeated string move accepts a segment override after the repeat. `RS.COM` moves one byte with the extra segment as the source and prints `RS Q`.

A frame with one nested level pushes the new frame pointer before the locals. `EN.COM` starts the stack at `2000h` and prints `EN 1FFE1FF8`.

Interrupt on overflow enters vector 4 when the overflow flag is set. `IT.COM` adds `1` to `7Fh` and prints `IT Y`.

A bounds check enters vector 5 when the index is outside the signed limits. `BD.COM` checks `0` against `1` and `10`, the handler brings the index inside the range, and the program prints `BD Y`.

A lock prefix executes the following instruction. `LK.COM` locks an add of `3` and `3` and prints `LK 0006`.

Reading a handle's time returns the directory stamp. `HELLO.COM` is marked `12:00:00` on `1988-01-01`, and `TM.COM` prints `TM 60001021`.

Writing a handle's time stores those words in the directory. `ST.COM` sets `1234h` and `4567h` on `TM.COM` and prints `TS 12344567`.

A carry sets every bit of the low accumulator byte, and a clear carry stores zero. `SC.COM` prints `SC FF00`.

Reading an interrupt vector that has not been set returns zeros and clears carry. `GV.COM` reads vector `80h` and prints `GV 00000000`.

Drive allocation reports media `F0h`, one sector per cluster, `512` bytes per sector, and `3599` clusters. `DI.COM` prints `DI F00102000E0F`.

The verify flag starts clear. Setting it and reading it back returns the stored value. `VF.COM` prints `VF 01`.

An FCB size query rounds the file length up to a record count. `HELLO.COM` is 38 bytes, the record size is 1, and `SZ.COM` prints `SZ 00000026`.

An FCB sequential read copies one record into the transfer area. The first byte of `HELLO.COM` is `B2h`, and `FR.COM` prints `FR B2`.

An FCB sequential write stores the transfer area into the file. `FW.COM` writes `WXYZ` and prints `FW WXYZ`.

An FCB delete removes the named file. `FD.COM` creates `ZZ.DAT`, deletes it, and the next open returns `0002h`. It prints `FD 0002`.

An FCB rename moves the directory name and leaves the bytes in place. `RN.COM` writes `ABCD` into `OLD.DAT`, renames it to `NEW.DAT`, and prints `RN ABCD`.

An FCB random read uses the random record field. `RR.COM` reads record 1 of `HELLO.COM` at one byte per record and prints `RR 41`.

An FCB random write stores the transfer area at the random record. `RW.COM` writes `Q` at record 2 and prints `RW 000051`.

An FCB create truncates an existing file. `MK.COM` writes three bytes, creates the same name through an FCB, and prints `MK 00000000`.

An FCB set-random-record stores `block * 128 + current`. `SR.COM` uses block 1 and record 3 and prints `SR 00000083`.

Extended open creates a file named at `DS:SI` and reports the action in `CX`. `EX.COM` creates `EX.DAT`, writes `EXOK`, and prints `EX 0002EXOK`.

Truename expands a path to a drive and directory with `.` and `..` removed. `TN.COM` passes `a:\dos\..\hello.com` and prints `TN A:\HELLO.COM`.

A temporary file is created in the requested directory and its generated name is written back over the terminator. `TF.COM` asks for a file in `A:\`, writes `TEMP`, and prints `TF 00000000TEMP`.

Committing a handle writes its buffer to the directory without closing it. `CM.COM` writes `CMIT`, commits the handle, opens the file again, and prints `CM CMIT`.

Get DTA returns the transfer address stored by set DTA. `GD.COM` sets the address to `1A80h` and prints `GD 0B001A80`.

The InDOS flag and the following critical-error flag are clear once the call returns. `ND.COM` sets both bytes, calls again, and prints `ND 0000`.

The list of lists reports the largest sector, the number of block devices, and the last drive. `LL.COM` reads those fields and prints `LL 02000202`.

`CONVERT.EXE` still prints `Usage: Convert [file]` when the tail is empty. A `.COM` input is not implemented. The shell copies `HELLO.EXE` to `CONV.EXE` and runs `CONVERT.EXE CONV.EXE`. It writes `CONV.COM`. The line is `conv.exe => conv.com [OK]`, the directory line is `CMD DIR CONV.COM`, and the file contains `Converted`.

`EXEC.EXE` walks its arguments and runs each program. The shell runs `EXEC.EXE EXITCODE.COM`. The child exits with status 3, and the line is `exec: EXITCODE.COM returned 3`.

`EXEFIX.EXE`, already recorded above, is also launched by the shell as `EXEFIX.EXE PACKME.EXE 1234 0010`. It writes those allocation words into `PACKME.EXE`. The serial line is `EXEFIX 1234 0010`.

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

`EXEPACK.EXE` is the compressor from that same tree, `v4.0/src/TOOLS/EXEPACK.EXE`, under the same MIT license. The shell runs `EXEPACK.EXE EXEFIX.EXE PACKED.EXE`. The written file is 0x28AF bytes and begins with the MZ mark, and it contains the packer's stub text. The serial line is `PACKED 28AF 4D5A OK`. The directory lists `CMD DIR PACKED.EXE`. After that write the compressor does not return to the shell, so the shell reports a non-zero status. The proof of the write is those two lines.

`BUILDIDX.EXE` is the message-index builder from that same tree, `v4.0/src/TOOLS/BUILDIDX.EXE`, under the same MIT license. The shell runs it with an empty tail, and it prints `Incorrect number of parms  given to BUILDIDX.`

`BUILDMSG.EXE` is the message builder from that same tree, `v4.0/src/TOOLS/BUILDMSG.EXE`, under the same MIT license. The shell runs it with an empty tail. It prints `BuildMsg - PSP at 0000` and `Allocatable memory size is 606208`, then returns status 1.

`MENUBLD.EXE` is the menu builder from that same tree, `v4.0/src/TOOLS/MENUBLD.EXE`, under the same MIT license. The shell runs it with an empty tail. It prints `Couldn't open  menu creation files.` and returns status 1.

`NOSRVBLD.EXE` is the message-file builder from that same tree, `v4.0/src/TOOLS/NOSRVBLD.EXE`, under the same MIT license. The file allocation table is two sectors, so this tool still fits beside the packed output. The shell runs it with an empty tail. It prints `At lest one filespec ( for .SKL ) needed.` and returns status 1.

Repeated word string operations `F3 AB`, `F3 A7`, `F3 AD`, and `F3 AF` store, compare, load, and scan a word on each count. `CL.EXE` is the Microsoft C 5.10 compiler driver from that same tree, `v4.0/src/TOOLS/CL.EXE`, under the same MIT license. The shell runs it with an empty tail. It prints `usage: cl [ option... ] filename... [ -link linkoption... ]` and returns status 1.

INT 21h function 36h reports free and total allocation units. DOS `DISK_INFO` counts zero entries from cluster 2 through the highest cluster, and the total is one less than that highest cluster. Function 36h swaps those results, so the free count and the total are separate. The sectors-per-cluster count stays in the low half of the accumulator word. A highest cluster of 4086 or more is a 16-bit table; below that the entries are 12-bit. Drive 0 and drive 1 both name this volume. Any other drive number returns FFFF. The guest scans the mutable table, so a volume larger than 512 sectors still reports the clusters that are actually free, and EXEPACK still writes `PACKED 28AF 4D5A OK`.

`LIB.EXE` is the library manager from that same tree, `v4.0/src/TOOLS/LIB.EXE`, under the same MIT license. The shell runs `LIB.EXE LIBH.LIB, LIB.LST` on the real library `v4.0/src/TOOLS/BLD/LIB/LIBH.LIB`. It prints `Microsoft (R) Library Manager  Version 3.10`. A missing file opened with the create flag is created with the caller's attribute, and the new handle can be duplicated. The runtime then writes `LIB.LST`. The directory line is `CMD DIR LIB.LST`, and the listing begins `LIB LST __aFFaldiv`.

`ASC2HLP.EXE` is the help builder from that same tree, `v4.0/src/TOOLS/ASC2HLP.EXE`, under the same MIT license. The shell runs `ASC2HLP.EXE USA.TXT O.HLP` on the real help source `v4.0/src/SELECT/USA.TXT`. A child releases its allocation when it exits, so this builder can take the 64000-byte block it asks for. A missing output file is created with function 5Bh and is left untouched when that name already exists. It prints `The number topic(s) and text string(s) written = 57`.

The guest volume is 1208 sectors with four sectors of FAT. The highest cluster is 1200, still below 4086, so the entries stay 12-bit. That leaves free clusters after `LINK.EXE`, `SAY.OBJ`, and `BINMODE.OBJ` for the packed file, the library listing, and the help output. A shift writes the zero, sign, and parity flags from its result and leaves the carry flag as the bit that shifted out. `LINK.EXE` is the overlay linker from that same tree, `v4.0/src/TOOLS/LINK.EXE`, under the same MIT license. The shell runs `LINK.EXE SAY.OBJ,SAY.EXE;` and then `SAY.EXE`. A program larger than 32KB keeps its conventional allocations past the loaded image, the minimum extra paragraphs, and a full 64KB beyond the header stack, so the near data segment is not reused as a far heap. The linker prints `Microsoft (R) Overlay Linker  Version 3.65`. The linked program prints `LINKED OK`.

`EXEFIX.EXE`, already recorded above, is also launched by the shell as `EXEFIX.EXE PACKME.EXE 1234 0010`. It writes those allocation words into `PACKME.EXE`. The serial line is `EXEFIX 1234 0010`.

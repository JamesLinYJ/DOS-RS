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

# Guest program observations

These are the individual workloads behind the QEMU smoke suite. The guest disk includes tools and sample inputs from the MS-DOS 4.0 release identified in [NOTICE](../NOTICE). The exact assertions are in [qemu-smoke.sh](../scripts/qemu-smoke.sh).

`disks/guest12.img` is a FAT12 volume. The directory entry `HELLO   COM` is cluster 2. The loader copies those bytes to offset `0x100` in a program segment prefix and runs them. The program adds 6 to the letter A, prints that byte, then prints the rest of the line. The serial check is `GUEST HELLO COM`.

`HELLO   EXE` is cluster 3. The loader places the image 16 paragraphs above the prefix and adds that load segment to each relocation word. The program then copies the fixed word into the data selector and prints `HELLO EXE`.

`RUN.COM` opens `DATA.TXT`, reads one byte, writes it after `FILE `, then loads `CHILD.COM` with function `4Bh`. The child stops with function `4Ch` and code `7`. The parent reads that code with function `4Dh` and prints `FILE Z EXEC 7`. Handles, the saved return code, and the child register file are owned by the guest session. The parent register file is left in place.

`COMPRESS.COM` is the SELECT panel compressor from Microsoft MS-DOS 4.0, file `v4.0/src/TOOLS/COMPRESS.COM` in the [MIT-licensed](https://github.com/microsoft/MS-DOS/blob/main/LICENSE) `microsoft/MS-DOS` tree (Copyright (c) Microsoft Corporation). The guest image also carries `SEL-PAN.DAT`, five letter A bytes. The program opens that file, writes `SELECT.DAT`, and the kernel prints `COMPRESS FF0541`. The shell also launches `COMPRESS.COM` by name. That run creates the same file: the batch lists it as `Found: SELECT.DAT` and types the bytes `FF 05 41`.

`EXE2BIN.EXE` is the same tree's converter, `v4.0/src/TOOLS/EXE2BIN.EXE`, under that same MIT license. With an empty command tail it prints `File not found`. The kernel writes `EXE2BIN ` first, so the serial check is `EXE2BIN File not found`.

`EXEC.EXE` is the Microsoft C 5.10 launcher from that tree, `v4.0/src/TOOLS/EXEC.EXE`, under the same MIT license. The prefix carries `PATH=\` and the tail ` HELLO.COM`. Its child exec loads that program, and the serial log shows `GUEST HELLO COM` from the child.

`DBOF.EXE` is the dump-binary tool from that same tree, `v4.0/src/TOOLS/DBOF.EXE`, under the same MIT license. With an empty command tail it prints `DBOF Syntax: DBOF Source-file Target-file Hex-start Hex-length`.

`EXEFIX.EXE` is the header fixer from that same tree, `v4.0/src/TOOLS/EXEFIX.EXE`, under the same MIT license. With an empty command tail it prints `usage: exefix <file> <max-alloc> <min-alloc>`.

`CONVERT.EXE` is the file converter from that same tree, `v4.0/src/TOOLS/CONVERT.EXE`, under the same MIT license. With an empty command tail it prints `Usage: Convert [file]`.

`ATTRIB.EXE` is the attribute utility from that same tree, `v4.0/src/TOOLS/ATTRIB.EXE`, under the same MIT license. The tail ` HELLO.COM` makes it read that file's attribute and print `  A       A:\\HELLO.COM`.

`EXEMOD.EXE` is the header utility from that same tree, `v4.0/src/TOOLS/EXEMOD.EXE`, under the same MIT license. The tail ` HELLO.EXE /h` makes it print that file's header, including `.EXE size (bytes)                     3D              61`.

Opcode `8D` with a register source is not a memory operand. Compat16's `execute_lea` returns an error for that form. The 8086 manual only defines `8D` for a memory source. On 8086 and 8088 silicon the same encoding copies the most recent effective-address offset into the destination, because the address unit does not replace that offset when the source is a register. The guest follows the silicon.

`INDEXMAE.SYS` in `v4.0/src/DEV/XMAEM` is the remaining unused upstream binary. It is not a device image SYSINIT can call. The file is 1189 bytes, the attribute word `0C040h` and the name `386XMAEM` are not a header at offset 0, and the apparent message pointer is past the end of the file. The driver's own init, after any banner, checks for a PS/2 model 80, reads the machine status word, and then builds protected-mode tables for XMA hardware this guest does not have. No success line is invented for it. Guest programs do call INT 21h function 0Eh to select a drive. Drives 0 and 1 name this volume; any other number leaves the current drive unchanged. The call returns two usable drive letters and does not set carry. Function 19h reports the drive that 0Eh selected. Function 29h parses a file name into an FCB: a drive letter is stored as 1 for A, `*` fills the rest of that field with `?`, and the result is 1 when a wildcard was present. Function 33h reads and stores the control-break flag, stores the extended break flag, and reports drive 1 as the boot drive. `PARSE.COM` prints `PARSE 2 HELLO    COM 0`, `WILD 1 ???????? ???`, and `CTRL 0 1 BOOT 1`. Function 59h returns the extended error recorded by the last failed call: class, action, and locus come from the DOS error table, and a missing file is code 2, class 8, action 3, locus 2. Function 62h returns the program segment prefix of the program that is running. Function 46h forces one open handle onto another, closing the target first when the two differ. Function 87h is not a DOS 4.0 call, so it returns invalid function with carry set. `CALLS.COM` prints `EXT 0002 08 03 02`, `PSP 0B00`, `DUP2 OK`, and `FN87 0001 1`.

`EXEPACK.EXE` is the compressor from that same tree, `v4.0/src/TOOLS/EXEPACK.EXE`, under the same MIT license. The shell runs `EXEPACK.EXE EXEFIX.EXE PACKED.EXE`. The written file is 0x2A00 bytes and begins with the MZ mark, and it contains the packer's stub text. The serial line is `PACKED 2A00 4D5A OK`. The directory lists `Found: PACKED.EXE`. A far allocation that used to alias the program image now keeps those instruction bytes, so the compressor returns to the shell. The shell does not print `Exit code: 255`. The shell then runs `PACKED.EXE` with an empty tail. The stub unpacks it and the program prints `usage: exefix <file> <max-alloc> <min-alloc>`. It also copies `HELLO.EXE` to `FIX2.EXE` and runs `PACKED.EXE FIX2.EXE 0ABC 0020`. The unpacked image rewrites that header. The serial line is `PACKFIX 0ABC 0020`, and the directory line is `Found: FIX2.EXE`. The unpacked tool's `EXEFIX 1234 0010` on `PACKME.EXE` stays.

`BUILDIDX.EXE` is the message-index builder from that same tree, `v4.0/src/TOOLS/BUILDIDX.EXE`, under the same MIT license. The shell runs it with an empty tail, and it prints `Incorrect number of parms  given to BUILDIDX.`

`BUILDMSG.EXE` is the message builder from that same tree, `v4.0/src/TOOLS/BUILDMSG.EXE`, under the same MIT license. The shell runs it with an empty tail. It prints `BuildMsg - PSP at 0b00` and `Allocatable memory size is 526944`, then returns status 1. The segment is the program prefix function 62h returns.

`MENUBLD.EXE` is the menu builder from that same tree, `v4.0/src/TOOLS/MENUBLD.EXE`, under the same MIT license. The shell runs it with an empty tail. It prints `Couldn't open  menu creation files.` and returns status 1.

`NOSRVBLD.EXE` is the message-file builder from that same tree, `v4.0/src/TOOLS/NOSRVBLD.EXE`, under the same MIT license. The file allocation table is two sectors, so this tool still fits beside the packed output. The shell runs it with an empty tail. It prints `At lest one filespec ( for .SKL ) needed.` and returns status 1.

Repeated word string operations `F3 AB`, `F3 A7`, `F3 AD`, and `F3 AF` store, compare, load, and scan a word on each count. `CL.EXE` is the Microsoft C 5.10 compiler driver from that same tree, `v4.0/src/TOOLS/CL.EXE`, under the same MIT license. The shell runs it with an empty tail. It prints `usage: cl [ option... ] filename... [ -link linkoption... ]` and returns status 1.

INT 21h function 36h reports free and total allocation units. DOS `DISK_INFO` counts zero entries from cluster 2 through the highest cluster, and the total is one less than that highest cluster. Function 36h swaps those results, so the free count and the total are separate. The sectors-per-cluster count stays in the low half of the accumulator word. A highest cluster of 4086 or more is a 16-bit table; below that the entries are 12-bit. Drive 0 and drive 1 both name this volume. Any other drive number returns FFFF. The guest scans the mutable table, so a volume larger than 512 sectors still reports the clusters that are actually free, and EXEPACK still writes `PACKED 2A00 4D5A OK`.

`LIB.EXE` is the library manager from that same tree, `v4.0/src/TOOLS/LIB.EXE`, under the same MIT license. The shell runs `LIB.EXE LIBH.LIB, LIB.LST` on the real library `v4.0/src/TOOLS/BLD/LIB/LIBH.LIB`. It prints `Microsoft (R) Library Manager  Version 3.10`. A missing file opened with the create flag is created with the caller's attribute, and the new handle can be duplicated. The runtime then writes `LIB.LST`. The directory line is `Found: LIB.LST`, and the listing begins `LIB LST __aFFaldiv`.

`ASC2HLP.EXE` is the help builder from that same tree, `v4.0/src/TOOLS/ASC2HLP.EXE`, under the same MIT license. The shell runs `ASC2HLP.EXE USA.TXT O.HLP` on the real help source `v4.0/src/SELECT/USA.TXT`. A child releases its allocation when it exits, so this builder can take the 64000-byte block it asks for. A missing output file is created with function 5Bh and is left untouched when that name already exists. It prints `The number topic(s) and text string(s) written = 57`.

`LINK.EXE` is the overlay linker from that same tree, `v4.0/src/TOOLS/LINK.EXE`, under the same MIT license. The shell runs `LINK.EXE SAY.OBJ,SAY.EXE;` and then `SAY.EXE`. The process block reserves the declared image, minimum extra memory and initial stack before DOS allocates its heap. The linker prints `Microsoft (R) Overlay Linker  Version 3.65` and `Copyright (C) Microsoft Corp 1983-1988.  All rights reserved.` A console write returns every byte of that message. Function 47h returns the current directory without a leading slash, and the root is an empty string. `ATTRIB.EXE HELLO.COM` therefore prints `A:\HELLO.COM`. Function 4Fh continues the find started by function 4Eh. `NEXT.COM` searches `*.COM` and prints the second match, `NEXT CHILD.COM`. The found name is an ASCIIZ string; when the 13-byte field has room, a carriage return, line feed, and `$` follow the terminator so a function 09h print of that field stops after the name. `DOSCALL` therefore prints `INT FIND HELLO.COM` and then `INT CWD ROOT` on the next line. A directory entry with no date is reported as 1 January 1980, the same day function 2Ah returns, so a blank stamp is not mistaken for a missing file. The linked program prints `LINKED OK`. `EXE2BIN.EXE` then converts that program and the serial line is `EXE2BIN 0018 0E1F OK`.

`NMAKE.EXE` is the program maintenance utility from that same tree, `v4.0/src/TOOLS/NMAKE.EXE`, under the same MIT license. Images larger than 64KB are stored straight through conventional memory, so the packed stub is not wrapped over the front of the load. The shell runs `NMAKE.EXE -f MAKEFILE`. `TOOLS.INI` is the initialization file from that same tree, so the utility no longer prints `U4013`. The makefile copies `MAKEIN.TXT` to `MAKEOUT.TXT`. The utility prints `Microsoft (R) Program Maintenance Utility   Version 1.00.05`, and `TYPE MAKEOUT.TXT` prints `nmake-wrote`.

`MASM.EXE` is the macro assembler from that same tree, `v4.0/src/TOOLS/MASM.EXE`, under the same MIT license. A byte shift by one covers rotate, rotate-through-carry, logical shift, and arithmetic shift, in either direction. The shell runs `MASM.EXE MASMIN,MASMOUT,,;` on `MASMIN.ASM`, then `LINK.EXE MASMOUT.OBJ,MASMOK.EXE;` and `MASMOK.EXE`. `MASMIN.ASM` defines a stack segment, so that link does not warn `L4021`, and the linked program still prints `MASM OK`. `SAY.OBJ` stays without a stack segment. That is why `EXE2BIN` can still write the 24-byte binary, and why that link still warns `L4021: no stack segment`. The assembler prints `Microsoft (R) Macro Assembler Version 5.10`.

`C1.EXE` is the C front end from that same tree, `v4.0/src/TOOLS/C1.EXE`, under the same MIT license. A guest can keep twenty files open at once, and each open file can be as large as the small-model library. The shell runs `C1.EXE -f TINY.C`. The flag names the source, and `TYPE sy` shows the symbol `c1ok` from that compile.

The volume is 3632 sectors. Sixteen FAT sectors and 256 root entries stay under the 4086-cluster FAT12 limit, which leaves room for the library and the compiler's temporary files. `C2.EXE` reads the front end's `ex`, `sy`, and `in` files and writes `pr`, `gs`, and `ls`. `C3.EXE -c TINY.OBJ` turns those into an object. Its floating-point emulator loads an operand with `ES:LODSW` from the extra segment. `LINK.EXE TINY.OBJ,C1OK.EXE;` pulls `SLIBCE.LIB`, and `C1OK.EXE` prints `C2 OK`. The same load in `CL.EXE` is part of the large compile, and `LARGE.EXE` still prints `L1 OK`. The launch buffer holds `C2.EXE`.

`LIB2.EXE` is the library manager from `v4.0/src/LIB/LIB.EXE`, under the same MIT license. It is not the 3.10 tool. The shell runs `LIB2.EXE LIBH.LIB, LIB2.LST;`. It reads the library and writes the cross-reference. The banner is `IBM Library Manager/2 Version 1.01`, and the new file is `Found: LIB2.LST`.

`DBOF.EXE` still prints its syntax line when launched with an empty tail. Later the shell runs `DBOF.EXE MAKEIN.TXT DBOFOUT.TXT 0 B`. That reads eleven bytes of `nmake-wrote` and writes them as `db` bytes. `TYPE` shows `06EH,06DH,061H,06BH,065H`.

`BUILDIDX.EXE` still prints `Incorrect number of parms  given to BUILDIDX.` when the tail is empty. Later the shell runs `BUILDIDX.EXE USAMS.MSG` on `USA-MS.MSG`. It reads the message file, rewrites it, and writes the index. The lines are `Message file updated.` and `Index file updated.` A child keeps only the handles it inherited, and a file write is stored when the handle is closed.

`NOSRVBLD.EXE` still prints `At lest one filespec ( for .SKL ) needed.` when the tail is empty. Later the shell runs `NOSRVBLD.EXE MORE.SKL USAMS.MSG`. The skeleton names one class and then ends, because a `:util` line makes this builder repeat that line. A skeleton read stops at the first newline so the buffered position stays on the next line. It writes `MORE.CLA`, the directory line is `Found: MORE.CLA`, and the builder prints `Message files for MORE.SKL completed.`

`C1L.EXE` is the large-model C front end from that same tree, `v4.0/src/TOOLS/C1L.EXE`, under the same MIT license. The shell runs `C1L.EXE -f LARGE.C`. `TYPE sy` then shows the symbol `largeok`. The same `C2.EXE` and `C3.EXE -c LARGE.OBJ` passes turn that into an object. `LINK.EXE LARGE.OBJ,LARGE.EXE;` writes the program, and `LARGE.EXE` prints `L1 OK`.

`CL.EXE` still prints `usage: cl [ option... ] filename... [ -link linkoption... ]` when the tail is empty. A nested program load can hold the compiler passes, and the child's environment pointer keeps `MSC_CMD_FLAGS`. The shell deletes `LARGE.OBJ` and runs `CL.EXE -c LARGE.C`. CL starts C1, C2, and C3. The banner is `Microsoft (R) C Optimizing Compiler Version 5.10`, the new object is `Found: LARGE.OBJ`, and the object names `_largeok`.

`MENUBLD.EXE` still prints `Couldn't open  menu creation files.` when the tail is empty. The shell then runs `MENUBLD.EXE MENU.MNU USAMS.MSG`. It reads the menu, the message file, and `USAMS.IDX`, and writes `MENUM.C`. The line is `All four files have been opened for menu creation.`, the directory line is `Found: MENUM.C`, and the file contains `menu-ok`.

`BUILDMSG.EXE` still prints `Allocatable memory size is 526944` and returns status 1 when the tail is empty. The shell then runs `BUILDMSG.EXE USAMS BMSG.SKL`. The message file has a `BMSG` class, so the index contains it. The builder writes `BMSG.CTL` and the include files. The line is `Created include file [BMSG.CLA]`, the directory line is `Found: BMSG.CTL`, and the file begins `$M_NUM_CLS  EQU 1`. The `:DEF` handler records the message number and writes the length line from that number. It does not copy the quoted skeleton text into the message table, so that quote is not added to `BMSG.CLA`.

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

Set PSP stores the segment that get PSP returns. `PS.COM` selects `1234h`, reads it back, restores the previous segment, and prints `PS 1234`.

The allocation strategy is first fit until a program selects another. `MS.COM` selects best fit, reads it back, and prints `MS 0001`.

Country information for the United States uses country code `0001h`, month-day-year dates, and a `$` currency symbol. `CT.COM` prints `CT 0001000024`.

The active and system code pages are both 437. `CP.COM` reads them and prints `CP 01B501B5`.

The switch character starts as `/`. `SW.COM` selects `-`, reads it back, restores `/`, and prints `SW 2D`.

The volume serial and label are stored in the boot sector. `SN.COM` writes serial `12345678h` and label `DOS4VOLUMEX`, reads them back, and prints `SN 12345678DOS4VOLUMEX`.

The handle table holds 20 handles. `HC.COM` asks for 40 and prints `HC 0008`.

Creating a PSP copies the current process prefix and records that process as the parent. `NP.COM` prints `NP CD200B00`.

The drive parameter block reports the boot record. `DP.COM` reads one reserved sector, one FAT, 256 root entries, and 16 sectors per FAT. It prints `DP 00010101000010`.

The clock date starts at Tuesday 1 January 1980. `SD.COM` sets 2 January 1980, reads it back, and prints `SD 07BC010203`.

The clock time starts at midnight. `CK.COM` sets 13:45:07.50, reads it back, and prints `CK 0D2D0732`.

The uppercase table for the current code page is 128 bytes long and starts at character `80h`. `UT.COM` prints `UT 020080`.

Direct console output writes the character, and a read with no key waiting returns zero with the zero flag set. `DC.COM` prints `DC Q00`.

Printer output sends the byte to the printer. `LP.COM` sends `LP ZOK` and that line appears.

Auxiliary output sends the byte to the serial port. `AX.COM` sends `AX ZOK` and that line appears.

Input status is `FFh` while a file still has bytes to read and `00h` at the end. `IS.COM` prints `IS FF00`.

This volume is removable. `RM.COM` asks about drive A and prints `RM 0000`.

The same drive is local. `LC.COM` reads the attribute word and prints `LC 0000`.

A file handle on that drive is local too. `HL.COM` opens `HL.DAT` and prints `HL 0000`.

Output status stays ready at the end of a file, because the file can still grow. `OS.COM` seeks to the end and prints `OS FF`.

Drive A has one logical assignment. `LM.COM` asks for that map and prints `LM 00`.

An FCB search returns the matching directory name in the transfer area. `FF.COM` finds `HELLO.COM` and then `CHILD.COM`, and prints `FF HELLO.COM CHILD.COM`.

Closing an FCB writes its size back to the directory. `FC.COM` writes `WXYZ`, closes the file at two bytes, and prints `FC 0002WX`.

A random block read returns several records and the count. `BK.COM` reads two records from `ABCD` and prints `BK 0002ABCD`.

A random block write stores several records from the transfer area. `BW.COM` writes `WXYZ` as two records and prints `BW 0002WXYZ`.

Disk reset writes every open file that still has new bytes. `DZ.COM` writes `QR` without closing, resets the disk, and prints `DZ 00QR`.

A locked region rejects a write from another handle. `UL.COM` locks the first two bytes of `ABCD`, the other handle is refused, and the file still reads `ABCD`. It prints `UL 0021ABCD`.

Code page 437 has no double-byte lead bytes. `DB.COM` asks for that table and prints `DB 070000`.

Extended country information names the United States and code page 437. `CI.COM` prints `CI 01000101B5`.

An ASCIIZ string is capitalized in place. `CA.COM` turns `abc` into `ABC` and prints `CA ABC`.

A counted string stops at the given length. `CC.COM` capitalizes three letters of `abcx` and prints `CC ABCx`.

One character is capitalized on its own. `CQ.COM` passes `q` and prints `CQ 51`.

A yes or no answer is country text. `YN.COM` tests `y`, `n`, and `q`, and prints `YN 000102`.

The collating table gives each byte its own sort weight. `CO.COM` reads that table and prints `CO 060100`.

Filename characters run from byte 00 through FF. `FN.COM` reads that table and prints `FN 05001600FF`.

The machine name can be stored and read back. `MN.COM` sets `DOS4RS` and prints `MN 01DOS4RS`.

A printer redirect is stored and read back. `RD.COM` points `LPT1` at `\\SRV\PRT` and prints `RD \\SRV\PRT`.

`CONVERT.EXE` still prints `Usage: Convert [file]` when the tail is empty. A `.COM` input is not implemented. The shell copies `HELLO.EXE` to `CONV.EXE` and runs `CONVERT.EXE CONV.EXE`. It writes `CONV.COM`. The line is `conv.exe => conv.com [OK]`, the directory line is `Found: CONV.COM`, and the file contains `Converted`.

`EXEC.EXE` walks its arguments and runs each program. The shell runs `EXEC.EXE EXITCODE.COM`. The child exits with status 3, and the line is `exec: EXITCODE.COM returned 3`.

`EXEFIX.EXE`, already recorded above, is also launched by the shell as `EXEFIX.EXE PACKME.EXE 1234 0010`. It writes those allocation words into `PACKME.EXE`. The serial line is `EXEFIX 1234 0010`.

## Installable drivers

The [device suite](../scripts/qemu-devices.py) builds its own FAT image. A DOS client opens two character names from a chained MZ driver, writes and reads a byte, then writes and reads a full sector through a flat block driver. It also reads LBA 7 from a separate IDE disk, writes to COM2 and LPT1, and waits for the BIOS key word 1E61h. QEMU supplies the key through its emulated PS/2 controller. The suite checks both host output files and the guest transcript on both CPU targets.

An optional upstream FreeDOS driver exercises a separate implementation of the request ABI. To build the revision used in testing:

```sh
git clone https://github.com/FDOS/kernel.git target/freedos-kernel
git -C target/freedos-kernel checkout ca25e3b26695074f754e1baffdfb4fd1452a6dbc
nasm -f bin target/freedos-kernel/setver/setver.asm -o target/SETVER.SYS
python3 scripts/qemu-devices.py --setver target/SETVER.SYS
```

The client checks the table pointer that SETVER installs through INT 21h AH=52h. EXEC uses matching table entries for the child's PSP version word and AH=30h response. In a separate x86_64 run, a configured table entry for WHOAMI.COM returned 7.10 through both interfaces. [FreeDOS driver source](https://github.com/FDOS/kernel/blob/ca25e3b26695074f754e1baffdfb4fd1452a6dbc/setver/setver.asm)

The loader currently accepts up to sixteen resident headers and a 64 KiB load segment per image. INIT can call DOS character/string output, version and list-of-lists services. Firmware interrupt and port coverage is limited to the implemented PC devices. Installable block units support absolute I/O. Drive mounting, driver-installed IRQ handlers, XMS and EMS remain open.

//! Fixed boot workloads and their serial observations.
use crate::{bios, cmd, dos, guest, invoke_int21, put_hex8, put_hex16, putc, puts, puts_result};

pub(super) fn run() {
    puts_result(&invoke_int21(true));
    puts_result(&invoke_int21(false));
    bios::mscon::con_write_bytes(b"BIOS\r\n");
    bios::msaux::aux_write_bytes(b"AUX\r\n");
    let mut sector = [0u8; 512];
    let disk_status = bios::msdisk::read_sector0(&mut sector);
    puts("DSK");
    put_hex16(disk_status);
    puts("\n");
    let mut floppy = [0u8; 512];
    if bios::msdisk::read_floppy_boot(&mut floppy)
        && bios::msdisk::boot_sector_valid(&floppy)
        && bios::msdisk::bpb_bytes_per_sector(&floppy) == Some(512)
        && bios::msdisk::image_fat_media() == Some(0xF8)
        && bios::msdisk::fixed_disk_paths()
    {
        puts("DSKF55AA\n");
    }
    puts("MED");
    put_hex16(bios::msdisk::media_check() as u16);
    puts("\n");
    let mut clock = [0u8; 6];
    let _clock_status = bios::msclock::read_clock(&mut clock);
    puts("CLK");
    put_hex16(u16::from_le_bytes([clock[0], clock[1]]));
    put_hex16(u16::from_le_bytes([clock[2], clock[3]]));
    puts("\n");
    // INT 1Ah is entered with CX=1 and DX=0x0E. The gate must hand those
    // ticks back, and TIM$READ then stores 1:00:00.36.
    let mut forced = [0u8; 6];
    let _forced = bios::msclock::read_clock_at(&mut forced, 1, 0x0E);
    puts("CLKP");
    for byte in forced {
        put_hex8(byte);
    }
    puts("\n");
    // Same 1:00:00.36 record, through TIME_TO_TICKS. The inverse is
    // CX=1 DX=0x000D.
    let written = [0x00, 0x00, 0x00, 0x01, 0x24, 0x00];
    let (hi, lo) = bios::msclock::write_clock(&written);
    puts("CLKW");
    put_hex16(hi);
    put_hex16(lo);
    puts("\n");
    let _prn = bios::mslpt::prn_write_bytes(b"PRN\r\n");
    if bios::mslpt::prn_stat() == 0x0100 {
        puts("PRNS\n");
    }
    puts("PRNR");
    put_hex16(bios::mslpt::prn_ioctl_get());
    puts("\n");
    // `PRN$TILBUSY` is a printer entry, not a SYSINIT step. The byte it
    // sends is the `.` on its own line. The count follows with the other
    // printer lines, before the loader and the missing interpreter.
    let sent = bios::mslpt::prn_tilbusy(b".");
    puts("\nPRNT");
    put_hex16(sent);
    puts("\n");
    if let Some(bytes) = bios::msload::load_io_sys() {
        puts("LOADOK\n");
        puts("LOAD");
        put_hex16(bytes);
        puts("\n");
    }
    let _messages = bios::sysimes::catalog_bytes();
    if let Ok(number) = bios::sysinit2::get_num(b"48") {
        puts("SYN");
        put_hex16(number);
        puts("\n");
    }
    puts("PAR");
    put_hex16(bios::sysinit1::para_round(17));
    puts("\n");
    puts("BUF");
    put_hex16(bios::sysinit1::default_buffers(false, false, false));
    puts("\n");
    if bios::sysinit1::config_line(b"BUFFERS=2\r") == Ok(Some(2))
        && bios::sysinit1::config_line(b"FOO\r").is_err()
    {
        bios::mscon::con_write_bytes(bios::sysimes::BADOPM);
        bios::mscon::con_write_bytes(b"\r\n");
    }
    if bios::sysinit1::round_mem(17, 0, 0x100).is_ok()
        && bios::sysinit2::skip_comment(b"; note\nBUFFERS=4\n") == 7
        && bios::sysinit2::parse_switches(b"\r", b"ABCD").is_err()
        && bios::sysinit2::parse_switches(b"/D\r", b"ABCD") == Ok(1 << 3)
        && bios::sysinit2::asciiz_len(b"COMMAND.COM\0") == 12
    {
        // The round and the comment skip agreed. The serial lines stay.
    }
    if crate::dos::exec::invoke() {
        puts("CMDOK\n");
        if let Some(payload) = crate::dos::exec::command_payload() {
            let _size = cmd::command::envdata::environ_len();
            if cmd::command::envdata::payload_is_comspec(payload) {
                puts("COMSPEC=");
                for &byte in cmd::command::envdata::ECOMSPEC {
                    putc(byte);
                }
                puts("\n");
            }
        }
        cmd::command::tcmd2a::print_default_prompt();
        puts("\n");
        let mut shell = cmd::command::tenv::Environment::new();
        shell.dispatch(b"VER");
        // The prompt loop runs the next typed line. Bare ECHO prints message
        // 1057. A tail that is not ON or OFF is printed from 82h. OFF clears
        // the bit, so the following bare ECHO prints the off text.
        shell.dispatch(b"ECHO\r");
        shell.dispatch(b"ECHO hello\r");
        shell.dispatch(b"ECHO OFF\r");
        shell.dispatch(b"ECHO\r");
        // `BREAK` with an empty tail reads CntCFlag. It starts clear, so the
        // line is message 1055 with "off". ON stores DL=1, and the next read
        // prints "on".
        shell.dispatch(b"BREAK\r");
        shell.dispatch(b"BREAK ON\r");
        shell.dispatch(b"BREAK\r");
        // `VERIFY` reads VERFLG. It starts clear. ON stores AL bit 0 through
        // AH=2Eh, and the next AH=54h read prints message 1056 as on.
        shell.dispatch(b"VERIFY\r");
        shell.dispatch(b"VERIFY ON\r");
        shell.dispatch(b"VERIFY\r");
        // Bare PATH. The environment value after PATH= is a NUL, so
        // print_path takes the empty path and prints message 1049.
        // The next line stores `\DOS`. A second bare PATH prints it.
        shell.dispatch(b"PATH\r");
        shell.dispatch(b"PATH \\DOS\r");
        shell.dispatch(b"PATH\r");
        // CLS asks INT 2Fh and clears through INT 10h.
        shell.dispatch(b"CLS\r");
        // AH=43h. Archive comes from the directory attribute. +R sets bit 0.
        shell.dispatch(b"ATTRIB COMMAND.COM\r");
        shell.dispatch(b"ATTRIB +R COMMAND.COM\r");
        // AH=57h stores 1984-03-04 01:02, then DIR prints that stamp.
        cmd::command::tcmd1a::stamp_command();
        // SET with no tail lists the block. FOO=BAR is stored, then listed.
        shell.dispatch(b"SET FOO=BAR\r");
        shell.dispatch(b"SET\r");
        // DATE and TIME read AH=2Ah and AH=2Ch, then store a new value.
        shell.dispatch(b"DATE\r");
        shell.dispatch(b"DATE 3-4-1984\r");
        shell.dispatch(b"DATE\r");
        shell.dispatch(b"TIME\r");
        shell.dispatch(b"TIME 1:02:03.04\r");
        shell.dispatch(b"TIME\r");
        // VOL searches for a volume label. This disk has none.
        shell.dispatch(b"VOL\r");
        // DIR lists the boot root. The names are the 8.3 fields.
        shell.dispatch(b"DIR\r");
        // TYPE prints the file bytes. NONE is the missing-file line.
        shell.dispatch(b"TYPE COMMAND.COM\r");
        shell.dispatch(b"TYPE NONE\r");
        // DEL removes IO.SYS. The following DIR no longer lists it.
        shell.dispatch(b"DEL IO.SYS\r");
        shell.dispatch(b"DIR\r");
        // REN changes COMMAND.COM to FOO.COM. The next DIR shows the new name.
        shell.dispatch(b"REN COMMAND.COM FOO.COM\r");
        shell.dispatch(b"DIR\r");
        // COPY adds BAR.COM. The next DIR lists it.
        shell.dispatch(b"COPY FOO.COM BAR.COM\r");
        shell.dispatch(b"DIR\r");
        // The copy wrote the source bytes. TYPE reads them back with AH=3Fh.
        shell.dispatch(b"TYPE BAR.COM\r");
        // MD makes SUB, CD prints it as the directory header, RD removes it.
        shell.dispatch(b"MD SUB\r");
        shell.dispatch(b"DIR\r");
        shell.dispatch(b"CD SUB\r");
        shell.dispatch(b"DIR\r");
        shell.dispatch(b"CD \\\r");
        shell.dispatch(b"RD SUB\r");
        shell.dispatch(b"DIR\r");
        // PROMPT $P$G expands the current directory and the greater-than sign.
        shell.dispatch(b"PROMPT $P$G\r");
        shell.dispatch(b"IF EXIST FOO.COM ECHO ifok\r");
        shell.dispatch(b"CALL\r");
        shell.dispatch(b"GOTO NOWHERE\r");
        shell.dispatch(b"FOR %A IN (X) DO ECHO forok\r");
        shell.dispatch(b"SHIFT\r");
        // FCB open of FOO.COM after the rename. The line is the file length
        // and the sequential-read return.
        dos::fcb::show();
        // Extend cluster 3 on the FAT12 image, then the same step on a FAT16 table.
        dos::rom::show();
        // Seek within BAR.COM, write the first byte back, and close the handle.
        dos::close::show();
        // Media check from the rest of FAT.ASM: unchanged, changed, dirty, error.
        dos::fat::media_show();
        // Drive parameter block, buffer cache, and the sector read under it.
        dos::buf::show();
        // `MetaCompare` from DIR.ASM against the live root.
        dos::dir::show();
        // `DOS_DUP` increments the local SFT reference count.
        dos::dup::show();
        // `$LockOper` from LOCK.ASM. The second lock of the same range fails.
        dos::lock::show();
        // IOCTL on a closed handle, removable media, and the path checks.
        dos::ioctl::show();
        // Exec rejects AL=2, misses a name, and loads COMMAND.COM as a COM file.
        dos::exec::show();
        // First-fit arena: two blocks, free the first, then a request that does not fit.
        dos::alloc::show();
        // The last error is the invalid drive, with its class, action, and locus.
        dos::ms_code::show();
        // Critical section enter/leave after the sharer patches the leading RET.
        dos::crit::show();
        // Abort closes every JFN. A later close of handle 5 is invalid.
        dos::abort::show();
        // INT 24h: write-protect on the FAT, then ignore, retry, fail, and abort.
        dos::ctrlc::show();
        // Device request header, the >32 MB block error, and a file read at EOF.
        dos::dev::show();
        // Free and total clusters from the boot FAT.
        dos::dinfo::show();
        // Device names, attribute match, and an empty root path.
        dos::dir2::show();
        // Make a directory, change to it, remove it, and reject a device and the root.
        dos::dircall::show();
        // Create a file, refuse a second copy, and reject a read-only replace.
        dos::create::show();
        // Delete misses a name, refuses a directory and a read-only file, then marks E5.
        dos::delete::show();
        // Split a read into partial sectors, and report a lock violation.
        dos::disk2::show();
        // Divide a sector number, shift it to a cluster, and check a buffer.
        dos::disk3::show();
        // File time, date, size, and an attribute DOS will not store.
        dos::finfo::show();
        // Microsoft build switches: escape, cancel, and high memory.
        dos::highsw::show();
        // Internal directory search: one file, then no further match.
        dos::isearch::show();
        // Extended attributes: maximum size, a short buffer, and a set.
        dos::extattr::show();
        // IFS service: enter and leave DOS, stack size, and a path copy.
        dos::ifs::show();
        // IBM IOCTL build: the switch is true and the media-id call is 66h.
        dos::msioctl::show();
        // Standard IOCTL build: the switch is off and the calls match IBM.
        dos::stdioctl::show();
        // Wait, Ctrl-C exit, keep, stay resident, and abort.
        dos::msproc::show();
        // IBM process switches: escape is null and DOS stays low.
        dos::mssw::show();
        // IBM absolute disk read: BIOS status 2 becomes 80h.
        dos::mscode::show();
        // Ctrl-C: same stack retries, carry on a new stack aborts.
        dos::msctrlc::show();
        // Console input with no echo, including a zero scan code.
        dos::mscpmio::show();
        // DOS printf: hex, newline, character, and string.
        dos::dosprint::show();
        // Buffered console input: empty buffer, skipped Ctrl-F, and a full line.
        dos::kstrin::show();
        // IBM dispatch: CALL 5 stops at 24h and F8h is not an OEM hook.
        dos::msdisp::show();
        // Standard process build: escape is 1Bh and cancel is Ctrl-X.
        dos::stdproc::show();
        // Standard dispatch table: CALL 5 limit 24h, INT 21h limit 6Ch.
        dos::stdtable::show();
        // Standard dispatch: F8h installs an OEM handler, F9h fails until then.
        dos::stddisp::show();
        // Standard Ctrl-C: Ctrl-N turns the printer off, Ctrl-C breaks.
        dos::stdctrlc::show();
        // Standard absolute disk calls leave the BIOS status untranslated.
        dos::stdcode::show();
        // Standard console I/O: escape is 1Bh and a ready A is returned.
        dos::stdcpmio::show();
        // OEM message constants: OEM number FF and code page 437.
        dos::stddosme::show();
        // Share printf prints user id, a colon, and process id.
        dos::shrprint::show();
        // Non-resident constants: data version 1 and retry count 3.
        dos::nstdos::show();
        // Internal open: access, redirector verb, and read-only FCB mode.
        dos::open::show();
        // Printf: segmented string, message out, and a hex number.
        dos::print::show();
        // Process exit: wait clears the code, and keep holds at least six paragraphs.
        dos::proc::show();
        // Rename: same drive, question-mark names, and a freed source.
        dos::rename::show();
        // Segment checks: DS against SS or CS, and list walks that stop at FFFFh.
        dos::segcheck::show();
        // Sharing: slot 1 checks, slot 2 closes, and only reply 1 retries.
        dos::share::show();
        // Server call: functions 7 through 9 skip the ids, and 11 returns the table.
        dos::srvcall::show();
        // Alternate vectors: escape is 1Bh and terminate lives at F0h.
        dos::stdasw::show();
        // Standard data: debug is off, and alternate vectors are off.
        dos::stddata::show();
        // Standard switches: alternate vectors off, IBM cancel is Escape.
        dos::stdsw::show();
        // Line editor: Ctrl-F is skipped, and escape copies from the template.
        dos::strin::show();
        // Date and time: 1980 through 2099, with leap day and packed fields.
        dos::time::show();
        // Handles: a slot of FF is unused, and a busy file is reused by its owner.
        dos::util::show();
        // Dispatch notes: memory is section 4, and call 18h is not listed.
        dos::dispatch::show();
        // Country tables: code page 437, and 82h maps to E.
        dos::dosmes::show();
        // Drive macros: functions 7 and 8 update in-use, and A is unit 0.
        dos::r#macro::show();
        // Path canonicalization: slash, drive letters, and dot-dot backup.
        dos::macro2::show();
        // String compare folds case and slash, and a full table index is rejected.
        dos::misc2::show();
        // New directory nodes: the root cannot grow, and a second label is rejected.
        dos::mknode::show();
        // Initialized data: debug marker, 128-byte sectors, and sharer slots.
        dos::msconst::show();
        // ROM cartridges are scanned from C000 and stop at F000.
        dos::mshalo::show();
        // Init walks the fake-version list and stops the device chain at the clock.
        dos::msinit::show();
        // Allowed errors per call. The dispatch vectors stay where they are.
        dos::ms_table::show();
        // Command equates, Ctrl-C, and the transient reload checks.
        cmd::command::comequ::show();
        cmd::command::command1::show();
        cmd::command::command2::show();
        cmd::command::comseg::show();
        cmd::command::comsw::show();
        // COPY scans the destination before it decides ASCII or binary.
        cmd::command::copy::show();
        cmd::command::copypr1::show();
        cmd::command::copypr2::show();
        // Token scan: a return ends the line, and stars become question marks.
        cmd::command::cparse::show();
        cmd::command::fordata::show();
        cmd::command::ifequ::show();
        // Startup switches: /C stops the scan, and /MSG needs /P.
        cmd::command::init::show();
        // The init parser leaves dates, drives, and quotes off.
        cmd::command::iparse::show();
        cmd::command::parse2::show();
        // A complete path is not searched again. Empty PATH pieces are skipped.
        cmd::command::path1::show();
        // .COM outranks .EXE and .BAT. A named extension stops the search.
        cmd::command::path2::show();
        // Resident flags: echo starts on, and the last command looks external.
        cmd::command::rdata::show();
        // Critical errors: ignore is checked before retry, and /F fails.
        cmd::command::rucode::show();
        // IF checks NOT, then ERRORLEVEL, then EXIST, then a string.
        cmd::command::tbatch2::show();
        // CTTY wants a device, and CHCP distinguishes a missing country file.
        cmd::command::tcmd2b::show();
        // Pipes outrank FOR, which outranks batch. A bare drive changes drives.
        cmd::command::tcode::show();
        // MKDIR reports an existing directory only after access denied.
        cmd::command::tenv2::show();
        // FOR rejects a nested loop before it removes pipes.
        cmd::command::tfor::show();
        // A switch is consumed before the path is classified.
        cmd::command::tmisc2::show();
        // Line feed is not an end of line, and a quote is not special.
        cmd::command::tparse::show();
        // A date substitution is copied, then the original value is cleared.
        cmd::command::tprintf::show();
        // Utility messages stay in table order, including each substitution.
        cmd::command::tranmsg::show();
        // Transient bytes stay in source order, including shared names.
        cmd::command::tspc::show();
        // /E accepts 160 through 32768. /C is not the last switch.
        cmd::command::uinit::show();
        // COMP is a COM file. Its entry is 100h.
        cmd::comp::comp1::show();
        // Ten mismatches end a compare. The end mark is 1Ah in either file.
        cmd::comp::comp2::show();
        // Extended open keeps the code-page check in bit 8.
        cmd::comp::compeq::show();
        // Compare offsets are hex dwords. Name prompts read a buffered line.
        cmd::comp::compms::show();
        // COMP parses files, drives, and switches. Dates and numbers stay off.
        cmd::comp::compp::show();
        // A matching ordinal is the first file. Any other ordinal is the second.
        cmd::comp::comppar::show();
        // A bad DOS version is message 1 in the utility class.
        cmd::comp::compsm::show();
        // A blank line leaves assemble mode. INT 3 stays one byte.
        cmd::debug::debasm::show();
        // A range that lands on the segment boundary is kept.
        cmd::debug::debcom1::show();
        // Flag names run from the high bit. The same bit twice is an error.
        cmd::debug::debcom2::show();
        // A near call is three bytes. INT 3 is one. Trace defaults to one.
        cmd::debug::debcom3::show();
        // Sixteen-bit names run AX, CX, DX, BX. IP starts at 100h.
        cmd::debug::debconst::show();
        // The create-process dword keeps offset 100h beside the segment.
        cmd::debug::debdata::show();
        // The version must be at least 40h before a page call runs.
        cmd::debug::debems::show();
        // The two-letter names keep the first letter in the low byte.
        cmd::debug::debequ::show();
        // A zero marker selects the write-protect messages.
        cmd::debug::deberr::show();
        // A failed load is displayed and the carry is kept.
        cmd::debug::debmes::show();
        // The default length is the dump size shifted right twice.
        cmd::debug::debuasm::show();
        // X runs expanded memory because the system-version switch is off.
        cmd::debug::debug::show();
        // A 720 KB drive compares only with another 720 KB drive.
        cmd::diskcomp::compinit::show();
        // A function alone leaves the low byte in place.
        cmd::diskcomp::dcmpmacr::show();
        // Strike-a-key clears the buffer before a silent read.
        cmd::diskcomp::dcompms::show();
        // End of line is accepted before a nonzero code is an error.
        cmd::diskcomp::dcomppar::show();
        // Drive-only syntax and switches are the included parser pieces.
        cmd::diskcomp::dcompp::show();
        // A wrong DOS version is utility class on the error handle.
        cmd::diskcomp::dcompsm::show();
        // A volume serial is ignored only for media F0h with signature 28h or 29h.
        cmd::diskcomp::diskcomp::show();
        // Copy fills twelve lists and does not patch a 386.
        cmd::diskcopy::copyinit::show();
        // There is no message 24. A serial field is right aligned and zero padded.
        cmd::diskcopy::dcopyms::show();
        // The only switch is /1, and drive text is capitalized by character.
        cmd::diskcopy::dcopypar::show();
        // File-table capitalization is included with drive syntax.
        cmd::diskcopy::dcopyp::show();
        // Classes are linked in the order 1, 2, then A.
        cmd::diskcopy::dcopysm::show();
        // A function alone leaves the low byte in place.
        cmd::diskcopy::dcpymacr::show();
        // A 40-track source is not formatted onto 15-sector media.
        cmd::diskcopy::diskcopy::show();
        // Append takes one parameter. A bare line takes fewer than two.
        cmd::edlin::edlcmd1::show();
        // The guest adds six to the letter A, then prints the rest of the line.
        guest::run::show();
    } else {
        // EXEC returned carry. `COMERR` is only that path.
        bios::sysinit1::com_err();
    }
}

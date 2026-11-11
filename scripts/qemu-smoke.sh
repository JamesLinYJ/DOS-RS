#!/bin/bash
# Boot both kernels under QEMU. Map isa-debug-exit status 33 to process status 0.
set -euo pipefail

root="$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"

doctor() {
	local missing=0
	command -v qemu-system-i386 >/dev/null || { echo "missing qemu-system-i386" >&2; missing=1; }
	command -v qemu-system-x86_64 >/dev/null || { echo "missing qemu-system-x86_64" >&2; missing=1; }
	command -v timeout >/dev/null || { echo "missing timeout" >&2; missing=1; }
	rustc +nightly-2026-11-08 --version | grep -q '1.102.0-nightly' || {
		echo "want rustc 1.102.0-nightly from nightly-2026-11-08" >&2
		missing=1
	}
	if [[ ! -f "$root/target/i686-unknown-none/release/dos4-rs" ]]; then
		echo "missing i686 kernel. Build it before driving." >&2
		missing=1
	fi
	if [[ ! -f "$root/target/x86_64-unknown-none/release/dos4-rs" ]]; then
		echo "missing x86_64 kernel. Build it before driving." >&2
		missing=1
	fi
	if [[ "$missing" -ne 0 ]]; then
		exit 1
	fi
	echo "doctor ok"
}

run_one() {
	local name="$1"
	local qemu="$2"
	local kernel="$3"
	local out
	out="$(mktemp)"
	set +e
	timeout 15 "$qemu" \
		-kernel "$kernel" \
		-display none \
		-serial stdio \
		-no-reboot \
		-device isa-debug-exit,iobase=0xf4,iosize=0x04 \
		-drive if=floppy,index=0,format=raw,file="$root/disks/msload.img" \
		-m 64 \
		>"$out" 2>&1
	local status=$?
	set -e
	if [[ "$status" -ne 33 ]]; then
		cat "$out" >&2
		echo "$name qemu status $status, want 33" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "dos4-rs $name" "$out"; then
		echo "$name missing banner" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "AX=0004 BX=FF00 CX=0000 CF=1" "$out"; then
		echo "$name missing AH=30h result" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "BIOS" "$out"; then
		echo "$name missing BIOS console line" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "AUX" "$out"; then
		echo "$name missing AUX line" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DSK8102" "$out"; then
		echo "$name missing disk status" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DSKF55AA" "$out"; then
		echo "$name missing floppy boot signature" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MED0001" "$out"; then
		echo "$name missing media check" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "CLK00000000" "$out"; then
		echo "$name missing clock line" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "CLKP000000012400" "$out"; then
		echo "$name missing forced clock line" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "CLKW0001000D" "$out"; then
		echo "$name missing clock write line" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "PRN" "$out"; then
		echo "$name missing printer line" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "PRNS" "$out"; then
		echo "$name missing printer status" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "PRNR0050" "$out"; then
		echo "$name missing printer retry count" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "LOADOK" "$out"; then
		echo "$name missing MSLOAD line" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "LOAD0200" "$out"; then
		echo "$name missing MSLOAD size" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "PRNT0001" "$out"; then
		echo "$name missing printer tilbusy count" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "SYN0030" "$out"; then
		echo "$name missing SYSINIT number" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "Unrecognized command in CONFIG.SYS" "$out"; then
		echo "$name missing CONFIG command line" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "BUF0002" "$out"; then
		echo "$name missing default buffers" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "PAR0002" "$out"; then
		echo "$name missing paragraph round" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMSPEC=\\\\COMMAND.COM" "$out"; then
		echo "$name missing COMSPEC" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "Microsoft DOS             Version 4.0" "$out"; then
		echo "$name missing VER line" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "ECHO is on" "$out"; then
		echo "$name missing ECHO state" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "hello" "$out"; then
		echo "$name missing ECHO text" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "ECHO is off" "$out"; then
		echo "$name missing ECHO off" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "BREAK is off" "$out"; then
		echo "$name missing BREAK off" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "BREAK is on" "$out"; then
		echo "$name missing BREAK on" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "VERIFY is off" "$out"; then
		echo "$name missing VERIFY off" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "VERIFY is on" "$out"; then
		echo "$name missing VERIFY on" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "No Path" "$out"; then
		echo "$name missing PATH" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "PATH=\\\\DOS" "$out"; then
		echo "$name missing PATH setter" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "CLS" "$out"; then
		echo "$name missing CLS" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "01-01-80 00:00" "$out"; then
		echo "$name missing DIR date" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "ATTR A COMMAND.COM" "$out"; then
		echo "$name missing ATTRIB" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "ATTR AR COMMAND.COM" "$out"; then
		echo "$name missing ATTRIB +R" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "03-04-84 01:02" "$out"; then
		echo "$name missing file time" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "FOO=BAR" "$out"; then
		echo "$name missing SET" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "Current date is Tue 01-01-1980" "$out"; then
		echo "$name missing DATE" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "Current date is Sun 03-04-1984" "$out"; then
		echo "$name missing DATE set" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "Current time is  00:00:00.00" "$out"; then
		echo "$name missing TIME" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "Current time is  01:02:03.04" "$out"; then
		echo "$name missing TIME set" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "Volume in drive A has no label" "$out"; then
		echo "$name missing VOL" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "Directory of  A:\\\\" "$out"; then
		echo "$name missing DIR" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMMAND  COM" "$out"; then
		echo "$name missing DIR file" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if [ "$(grep -o "\\\\COMMAND.COM" "$out" | wc -l)" -lt 3 ]; then
		echo "$name missing TYPE" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "File not found" "$out"; then
		echo "$name missing TYPE error" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if [ "$(grep -c "IO       SYS" "$out")" -ne 1 ]; then
		echo "$name missing DEL" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "FOO      COM" "$out"; then
		echo "$name missing REN" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "BAR      COM" "$out"; then
		echo "$name missing COPY" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "SUB      <DIR>" "$out"; then
		echo "$name missing MD" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "Directory of  A:\\\\SUB" "$out"; then
		echo "$name missing CD" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if [ "$(grep -c "SUB      <DIR>" "$out")" -ne 1 ]; then
		echo "$name missing RD" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "A:\\\\>" "$out"; then
		echo "$name missing PROMPT" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "ifok" "$out"; then
		echo "$name missing IF" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "Batch file missing" "$out"; then
		echo "$name missing CALL" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "Label not found" "$out"; then
		echo "$name missing GOTO" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "forok" "$out"; then
		echo "$name missing FOR" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "FCB FOO.COM 0010 SR00 NXFF AL03" "$out"; then
		echo "$name missing FCB" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "FAT12 0004 EOF REL EOF FAT16 0003 EOF REL EOF" "$out"; then
		echo "$name missing FAT chain" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "SEEK 00000001 00000010 CL00 CL06 AR21" "$out"; then
		echo "$name missing seek or close" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "FATMED NC CHG CDSFFFF DTY ERR FST" "$out"; then
		echo "$name missing FAT media check" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DPB 0200 003D F8 BUF F8FF EV11 HIT DIR IO SC" "$out"; then
		echo "$name missing buffer or DPB" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "LOCK OK VL21 UL00" "$out"; then
		echo "$name missing LOCK" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DUP REF0002" "$out"; then
		echo "$name missing DUP" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DIRSCAN FOO HIT BAR HIT ZZZ MISS" "$out"; then
		echo "$name missing directory scan" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "IOCTL H06 REM00 FIX01" "$out"; then
		echo "$name missing IOCTL" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "PATHLEN 4 OK 67 BAD" "$out"; then
		echo "$name missing path length" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "PATH E5 LEN05 DR0F" "$out"; then
		echo "$name missing PATH" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "EXEC FN01 NF02 COM" "$out"; then
		echo "$name missing EXEC" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "EXEC LD 0100 SP00FE" "$out"; then
		echo "$name missing EXEC load" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "CRIT 8001 8101 8002 8102" "$out"; then
		echo "$name missing critical section" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "ABORT H06" "$out"; then
		echo "$name missing abort" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "I24 AREA02 ER0013 CL0B AC07 LC02 IG00 RT01 FL03 AB" "$out"; then
		echo "$name missing INT 24" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DEV HS8107 L16 C04 W08 EOF1A Z" "$out"; then
		echo "$name missing device call" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DINFO DX003A BX003C CX0200 AL01 AHF8" "$out"; then
		echo "$name missing disk info" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DIR2 CONC3 NULC4 ND VOLND MAT01 RT00 SR0000FFFF ENT40" "$out"; then
		echo "$name missing DIR2" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DIRCALL MK00 CX0002 RM00 DV05 RT05 EX05 DOT202E2E2E" "$out"; then
		echo "$name missing DIRCALL" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "CREATE NW20 TR00 EX50 RO05 AT05 DA05 DIR05 EM02" "$out"; then
		echo "$name missing CREATE" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DELETE NF02 DR05 RO05 E500" "$out"; then
		echo "$name missing DELETE" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DISK2 BD019C 0001 004C EX0000 0000 0200 LK0021" "$out"; then
		echo "$name missing DISK2" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DISK3 DIV0004 0000 0000 SHR0002 EOF0026 CL01 AC04 FULL0027 HIT" "$out"; then
		echo "$name missing DISK3" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "FINFO AT21 TM0841 DT0864 SZ0010 DIR10 DV02 BAD05 CL07 AC04 LC01 SET21 FO80" "$out"; then
		echo "$name missing file info" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "HIGHSW MSFFFF IBM0000 HIFFFF ESC1B CX18 PRN0000 EXTFFFF" "$out"; then
		echo "$name missing high switches" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "ISEARCH DR01 AT21 E5E5 Q01 NF12 NX12 PN03 DVFFFF" "$out"; then
		echo "$name missing internal search" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "XA G0 C0002 G1 OK G4 C0002 L0000 N0 C0002 S OK" "$out"; then
		echo "$name missing extended attributes" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "IFS IN01 OUT00 ST0180 RT0003 ERCF EQ A:\\\\DOS" "$out"; then
		echo "$name missing IFS" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MSIOCTL IBMFFFF GET00 SET01 GEN0D MED66 SEC3F" "$out"; then
		echo "$name missing IBM IOCTL" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "STDIOCTL IBM0000 GET00 SET01 GEN0D MED66 SEC3F" "$out"; then
		echo "$name missing standard IOCTL" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MSPROC IBMFFFF ESC00 CAN1B PRNFFFF WT0000 EX0122 KP200A SR2007 AB0000 RS06" "$out"; then
		echo "$name missing process exit" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MSSW IBMFFFF WG0000 HM0000 ESC00 CAN1B PRNFFFF EXTFFFF" "$out"; then
		echo "$name missing process switches" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MSCODE IBMFFFF MAP0280 MAP0640 SCFF DRV00 OK DRVFF BAD" "$out"; then
		echo "$name missing absolute disk" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MSCTRLC IBMFFFF MSG03 RT00 ABFF" "$out"; then
		echo "$name missing Ctrl-C" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MSCPMIO IBMFFFF IN41 SC00 IN00 SC01 STR02" "$out"; then
		echo "$name missing console input" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DOSPRINT X=0A1BCRLFAOK" "$out"; then
		echo "$name missing DOS print" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "KSTRIN Z00 DROP HI02 CR SKAB FLAB" "$out"; then
		echo "$name missing string input" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MSDISP IBMFFFF MC6C C24OK C25BAD OEMNO" "$out"; then
		echo "$name missing IBM dispatch" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "STDPROC IBM0000 ESC1B CAN18 PRN0000 HM0000 WT0000" "$out"; then
		echo "$name missing standard process" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "STDTABLE IBM0000 CALL24 COM6C" "$out"; then
		echo "$name missing standard table" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "STDDISP IBM0000 C24OK C25BAD SETF8 BADF9" "$out"; then
		echo "$name missing standard dispatch" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "STDCTRLC IBM0000 N0E OFF C03 BRK PLAIN" "$out"; then
		echo "$name missing standard Ctrl-C" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "STDCODE IBM0000 MAP0202 MAP0606 DRV00 OK DRVFF BAD" "$out"; then
		echo "$name missing standard disk" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "STDCPMIO IBM0000 ESC1B CAN18 IN41 SC00" "$out"; then
		echo "$name missing standard console" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "STDDOSME DBG00 OEMFF CP01B5 ENT06" "$out"; then
		echo "$name missing DOS messages" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "SHRPRINT 0001:0002" "$out"; then
		echo "$name missing share print" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "NSTDOS IBM0000 DV0001 RT0003 OEMFFFF" "$out"; then
		echo "$name missing non-resident DOS" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "OPEN AC00 BAD0C NET70 NA0C NS0C BLK0C RO05 RD00 FCB00 NFC70 DIR05 VOL05 DEV00 FO80 NF02 NP03 V16 V2E SHR20 RT00 SHOK PID0001" "$out"; then
		echo "$name missing open" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "PRINT EN8007 SEL1234 ABz CD MN0A1B LV8107" "$out"; then
		echo "$name missing printf" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "PROC WT00AB Z0000 EX0022 CC0122 KP2006 K72007 KF0000 OV3000 AB0000 RS06 AT0088 SV000A" "$out"; then
		echo "$name missing process exit" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "RENAME NET11 DRV11 NF02 KEEP03 DL05 DV05 SH20 DP03 DF03 DZ03 DD05 BD05 EX05 CD05 SF00 RNFILE    DOC MVE5" "$out"; then
		echo "$name missing rename" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "SEGCHECK DS01 CS00 ES01 DP00 DP01 SF00 SF01 NP00 OW01 ZR01 BU01 BE00" "$out"; then
		echo "$name missing segment check" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "SHARE S01 OK DN01 NO S02 OK EC NO RD00 AL18 CX0001 RT01 FL00 MFT0000 TRYOK FAIL" "$out"; then
		echo "$name missing sharing" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "SRVCALL F6U0006 NSABCD N9ABCD F0A U0042 P0043 TB0B IDABCD U0007 ER0001 LC01 SHFF CPY06 SK01 SG02 EE0020 AC04 CL05 LO07 PT1234 ES5678 AD0003 SW8010 CM0001 NM05 CU03 CP04 GL09 NO SP1125" "$out"; then
		echo "$name missing server call" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "STDASW MSFFFF IBM0000 WG0000 AVFFFF HM0000 ESC1B CAN18 PRN0000 EXTFFFF INTF0" "$out"; then
		echo "$name missing alternate switches" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "STDDATA DBG00 INSFFFF IBM0000 AV0000 RB0000 HM0000 ESC1B CAN18" "$out"; then
		echo "$name missing standard data" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "STDSW MSFFFF IBM0000 WG0000 AV0000 RB0000 HM0000 ESC1B CAN18 PRN0000 EXTFFFF INT22 IESC00 ICAN1B WESC1F" "$out"; then
		echo "$name missing standard switches" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "STRIN Z00 Hi L02 C03 BS01 CU00 CA01 BEL01 F6AB CAN01 W17 CPX CPLXY INS1 ESC1B CZ1A SKY TAB06" "$out"; then
		echo "$name missing string input" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "TIME Y07BC M01 D01 W02 C0000 EARLYFF LATEFF MFF DFF FEB1D C003B N29FF T00 HRFF MNFF SCFF HDFF P0061 K63D4 R07BC0301" "$out"; then
		echo "$name missing time" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "UTIL H06 S01 B06 T00 T01 NO JF01 JF04 SN02 OW00 NB04" "$out"; then
		echo "$name missing handle utilities" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DISPATCH DSK01 DEV02 MEM04 NET05 GAP03 0E NONE 3F READ 40 WRITE 48 ALLOC 00 CLOSE 2A DEVIO 36 DINFO 18 MISS 4B BARE" "$out"; then
		echo "$name missing dispatch notes" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DOSMES OEMFF IB00 CP01B5 ENT06 US0001 DT00 CUR24 TH2C DEC2E DB0000 MA41 M8245 FA01 FS00 FD00 CL41 CH43 ES53 IB4D" "$out"; then
		echo "$name missing country tables" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MACRO A7USE A8CLR A70F AS111E DP02 DPNO ICA4000 IC0000 CD00 CDNO U0 U1 UP111F U601 L01 V00 E1A SP0F SPOK NUL00" "$out"; then
		echo "$name missing drive macros" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MACRO2 SL5C NU1 CH0 DRV02 DRV00 DRVFF TX413A P1 G0 N1 CN5CBAR UP03 MF02 SC1 SC0 NS00 NSFF UNC DEV FIL EM02" "$out"; then
		echo "$name missing path canonicalization" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MISC2 EQ1 NE0 CP415C42 RAW61 LN03 UC41 U8245 TDOK TD05BAD NET1 LOC0 SNET1 ID00 ID03 FV0000 FV1234 MSG0000 MSG0004 FSFFFF FIA" "$out"; then
		echo "$name missing string helpers" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MKNODE BD0005 RT GR DOT2E20 DD2E2E EX01 MD05 NT03 RO05 DV03 BP04 MT04 EO07 NW00 SH06 DIR00 C00000100 VC00 VC01 DA00 FLC0 FA20 FL42 VOL1" "$out"; then
		echo "$name missing directory nodes" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MSCONST DV0001 BUG42554720 RL0001 MX0080 NA8004 HS0001 EMSFF SC0001 J1106" "$out"; then
		echo "$name missing DOS constants" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MSHALO NO HITC000000C TERM NEXTHITC080000C STOP MARKHITC000000C SK0080 SK0100 IWFFFF DMA0080 ST0000 STFFF0 ST1000 ST0000" "$out"; then
		echo "$name missing ROM cartridge scan" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MSINIT FT0084 WN04 TG5872 SW000A ID0006 AL8005 PG0000 PG0001 PG0001 PG0002 PG1000 I21111100AB I2900000000 I2F222200AB EA CE3333 SG00AB HM00 AV0000 JFN00 N11 AR5A OW0000 SZ9F00 CN0003 CK02 IN05 MX0201 UN03 SK01 MV01 MV00 L0221 LFFFF FAFF SH00AB000E" "$out"; then
		echo "$name missing DOS initialization" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "MSTAB VR0004 MAP3D02 3D05 3B02 3B03 0163 3E06 4F12 6C50 BN0000 BN001F BN00440A BN00450D FO0002 NS0002 NM000B SP04 SCFF SQFFFFFFFF TG0000 SV0000" "$out"; then
		echo "$name missing DOS call tables" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMEQU SYM3E PG0019 NL01 WL05 CB0080 NT000A EC40 EA00 EB01 IW4E49 DW4F44 SD8003 SC801C Y01 N00 ELFFFF EI0D RN01 RS03 RF05 RD06 DA07 TM08 OK00 II01 IS02 IC04 FA08 RY10 IG20 AR0040" "$out"; then
		echo "$name missing command equates" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "CMD1 EX02E5 EX08E3 EX0BE2 EX05E6 EX01E1 K0000000000000000000000 K0100000000000000000000 K0000000000000000000000 K0200000000000000000000 K0200000000000000000000 K0000000000000000000000 K0300000000000000000001 K0400000000000000000001 K0301000000000000000001 K0304000000000100000001 K0405000000000000000001 K0307000200000200000001 K03000000FFFF0000000000" "$out"; then
		echo "$name missing resident control" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "CMD2 00010200 C0000 C0009 S0081 CP0080 SZ0001 SZ0001 SZ0002 FT00 FT01 AL3000 AL2000 ALF000 RM00 RM01 RM00 CL10 IN01 D5C D2F VF00 VF01 SC00 SC01 SM01 SM00" "$out"; then
		echo "$name missing transient reload" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMSEG R08 T04 RAFC TA08 OR12345678 OT9ABC" "$out"; then
		echo "$name missing command segments" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMSW IBFFFF CR0000 MS0000 BFFFFF IN0000 KN0000" "$out"; then
		echo "$name missing command switches" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COPY MS0200000000 ON0001000000 TW0000000000 TM0100000000 OW0400000000 CD0001010400 CC0000010400 BB0001010000 VV0001000001 BD0300000000 BP0300000000 DR0002000000 SL0003000000 SE0500000000 DA01 DC03 DN00 RB0500 RA0201 RZ0201" "$out"; then
		echo "$name missing COPY scan" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "CPR1 010112 010112 020101 030000" "$out"; then
		echo "$name missing COPY flush" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "CPR2 A040004 B000800 K090100 N000000 SZ0E ST3F EQ01 NE00" "$out"; then
		echo "$name missing COPY names" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "CPARSE F I00 S0000 C00 L03 R I00 S0000 C01 L00 P I80 S0000 C00 L01 V I01 S8010 C00 L01 Q I01 SC000 C00 L01 E I01 SC000 C01 L00 S I02 S0000 C00 L07 A I02 S0000 C00 L07 D I02 S0000 C00 L03" "$out"; then
		echo "$name missing command parser" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "FORDATA AR0544 FR064C" "$out"; then
		echo "$name missing FOR workspace" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "IFEQU IB01 IB00" "$out"; then
		echo "$name missing IF switches" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "INIT VR00 VR01 VR02 AP00 AP00 AP01 SP5C SP2F CD01 LT41 LT43 TP3000 TPF000 EV000A EV000B PC00 PC01 PC01 PC00 CC000D CC000E CM01D2 CM01D1 P000100000000000A D000001000000000A PD000101000000000A DP000101000000000A C000001010000000A F0100FF000000000A FF0100FF000100000A M0000FF000001000A MM0000FF000101000A E0000FF000000000B CP000001010000000A PC000001010000000A BC000001010000000A BX0000FF000300000A H00 H02 H03 H01 DV00 DV00 DV00 DV01" "$out"; then
		echo "$name missing command startup" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "IPARSE DT00 TM00 CX00 KY00 V200 V300 QU00 DR00" "$out"; then
		echo "$name missing init parser" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "PARSE2 0102010000000000 0002010000000000 0102020010001000 0102020000000000 0102400000000001 0102010000000002 0100000000000000 0100010000000000  IX0016" "$out"; then
		echo "$name missing argument array" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "PATH1 FL01 FL01 FL00 PS2F PS5C DR01 DR00 KS00 KS00 KS01 EM000001 DN010104 JN42494E5C464F4F ND42494E5C464F4F" "$out"; then
		echo "$name missing program search" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "PATH2 DR01 DR03 DR00 BD00 NC08 NE04 NB04 SB02 SX00 W464F4F2E3F3F3F K464F4F2E434F4D E00 E00 SV0055 SV0056 SV0000 AD02C4" "$out"; then
		echo "$name missing command lookup" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "RDATA EC010100 SU01 EX01 VFFFFF IN01 SW2D DR2F XAFFFF PM00 SG0000 BT0000 PF00 AP00 DSFF NS00 LT41 RD7264 WR7774 EN12" "$out"; then
		echo "$name missing resident data" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "RUCODE FT01 FT00 FT00 RW00 RW01 IX0C IX00 IX10 NS01 NS00 NS01 KN01 KN00 PR01 PR03 PR0F FF03 IG00 RT01 AB02 FL03 QFF Y01 N00 XFF AF01 AF01 AF00 LD01 LD00 LD01" "$out"; then
		echo "$name missing critical error handler" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "TBATCH2 KW01 KW02 KW03 KW00 EQ01 EQ00 SY01 LV01 LV00 NV01 NC0101 NC0200 GL03 GL0B GL00 LB01 LB00 SH0002 NX0002 CA20464F4F" "$out"; then
		echo "$name missing batch conditionals" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "TCMD2B CT00 CT00 CT00 CT00 CT83 CP00 CP07 CP02 CP01 CP03 CP04 CP05 CP06 TN2E00 TN413A2E TN433A2E TN464F4F EX00 EX01 EL01 EL00" "$out"; then
		echo "$name missing console and code page" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "TCODE VF00 VF01 SX01 SX00 RD00 RD00 RD01 PP00 PP00 PP01 BY0200 BY0400 BYFE00 BY0000 CR00 CR01 PR00 PR01 PR00 PR00 PR00 AC01 AC02 AC03 AC04 AC00 CL01 CL02 CL02 CL03 CL03 CL00 DV00FF DV0002 DV0000 DV0000 NM03 NM07 TL01 TL00 SW00F0 NU01 NU02 NU03 NU00" "$out"; then
		echo "$name missing command dispatch" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "TENV2 CD00 CD01 CD00 CD01 CD06 CD02 CD03 CD07 MK01 MK00 MK02 MK04 MK02 MK02 MK03 RM01 RM02 RM00 RM02 RM02 RM03 PC00 PC01 PC01 PC00 FC444952 FC434F4D4D414E442E434F4D EQ01 EQ00" "$out"; then
		echo "$name missing directory commands" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "TFOR NS01 TM01 NS01 TM00 ER00 ER00 ER00 ER00 EM00 EM00 EM00 OK01 MN0003 MX0005 VR69 SGFF00 ER00 ER00 ER00 MM00 ER00 OK00 MN0003 MX0004 VR69 SG0000 OK00 MN0003 MX0004 VR6A SG0000 SK00 SK00 SK01 SK03 IX0004 IXFFFF IX0004 AC01 AC02 AC00 SB6563686F2041420D LN08 SB6563686F2025490D LN08 EC00 EC01 EC02 FTFFFF BC00 FTFF00 BC01 FT0000 BC00 FT0000 BC01" "$out"; then
		echo "$name missing FOR loops" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "TMISC2 SW8010 DR01 IN02 ST00 SW0000 DR00 IN00 ST01 RS424152 SW0000 DR00 IN00 ST01 RS42 SW0000 DR00 IN02 ST00 SW0000 DR01 IN02 ST00 SW0000 DR01 IN00 ST00 SW0000 DR00 IN00 ST02 RS424152 SW8014 DR00 IN00 ST00 SW0000 DR00 IN00 ST00 PG01 PG01 PG00 PG00 SL03 SL01 MV41420D CT0002 MV0D CT0000 IN00 OU00 IN00 OU00 IN00 OU00 IN41 OU00 IN02 OU00 IN01 OU00 IN00 OU01 IN00 OU02 IN00 OU03 IN00 OU04 IN00 OU05 IN00 OU41 IN00 OU06 IN00 OU09 IN00 OU08 IN00 OU09 IN00 OU41 TC0000 TC0041 TC0002" "$out"; then
		echo "$name missing path and redirection" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "TPARSE CX00 KY00 V200 QU00 LF00 EL0002 OR0000 RS0D ELFFFF OR0001 RS0D PC0000 OR0001 TK464F4F RS4241520D PC0003 OR0000 TK2F58 RS0D PC0004 OR0000 TK413D42 RS0D PC0000 OR0001 TK41 RS3D420D PC0000 OR0001 TK410A42 RS0D PC0000 OR0001 TK3132 RS2F330D PC0000 OR0001 TK41 RS420D PC0000 OR0001 TK22464F4F RS424152220D PC0002 OR0001 TK RS464F4F0D PC0000 OR0001 TK RS464F4F0D PC0000 OR0000 TK464F4F RS0D PC0001 OR0001 TK464F4F RS0D PC0000 OR0001 TK0A RS0D" "$out"; then
		echo "$name missing command parser" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "TPRINTF HD01 CLFF CT00 CP0F000010 SR00000010 ID00 EX00 ER00 HD02 CLFF CT00 CP11112222 SR00000000 ID00 EX00 ER00 HD01 CLFF CT00 CP0F000300 SR00000200 ID01 EX01 ER00 HD01 CLFF CT00 NS EX00 ER00 HD01 CLFF CT00 NS EX00 ER03 CD0005 HD02 CLFF CT00 NS EX00 ER01 HD01 CLFF CT00 NS EX00 ER02 SZ0B" "$out"; then
		echo "$name missing message printing" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "TRANMSG CLFF CT00 EX0000 NS00 SZ0B RS00 SG0000 ID00 FL10 MX80 MN00 PD20 M03EA00 M03EB00 M03EC00 M03EF00 M03F000 M03F100 M03F200 M03F300 M03F600 M03F700 M03F800 M03F900 M03FA0101A1090920 M03FB0101A1090920 M03FC0101B10A0A20 M03FD00 M03FE0101A1050120 M03FF0101A1050120 M04000101A1050120 M040100 M040200 M040300 M040400 M040500 M040600 M040700 M040802011003032002340A0A20 M040900 M040A010110080820 M040B00 M040C0101A50C0C20 M040D00 M040E00 M040F00 M04100201A10101200221020230 M0411010100800120 M04120201008001200210800120 M04130201A304043002A3040430 M041400 M041500 M041600 M041700 M0418010110800020 M041900 M041A00 M041B00 M041C00 M041D00 M041E00 M041F010110800120 M0420010110800120 M0421010110800120 M042300 M042400 M042500 M042600 M0427010110800020 M0428010110800020 M04290101B10A0A20 M042A010110800020 X00 M042B00 M042C00 M042D00 M042E00 M043000 M043100 M043200 M043302011003032002340A0820 M04340101A60B0B20 M04350201A40A08200285060620 M043600 PT504154483D PR50524F4D50543D CS434F4D535045433D" "$out"; then
		echo "$name missing message table" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "TSPC NX0057 CM0131 RE027D FL02E5 P102E8 P202E9 DS02EE AW02F0 DC02F3 F2034D DF0389 DB03CC XA0439 RM04DC DV04E1 SV0544 AR0666 FN0C30 PS0DF9 DT0E01 TM0E09 DO0E11 XL000D NM4350 FG8000 TY02 VL0002 AT0000 IL000E PG0019 LB000B FS0008 SB0016 ST0100 FB002B EN0E19" "$out"; then
		echo "$name missing transient layout" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "UINIT DV2F4445562F434F4E000000000000 CS2F434F4D4D414E442E434F4D0000 AB003A5C4155544F455845432E424154000D KB003A5C4B4155544F4558452E424154000D PRFF CM434F4D535045433D CH3D617A2F5C204341 EV00A0 EX8000 ER00 ER01 ER01 ER00 PM00 PX01 SW06 KW00 FI0201 CP0001 S2F5000000002 S2F4600000002 S2F4400000002 S2F4580000000 S2F4300000002 S2F4D534700000002 SG0000 BF00 CH00 RS00 XM00 EW00 DW00" "$out"; then
		echo "$name missing init data" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMP1 OR0100 MM0002 FC005C PA0080" "$out"; then
		echo "$name missing COMP entry" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMP2 LW00 HW00 EQ01 MM00 ST00 MK01 OK01 MM00 ST00 MK00 OK01 MM01 OF00000001 L58 R59 ST00 MK00 OK00 MM0A OF00000000 L00 R01 ST01 MK00 OK00 MM01 OF00000000 L1A R00 ST00 MK01 OK00" "$out"; then
		echo "$name missing file compare" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMPEQ EA02 SY8000 VR00 NU00 OP01 ZC00 FP01 TP02 YS01 KCC1 KIFF LN7F KN01 DO02 IO06 BK0A IS0B CK0C FO0F FC10 SF11 SN12 SD1A DD19 YN23 SV25 RB27 PF29 GV30 CD3B HO3D HC3E WR40 CU47 RT4C GC65 SO0001 SE0002 RO00 CC33 GS03 SS04 OF00 ON01 OB40 ZP00 NA00 OM82 FN0101 RD00 XO6C AL57 GL02 EE59 OK00 CB01 VB01 NS00 S101 S202 S303 C101 C202 CAFF CWFF00 DS000C SL000B PD20 CP01 MF00 EF01 IH01 SH00 AC02" "$out"; then
		echo "$name missing compare equates" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMPMS L0010000120 M000200010000FF00 M000300010000FF00 M000400010000FF00 M000500010000FF00 M000600010001FF00 F0133000120 M000700010001FF00 F0113000120 M000800010001FF00 F0113000120 M000900010000FF00 M000A00010000FF00 M000B00010001FF00 F0010000120 M000C00010001FF00 F0010000120 M000D00010001FF00 F0010000120 M000E00010000FF00 M000F00010000FF0A X M001000010000FF0A X M001100010000FF00 M001200010000FFC1 M001300010002FF00 F0110000120 F0210000120 M001400010000FF00 M001500010001FF00 F0010000120 M001600010000FF00 M001700010000FF00 M0000000200010200 F0010000120" "$out"; then
		echo "$name missing compare messages" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMPP NR00 DT00 TM00 FI01 CA01 CX00 DV01 QU00 NU00 KY00 SW01 V100 V200 V300 IN00 BS01 OK0000 ELFFFF MY0001 MS0002 NS0003 NK0004 RG0006 NV0007 ST0008 SY0009" "$out"; then
		echo "$name missing compare parser switches" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMPPAR MN00 MX02 SW00 KW00 FL0201 FN0001 TY05 TGFF NV00 AT0081 OC0000 PC0000 PL0040 P1464F4F P2424152 DN P1 P24E414D45 DN EX0009 TK4241442A LV01 ELFFFF" "$out"; then
		echo "$name missing compare parameters" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMPSM ID434F4D50 CR54686520444F5320434F4D50205574696C697479 VR01 NR01 IN01 NU01 TM00 DT00 CLA CL1 CL2 N01 CLFF H0002 RP0000 NI00 N01 CL00 H0002 RP0000 NI00 NF0000 SZ0010" "$out"; then
		echo "$name missing compare messages services" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DEBASM EM01 IX0003 NFFFFF SH01 SH00 W08 DL00 DX01 CL00 W10 DL01 DX01 CL00 W08 DL00 DX00 CL01 IM21 W10 DL01 DX00 CL01 IM21 ER01 MR0D MRF8" "$out"; then
		echo "$name missing line assembler" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DEBCOM1 HX00 HX09 HX0A HX0A HXFF HXFF N0000001A N0000FFFF NERR D0080 D0010 L0010 LERR L0008 E00000010 EERR E00010000 CH20 CH41 CH2E CH7E CH2E RW01 RW02 MV02030404 MV01010203 FL41424344 FLABCDABCDABCD SC0000 SC0003 SC LS414142 LS412242 LSER AD12340010 AD20000005 AD11110002 AD22220003 AD33330004 AD44440005 ADER" "$out"; then
		echo "$name missing debugger commands" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DEBCOM2 FG4E5620555020454920504C204E5A204E4120504F204E4320 STF202 OVFA02 DPFFFF CYF203 NCF202 RG00 RG0C RG0D RGFF UA41 UZ5A U77B U660 DL01 DL01 DL00 DL01 DL01 HX01 EX01 CM00" "$out"; then
		echo "$name missing registers and flags" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DEBCOM3 L03 L04 L05 L01 L02 L02 L02 LFF L02 L03 L04 LFF CT0001 CT0005 MK02 MK01 MK00" "$out"; then
		echo "$name missing trace lengths" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DEBCONST R8414C R84148 R64158 R64358 R64258 SR4553 SR4353 SZ4259 SZ4457 SH524F4C SH534152 IM414444 IM434D50 G154455354 G13F3F3F G2494E43 G23F3F3F NP90 RTC3 ITCC INEC XLD7 HLF4 AA37 CLF8 STFB MX00C1 SP005A IP0100 FLF202 WZ0F LN0080 NR08" "$out"; then
		echo "$name missing debugger tables" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DEBDATA RL0000 LB000A XM000E PG00190000 FR0419 OM0423 FN04280000 FZ04290000 PV04360000 DP0437 CR04730100 BG0475 BF047B AS048A BB0491 PT04E1 EN067B" "$out"; then
		echo "$name missing debugger data" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DEBEMS FN40 FN42 FN43 FN44 FN45 FN46 FN4B FN4D FR5800 LM40 HT00FF PE MP ST DA AL EL MP AL EL EL NI NI NI OK OK ER80 ER00 ER83 ER00 ER00 ERFF PG10 HD0007 PGFF HD0007 MP440200010ABC DA45000B UP000A UF0003 UA0007 UP0001 UF0002 UAFFFF SK HU0004 HT00FF SK SGE000 PH03 RH0001 RP0002 RH0003 RP0004 RC02 RC00" "$out"; then
		echo "$name missing expanded memory" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DEBEQU PR002D FC005C EX005C BL0050 BM000A BT0032 RT000E SD0000 BZ0200 NB5842 NP5042 NS4953 ND4944 CW2C00 OL0023 IJ0000 CCFFFF SV0000 MP0021 IM00FF CR000D LF000A TB0009 BS0008 EF001A RB007F EQ003D MN002D SP0020 DQ0022 SQ0027 CM002C PD002E CN003A SC003B LB005B AT0040 ZR0030 LA0061 LZ007A UA0041 UC0043 UE0045 UF0046 UL004C UM004D UN004E UP0050 US0053 UX0058 UZ005A VS0001 VB0003 VT0022 VK0023 VE0024 VR0025 VW0026 IO440D RW0008 RS0000 WS0000 DR0001 LS0000 LE0002 G40040 G20042 G30043 MM0044 G50045 G60046 G70047 G80048 GB004B GD004D GE004E GM0000 FR5800 GS0001 HL00FF LM0040 E00080 E30083 E40084 E50085 E60086 E70087 E80088 E90089 EA008A EB008B ED008D EE008E EF008F NI00FF" "$out"; then
		echo "$name missing debugger equates" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DEBERR JP00 WR40 RD3F RS0D L41 M02 L42 M01 L41 M04 L43 M03 L41 M01 L40 M02 TP00 RL00" "$out"; then
		echo "$name missing disk errors" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DEBMES SV00 FT002D N003B SM00004242 XR0067 SB002C SS0122 SX8310 A6 A7 F0008 CFF H0001 K0000 I00 B0000 NF0002 C01 H0002 CP002A K06 DR000E W01 P20 UA20 DA93 W04 DK00 WR23 DL41 EN6466626662726270 AB0050 OB0051 EZ03 PF00 PF01 LD00 ND00 LD01 DY01" "$out"; then
		echo "$name missing debugger messages" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DEBUASM LN0020 R0004 Z0000 R0001 Z0000 IX0240 W00 IX0004 W01 C20 E20 Z00 F0032 S0023 P000E P000C P0000 PFFFE D04 S03 C02 E01 N03 T09 N00 T09 HX3141 SG2B3035 SG2D3031 SG2D3830 SG2B3030 M4158 G00 P0000 M434C G00 P0000 M5B42582B53495D G03 P4444 M5B42585D G03 P1111 M5B42502B53495D G02 P5555 M5B53495D G03 P3333 M5B42582B53492D30325D G03 P4442 M5B53492B303041425D G03 P33DE M5B313233345D G03 P1234 FR NR NR FW46415220 S12C31 CL2C434C E574F52442050545220 E425954452050545220 E414444 E44495652 ERV" "$out"; then
		echo "$name missing unassembler" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DEBUG SV00 RP AS ER CM DU EN FL GO HX IN ER ER LD MV NM OU ZT QT RG SE TR UN ER WR EM ER ER ER ER ER QT RP P00 P00 Q01 F50 TV2522 EX4C EX00 GV3503 GV3501 SV2524 SV2523 CR26 CP0100 EN2168 DM1A0080 SP20FE PS1FFE DL0000 W07 R04 D0040 W0F R08 D0080 L01 L01 L00 L00 L01 PB412020420D PQ228140220D FL414222636422450D FL81610D NF NF NS03 FT00 FT01 FT0D FT02 TM4C TM00 TM01" "$out"; then
		echo "$name missing command letters" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMPINIT M909090 M909090 M909090 M000000 S01 T01 L4141 S02 T02 L4242 S01 T01 L4141 S01 T02 L4142 ST0005 STFFFF STFFFF ST0006 ST0005 STFFFF LT00 ST0006 LT42 STFFFF LT41 ST0005 ST0005 ST0005 STFFFF ST0005 ST0005 STFFFF ST0010 ST0010 STFFFF CT01 T01 CT01 T01 CT02 T02 BF2060 BEA000 CB2523 SL09" "$out"; then
		echo "$name missing compare init" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DCMPMACR AX4408 AX190E AX0007 TK0000 N003F F00010200 M00120200 L003F0200 PR0005" "$out"; then
		echo "$name missing compare macros" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DCOMPMS M0002FF0000 M0003FF0000 M0004FF0000 M0005FF0000 M0006FF0000 M0007FF0001 M0008FF0001 M0009FF0000 M000AFF0000 M000BFF0001 M000CFFC800 M000DFF0000 M000EFFC100 M000FFF0003 M0010FF0000 M0011FF0003 M0012FF0002 M0013FF0000 M0014FF0000 M0015FF0000 M0016FF0000 P10 C00 W21 DL413A00 ID40 SK00 TR0000 FS04 I05" "$out"; then
		echo "$name missing compare messages" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DCOMPPAR MN00 MX02 SW01 KW00 FL0101 FN0000 TY03 TGFF CP0081 OR0000 NS02 S00 T00 AFF EFF STFFFF EX00 MN0000 K00 S01 T02 AFF EFF STFFFF EX00 MN0000 K00 S00 T00 A01 EFF STFFFF EX00 MN0000 K00 S00 T00 AFF E01 STFFFF EX00 MN0000 K00 S00 T00 AFF EFF ST0004 EX01 MN0009 K01 S00 T00 AFF EFF ST0004 EX01 MN0003 K00" "$out"; then
		echo "$name missing compare parser" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DCOMPP I00 F00 D00 T00 L00 C00 X00 R01 Q00 N00 K00 S01 100 200 300 B01 R0000 RFFFF R0001 R0002 R0003 R0004 R0006 R0007 R0008 R0009" "$out"; then
		echo "$name missing parser switches" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DCOMPSM NM4449534B434F4D50 CLA CL1 CL2 LD DS CH NU IN M0001 CFF H0002 K0000 I00 M0001 C00 H0002 K0000 I00 NF01 CX0000" "$out"; then
		echo "$name missing compare messages services" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DISKCOMP EX00 EX01 EX02 XF4C FB20 BF1000 BE3FF0 LT4F ET0F SZ0200 PB000D ST01 NR ST01 RS ST02 RS ST00 RS SK CL440F GO SP GO AG SY RT CY SN03 EN0A SD00 FT SN01 EN08 SD00 FT SN00 EN0A SD00 OL BD SN27 EN08 SD00 SN27 EN08 SD01 FT SN03 EN0A SD00 FT FT SN03 EN0A SD01 BD FT SN00 EN08 SD00 SN00 EN0F SD01 FT TB1200 PQ20 TS0240 BG1240 OK FT FN02 FN01 PS01 PS02 SW02010305 CW0020 CD0010 CP KP KP CP AT0027 LN04 IO01 L41 IO01 IO02 L43 MR11 MC12 BPF9FDFCFC IX0000 IX0008 GN440D080060 ST0200 EN0100 EQ0056 SM00058634 FNFFFF RM1000 CS00E0 DC4408 IVFFF7" "$out"; then
		echo "$name missing diskette compare" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COPYINIT SL0C S01 T01 L4141 S02 T02 L4242 S01 T01 L4141 S01 T02 L4142 ST0005 STFFFF STFFFF ST0006 ST0005 STFFFF LT00 ST0006 LT42 STFFFF LT41 ST0005 ST0005 ST0005 STFFFF ST0005 ST0005 STFFFF ST0010 ST0010 STFFFF CT01 T01 CT01 T01 CT02 T02 BF2060 BEA000 CB2523" "$out"; then
		echo "$name missing copy init" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DCOPYMS M0002FF0000 M0003FF0000 M0004FF0000 M0005FF0000 M0006FF0000 M0007FF0000 M0008FF0001 M0009FF0001 M000AFF0000 M000BFF0000 M000CFF0000 M000DFF0001 M000EFF0000 M000FFFC800 M0010FFC100 M0011FF0003 M0012FF0000 M0013FF0003 M0014FF0003 M0015FF0000 M0016FF0000 M0017FF0000 M0019FF0000 M001AFF0002 A18 P10 C00 W21 RA3 Z0430 DL413A00 ID40 FS04" "$out"; then
		echo "$name missing copy messages" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DCOPYPAR MN00 MX02 SW01 KW00 FL0101 FN0002 TY06 TGFF SW0001 FN0002 NS01 CP0081 OR0000 S00 T00 AFF STFFFF EX00 MN0000 K00 S01 T02 AFF STFFFF EX00 MN0000 K00 S00 T00 A01 STFFFF EX00 MN0000 K00 S00 T00 AFF ST0004 EX01 MN0009 K01 S00 T00 AFF ST0004 EX01 MN0003 K00" "$out"; then
		echo "$name missing copy parser" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DCOPYP F00 D00 T00 L00 C01 X00 R01 Q00 N00 K00 S01 100 200 300 I00 B01" "$out"; then
		echo "$name missing copy parser switches" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DCOPYSM NM4449534B434F5059 CL1 CL2 CLA LD DS CH NU IN M0001 CFF H0002 K0000 I00 M0001 C00 H0002 K0000 I00 NF01 CX0000" "$out"; then
		echo "$name missing copy message services" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DCPYMACR AX4408 AX190E AX0007 TK0000 N003F F00010200 M00120200 L003F0200 PR0005" "$out"; then
		echo "$name missing copy macros" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DISKCOPY EX00 EX01 EX02 EX01 XF4C FB20 BF1000 BE3FF0 LT4F ET0F SZ0200 SB0200 PB000D CT01 ST01 NR ST01 RS ST02 RS ST00 RS SK CL440F S04 TSK T1 T2 UN00 FT01 VL00 UN00 FT01 VL00 UN00 FT00 VL01 UN01 FT00 VL01 GO SP GO AG SY RT CY NF FL FL FN02 FN01 NF FL FL WN02 WN00 WN01 PS01 PS02 PS01 FS0000 AD OK RT HR HS LW0406 HW11CB KP KP LW0406 HW11CB OK RT GU FF RW NC0050 BF05 RF00 FC FC DN04 TB FC TB DN01 DN00 RT HD OK SN03 EN0A SD00 FT SN01 EN09 SD00 SN00 EN0A SD00 FT OL BD SN27 EN08 SD00 SN27 EN09 SD00 SN4F EN0F SD01 FT SN27 EN08 SD01 IX0000 IX0008 RT CY NC TB1200 PQ20 TS0240 BG1240 OK FT F90F FD09 FC09 FC08 LN000D N000F F00010200 L000F0200 OK FM FT FM FM OL FM FM FT FM OK FT OK OK FT CY CY NC NC NC M01 M01 M00 GN440D08 IO01 L41 TO TO IO01 L43 IO01 L41 KP IO02 UK00 IO01 NL IO02 UK01 IO02 UK00 RD S01 T04 L41 WR S00 T07 L42 MXADC1 SP CL SA CL SA ST YN6523 SN00 CY AN01 AT0000 AT0002 SK VH11CB VL0406 PA EQ0068 SM0006DFE9 FNFFFF RM1000 CS00E0 DC4408 IVFFF7 BF0005 MXADC1 CA00FF" "$out"; then
		echo "$name missing diskcopy" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "EDLCMD1 RD3F WR40 SK42 CL3E EX4C IN0A CE NM FL LD LD CE ME AV0050 CT000A EF00 CT0004 EF01 CT0005 EF00 EF01 EF01 EF00 AT0100 N0005 LV0000 AT0100 N0000 LV0000 AT0180 N0001 LV0080 KP AD02 BK AL01 CXFFFF NS RL0004 CL NM SP ME CE OK OK PT003C EM EM EM PT0040 EM DB KB OK CL ALFF CL ALFF MD01 CE MD00 LN0016 TL LN00FE RS0007 RS0000 CP0003 RM0007 CP0000 RM000A AC SK MD01 CE MD00 FN AC CU0007 NF CE A0004 B0004 A0002 B0002 A0003 B0008 NF MV0015 CE CE ST0001 ST0006 ST0009 QT PG0018 PG0015 PG000A QT CE CE ST0002 ST0001 ST0001 ST0004 NF PG0010 PG0019 PG0019 CE LN0005 LN0007 NF EN EM RP" "$out"; then
		echo "$name missing line editor commands" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "GUEST HELLO COM" "$out"; then
		echo "$name missing guest hello" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "HELLO EXE" "$out"; then
		echo "$name missing guest exe" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "FILE Z EXEC 7" "$out"; then
		echo "$name missing guest file exec" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "CMD VER 4.00" "$out" \
		|| ! grep -q "CMD ECHO hello" "$out" \
		|| ! grep -q "CMD DIR HELLO.COM" "$out" \
		|| ! grep -q "CMD TYPE Z" "$out" \
		|| ! grep -q "CMD COPY OK" "$out" \
		|| ! grep -q "CMD DEL OK" "$out" \
		|| ! grep -q "CMD REN OK" "$out" \
		|| ! grep -q "CMD MD OK" "$out" \
		|| ! grep -q "CMD CD OK" "$out" \
		|| ! grep -q "CMD RD OK" "$out" \
		|| ! grep -q "CMD SET OK" "$out" \
		|| ! grep -q "CMD ECHO BATCH" "$out" \
		|| ! grep -q "CMD ECHO FROM BATCH" "$out" \
		|| ! grep -q "EXT COM" "$out" \
		|| ! grep -q "EXT EXE" "$out" \
		|| ! grep -q "CMD ERRORLEVEL 3" "$out" \
		|| ! grep -q "CMD DIR SELECT.DAT" "$out" \
		|| ! grep -q $'CMD TYPE \xff\x05A' "$out" \
		|| ! grep -q "EXEFIX 1234 0010" "$out" \
		|| ! grep -q "PACKFIX 0ABC 0020" "$out" \
		|| ! grep -q "PARSE 2 HELLO    COM 0" "$out" \
		|| ! grep -q "WILD 1 ???????? ???" "$out" \
		|| ! grep -q "CTRL 0 1 BOOT 1" "$out" \
		|| ! grep -q "EXT 0002 08 03 02" "$out" \
		|| ! grep -q "PSP 0B00" "$out" \
		|| ! grep -q "DUP2 OK" "$out" \
		|| ! grep -q "FN87 0001 1" "$out" \
		|| ! grep -q "CMD DIR FIX2.EXE" "$out" \
		|| ! grep -q "CMD DIR PACKED.EXE" "$out" \
		|| ! grep -q "PACKED 2A00 4D5A OK" "$out" \
		|| grep -q "CMD ERRORLEVEL /" "$out" \
		|| ! grep -q "Incorrect number of parms  given to BUILDIDX." "$out" \
		|| ! grep -q "BuildMsg - PSP at 0b00" "$out" \
		|| ! grep -q "Allocatable memory size is 606208" "$out" \
		|| ! grep -q "CMD ERRORLEVEL 1" "$out" \
		|| ! grep -q "Couldn't open  menu creation files." "$out" \
		|| ! grep -q "At lest one filespec ( for .SKL ) needed." "$out" \
		|| ! grep -qF "usage: cl [ option... ] filename... [ -link linkoption... ]" "$out" \
		|| ! grep -q "Microsoft (R) Library Manager  Version 3.10" "$out" \
		|| ! grep -q "CMD DIR LIB.LST" "$out" \
		|| ! grep -q "LIB LST __aFFaldiv" "$out" \
		|| ! grep -q "The number topic(s) and text string(s) written = 57" "$out" \
		|| ! grep -q "Microsoft (R) Overlay Linker  Version 3.65" "$out" \
		|| ! grep -q "LINKED OK" "$out" \
		|| ! grep -q "Copyright (C) Microsoft Corp 1983-1988.  All rights reserved." "$out" \
		|| ! grep -qF 'A:\HELLO.COM' "$out" \
		|| ! grep -qF 'NEXT CHILD.COM' "$out" \
		|| ! grep -qF 'IOCTL 0000' "$out" \
		|| ! grep -qF 'SAR C0' "$out" \
		|| ! grep -qF 'JP Y' "$out" \
		|| ! grep -qF 'OF Y' "$out" \
		|| ! grep -qF 'WP 0' "$out" \
		|| ! grep -qF 'MUL 1' "$out" \
		|| ! grep -qF 'DA 20' "$out" \
		|| ! grep -qF 'SH 0010' "$out" \
		|| ! grep -qF 'IM 000F' "$out" \
		|| ! grep -qF 'RL 10' "$out" \
		|| ! grep -qF 'AM 0105' "$out" \
		|| ! grep -qF 'AD 000F' "$out" \
		|| ! grep -qF 'AA 0100' "$out" \
		|| ! grep -qF 'AS 0009' "$out" \
		|| ! grep -qF 'DS 19' "$out" \
		|| ! grep -qF 'CMC 0' "$out" \
		|| ! grep -qF 'S6 02' "$out" \
		|| ! grep -qF 'LD Q' "$out" \
		|| ! grep -qF 'CW Y' "$out" \
		|| ! grep -qF 'RS Q' "$out" \
		|| ! grep -qF 'EN 1FFE1FF8' "$out" \
		|| ! grep -qF 'IT Y' "$out" \
		|| ! grep -qF 'BD Y' "$out" \
		|| ! grep -qF 'LK 0006' "$out" \
		|| grep -qF 'A:\\HELLO.COM' "$out" \
		|| ! grep -q "EXE2BIN 0018 0E1F OK" "$out" \
		|| ! grep -q "Microsoft (R) Program Maintenance Utility   Version 1.00.05" "$out" \
		|| ! grep -q "nmake-wrote" "$out" \
		|| grep -q "U4013" "$out" \
		|| ! grep -q "Microsoft (R) Macro Assembler Version 5.10" "$out" \
		|| ! grep -q "MASM OK" "$out" \
		|| [ "$(grep -c "L4021: no stack segment" "$out")" -ne 1 ] \
		|| ! grep -q "c1ok" "$out" \
		|| ! grep -q "C2 OK" "$out" \
		|| ! grep -q "IBM Library Manager/2 Version 1.01" "$out" \
		|| ! grep -q "CMD DIR LIB2.LST" "$out" \
		|| ! grep -q "06EH,06DH,061H,06BH,065H" "$out" \
		|| ! grep -q "Message file updated." "$out" \
		|| ! grep -q "Index file updated." "$out" \
		|| ! grep -q "Message files for MORE.SKL completed." "$out" \
		|| ! grep -q "CMD DIR MORE.CLA" "$out" \
		|| ! grep -q "; MORE.clA" "$out" \
		|| ! grep -q "largeok" "$out" \
		|| ! grep -q "L1 OK" "$out" \
		|| ! grep -q "Microsoft (R) C Optimizing Compiler Version 5.10" "$out" \
		|| ! grep -q "CMD DIR LARGE.OBJ" "$out" \
		|| ! grep -q "_largeok" "$out" \
		|| grep -q "D4005" "$out" \
		|| ! grep -q "All four files have been opened for menu creation." "$out" \
		|| ! grep -q "CMD DIR MENUM.C" "$out" \
		|| ! grep -q "menu-ok" "$out" \
		|| ! grep -q "Created include file \[BMSG.CLA\]" "$out" \
		|| ! grep -q "CMD DIR BMSG.CTL" "$out" \
		|| ! grep -q "\$M_NUM_CLS  EQU 1" "$out" \
		|| ! grep -q "conv.exe => conv.com \[OK\]" "$out" \
		|| ! grep -q "CMD DIR CONV.COM" "$out" \
		|| ! grep -q "Converted" "$out" \
		|| ! grep -q "exec: EXITCODE.COM returned 3" "$out"; then
		echo "$name missing command built-ins" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "OP LEA 10" "$out" \
		|| ! grep -q "OP XOR 00" "$out" \
		|| ! grep -q "OP ALU 11" "$out" \
		|| ! grep -q "OP CMP OK" "$out" \
		|| ! grep -q "OP MUL 06" "$out" \
		|| ! grep -q "OP IMUL 0F" "$out" \
		|| ! grep -q "OP DIR 2F" "$out" \
		|| ! grep -q "OP STR Z" "$out" \
		|| ! grep -q "OP REP Z" "$out" \
		|| ! grep -q "OP LOOP 03" "$out" \
		|| ! grep -q "OP JCXZ OK" "$out" \
		|| ! grep -q "OP JMP OK" "$out" \
		|| ! grep -q "OP ENTER OK" "$out" \
		|| ! grep -q "OP LEAVE OK" "$out" \
		|| ! grep -q "OP SLDT 00" "$out" \
		|| ! grep -q "OP XCHG 02" "$out" \
		|| ! grep -q "OP CBW FF" "$out" \
		|| ! grep -q "OP CWD FF" "$out" \
		|| ! grep -q "OP SHR 04" "$out" \
		|| ! grep -q "OP INS Z" "$out" \
		|| ! grep -q "OP OUTS 21" "$out" \
		|| ! grep -q "OP STC OK" "$out" \
		|| ! grep -q "OP CLC OK" "$out"; then
		echo "$name missing guest opcodes" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "INT DRV 00" "$out" \
		|| ! grep -q "INT DATE 1980" "$out" \
		|| ! grep -q "INT TIME 00" "$out" \
		|| ! grep -q "INT FREE 0200" "$out" \
		|| ! grep -q "INT VEC 1234" "$out" \
		|| ! grep -q "INT ATTR 20" "$out" \
		|| ! grep -q "INT SEEK 01" "$out" \
		|| ! grep -q "INT MAKE OK" "$out" \
		|| ! grep -q "INT REN OK" "$out" \
		|| ! grep -q "INT DEL OK" "$out" \
		|| ! grep -q $'INT FIND HELLO.COM\r\nINT CWD ROOT' "$out" \
		|| ! grep -q "INT CWD ROOT" "$out" \
		|| ! grep -q "INT MEM OK" "$out" \
		|| ! grep -q "INT DIR OK" "$out"; then
		echo "$name missing guest dos services" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "BIOS 10 50" "$out" \
		|| ! grep -q "BIOS 13 55" "$out" \
		|| ! grep -q "BIOS 16 00" "$out" \
		|| ! grep -q "BIOS 1A 0001" "$out"; then
		echo "$name missing guest bios services" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "COMPRESS FF0541" "$out"; then
		echo "$name missing compress" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "EXE2BIN File not found" "$out"; then
		echo "$name missing exe2bin" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if grep -q "exec: can't run HELLO.COM" "$out"; then
		echo "$name exec failed" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! awk '
		/EXE2BIN File not found/ { seen = 1 }
		seen && /GUEST HELLO COM/ { hit = 1 }
		END { exit hit ? 0 : 1 }
	' "$out"; then
		echo "$name missing exec" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "DBOF Syntax: DBOF Source-file Target-file Hex-start Hex-length" "$out"; then
		echo "$name missing dbof" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if [ "$(grep -c "usage: exefix <file> <max-alloc> <min-alloc>" "$out")" -ne 2 ]; then
		echo "$name missing exefix" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "Usage: Convert \[file\]" "$out"; then
		echo "$name missing convert" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q '  A       A:\\HELLO.COM' "$out"; then
		echo "$name missing attrib" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q ".EXE size (bytes)                     3D              61" "$out"; then
		echo "$name missing exemod" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "ALLOC 0001 0006 FIT0001 ER08 BX0006" "$out"; then
		echo "$name missing ALLOC" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "ERR 000F CL08 AC03 LC02" "$out"; then
		echo "$name missing extended error" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "A>" "$out"; then
		echo "$name missing command prompt" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if ! grep -q "CMDOK" "$out"; then
		echo "$name missing COMMAND.COM load" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if grep -q "Bad or missing Command Interpreter" "$out"; then
		echo "$name printed the missing interpreter after a successful load" >&2
		cat "$out" >&2
		rm -f "$out"
		exit 1
	fi
	if [[ -n "${PROOF_DIR:-}" ]]; then
		mkdir -p "$PROOF_DIR"
		cp "$out" "$PROOF_DIR/$name.txt"
		printf '%s\n' "$status" >"$PROOF_DIR/$name.qemu-exit"
	fi
	cat "$out"
	rm -f "$out"
}

if [[ "${1:-}" == "doctor" ]]; then
	doctor
	exit 0
fi

doctor
run_one i686 qemu-system-i386 "$root/target/i686-unknown-none/release/dos4-rs"
run_one x86_64 qemu-system-x86_64 "$root/target/x86_64-unknown-none/release/dos4-rs"
exit 0

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

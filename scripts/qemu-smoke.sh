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

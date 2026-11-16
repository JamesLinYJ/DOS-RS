"""Exercise native SETUP from the emulated keyboard on disposable RAM disks."""

import argparse
import contextlib
import hashlib
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import time


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


class Session:
    def __init__(self, proc, log, proof):
        self.proc, self.log, self.proof = proc, log, proof
        self.mark = 0

    def monitor(self, line):
        self.proc.stdin.write((line + "\n").encode())
        self.proc.stdin.flush()

    def expect(self, text, timeout=120):
        needle = text.encode("gb18030")
        end = time.monotonic() + timeout
        while time.monotonic() < end and self.proc.poll() is None:
            data = self.log.read_bytes() if self.log.exists() else b""
            at = data.find(needle, self.mark)
            if at >= 0:
                self.mark = at + len(needle)
                return
            time.sleep(0.08)
        raise RuntimeError(
            f"{self.log}: missing {text!r}: {self.log.read_bytes()[-650:]!r}"
        )

    def key(self, k):
        self.monitor("sendkey " + k + " 20")
        time.sleep(0.07)

    def type(self, text):
        keys = {
            " ": "spc",
            "\n": "ret",
            ":": "shift-semicolon",
            ".": "dot",
            "/": "slash",
        }
        for c in text:
            self.key(keys.get(c, c))

    def picture(self, name):
        self.monitor("screendump " + str(self.proof / (name + ".ppm")))
        time.sleep(0.4)

    def wizard(self, command, disk, option=None, locale="2"):
        self.type(command + "\n")
        self.expect("1-8 >")
        if locale == "7":
            self.picture("setup-zh-language")
        self.key(locale)
        prompts = {
            "2": ("Disk >", "Action >", "Confirm >"),
            "7": ("磁盘 >", "操作 >", "确认 >"),
        }
        a, b, c = prompts[locale]
        self.expect(a)
        if locale == "7":
            self.picture("setup-zh-drive")
        self.key(str(disk))
        if option is not None:
            self.expect(b)
            if locale == "7":
                self.picture("setup-zh-actions")
            self.key(str(option))
            if command == "setup" and option == 1:
                # Fresh installs have an options page; maintenance goes straight to review.
                end = time.monotonic() + 90
                while time.monotonic() < end:
                    data = self.log.read_bytes()[self.mark :]
                    if b"Config > " in data:
                        self.expect("Config > ")
                        if locale == "7":
                            self.key("2")
                            self.expect("Config > ")
                            self.key("3")
                            self.expect("Config > ")
                            self.key("4")
                            self.expect("Config > ")
                            self.picture("setup-zh-config")
                        self.key("ret")
                        break
                    if c.encode("gb18030") in data:
                        break
                    time.sleep(0.08)
            self.expect(c)


@contextlib.contextmanager
def vm(boot, drives, proof, tag, arch="i386"):
    log = proof / (tag + ".txt")
    log.unlink(missing_ok=True)
    args = [
        "qemu-system-" + arch,
        "-m",
        "16",
        "-display",
        "none",
        "-nic",
        "none",
        "-vga",
        "std",
        "-monitor",
        "stdio",
        "-no-reboot",
        "-serial",
        "file:" + str(log),
        "-boot",
        "a" if boot else "c",
    ]
    if boot:
        args += [
            "-drive",
            "if=floppy,id=setupfd,format=raw,readonly=on,file=" + str(boot),
        ]
    for i, disk in enumerate(drives):
        args += ["-drive", f"if=ide,index={i},format=raw,file={disk}"]
    p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL)
    s = Session(p, log, proof)
    try:
        s.expect("A:\\> " if boot else "C:\\> ")
        yield s
    finally:
        if p.poll() is None:
            s.monitor("quit")
            p.wait(10)
        p.stdin.close()


def put(disk, at, data):
    with disk.open("r+b") as f:
        f.seek(at)
        f.write(data)


def disk_file(d, name, mib):
    path = d / name
    with path.open("xb") as f:
        f.truncate(mib << 20)
    return path


def test_disk_set(directory, disks, proof):
    first = disks.resolve() / "DISK1.IMG"
    second = disks.resolve() / "DISK2.IMG"
    for name in ("EDIT.EXE", "WELCOME.TXT"):
        result = subprocess.run(
            ["mdir", "-i", first, "::" + name],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        assert result.returncode != 0
    target = disk_file(directory, "swap-target.img", 16)
    blank = digest(target)

    def begin(s):
        s.type("setup\n")
        s.expect("1-8 >")
        s.key("2")
        s.expect("Disk >")
        s.key("1")
        s.expect("Action >")
        s.key("1")
        s.expect("Insert disk 2 >")

    def change(s, path):
        quoted = str(path).replace(chr(92), chr(92) * 2).replace('"', chr(92) + '"')
        s.monitor(f'change setupfd "{quoted}" raw read-only')

    with vm(first, [target], proof, "disk-swap-cancel") as s:
        begin(s)
        s.key("ret")
        s.expect("Wrong disk 2 >")
        s.key("esc")
        s.expect("DOS >")
        s.key("ret")
        s.expect("A:\\> ")
    assert digest(target) == blank
    with vm(first, [target], proof, "disk-swap-install") as s:
        begin(s)
        s.picture("disk-swap")
        change(s, second)
        s.key("ret")
        s.expect("Restore disk 1 >")
        s.key("ret")
        s.expect("Wrong disk 1 >")
        change(s, first)
        s.key("ret")
        s.expect("Config >")
        s.key("ret")
        s.expect("Confirm >")
        s.key("ret")
        s.expect("Installation complete")
        s.expect("DOS >")
        s.key("ret")
        s.expect("A:\\> ")
    for name in ("EDIT.EXE", "WELCOME.TXT"):
        packaged = subprocess.check_output(["mcopy", "-i", second, "::" + name, "-"])
        installed = subprocess.check_output(
            ["mcopy", "-i", str(target) + "@@1048576", "::" + name, "-"]
        )
        assert packaged == installed, name
    with vm(None, [target], proof, "disk-swap-cold-boot") as s:
        s.type("edit swap.txt\n")
        s.expect("Ctrl+S Save")
        s.type("two disks")
        s.key("ctrl-s")
        s.expect("Saved")
        s.key("ctrl-q")
        s.expect("C:\\> ")
    assert (
        subprocess.check_output(
            ["mcopy", "-i", str(target) + "@@1048576", "::SWAP.TXT", "-"]
        )
        == b"two disks"
    )
    target.unlink()
    print(
        "PASS install with one floppy drive, reject wrong media, cancel, swap back, cold boot and Edit",
        flush=True,
    )


def main():
    c = argparse.ArgumentParser(description=__doc__)
    for flag in ("ram-dir", "image", "proof"):
        c.add_argument("--" + flag, type=Path, required=True)
    c.add_argument("--fsck", default="fsck.fat")
    c.add_argument("--disk-set", type=Path, help="also install the two-floppy edition")
    a = c.parse_args()
    proof = a.proof.resolve()
    proof.mkdir(parents=True, exist_ok=True)
    boot = a.image.resolve()
    with tempfile.TemporaryDirectory(dir=a.ram_dir) as tmp:
        d = Path(tmp)
        if a.disk_set:
            test_disk_set(d, a.disk_set, proof)
        first = disk_file(d, "first.img", 8)
        target = disk_file(d, "second.img", 24)
        put(first, 93451, b"FIRST DISK MUST SURVIVE")
        before = digest(first)
        blank = digest(target)
        with vm(boot, [first, target], proof, "cancel") as s:
            s.wizard("setup", 2, 1)
            s.key("esc")
            s.expect("Config >")
            s.key("esc")
            s.expect("Action >")
            s.key("esc")
            s.expect("Disk >")
            s.key("esc")
            s.expect("1-8 >")
            s.key("esc")
            s.expect("Cancelled.")
            s.expect("A:\\> ")
        assert digest(target) == blank and digest(first) == before
        for mode, flag in [("cga", " /cga"), ("ega", " /ega"), ("mono", " /mono")]:
            with vm(boot, [first, target], proof, "display-" + mode) as s:
                s.type("setup" + flag + "\n")
                s.expect("1-8 >")
                s.key("right")
                s.expect("1-8 >")
                s.picture("setup-" + mode)
                s.key("ret")
                s.expect("Disk > " if mode == "mono" else "磁碟 > ")
                s.key("esc")
                s.expect("1-8 >")
                s.key("esc")
                s.expect("A:\\> ")
        assert digest(target) == blank and digest(first) == before
        with vm(boot, [first, target], proof, "second-disk", "x86_64") as s:
            s.wizard("setup", 2, 1, "7")
            s.picture("setup-zh-review")
            s.key("ret")
            s.expect("GLYPHS.FNT")
            s.expect("10%")
            s.picture("setup-zh-copy")
            s.expect("安装完成")
            s.expect("DOS > ")
            s.picture("setup-zh-finished")
            s.key("ret")
            s.expect("A:\\> ")
        assert digest(first) == before and digest(target) != blank
        volume = str(target) + "@@1048576"

        def copy(source, destination):
            subprocess.run(
                ["mcopy", "-o", "-i", volume, str(source), str(destination)], check=True
            )

        user = d / "user.txt"
        user.write_bytes(b"keep personal files\r\n")
        copy(user, "::USER.TXT")
        config = d / "installed-config"
        copy("::CONFIG.SYS", config)
        assert (
            config.read_bytes()
            == b"LANG=zh-Hans\r\nDISPLAY=AUTO\r\nSMARTDRV=OFF\r\nCDROM=OFF\r\nDEVICE=EMM386.SYS\r\n"
        )
        auto = d / "installed-autoexec"
        copy("::AUTOEXEC.BAT", auto)
        assert b"REM Minimal" in auto.read_bytes()
        large = d / "large.txt"
        large.write_bytes(b"line of text\r\n" * 3200)
        copy(large, "::LARGE.TXT")
        original = d / "original"
        copy("::WELCOME.TXT", original)
        corrupt = d / "bad"
        corrupt.write_bytes(b"X" + original.read_bytes()[1:])
        copy(corrupt, "::WELCOME.TXT")
        snapshot = digest(target)
        with vm(boot, [first, target], proof, "verify") as s:
            s.wizard("setup", 2, 1)
            s.key("ret")
            s.expect("Files differ.")
            s.expect("DOS > ")
            s.key("ret")
            s.expect("A:\\> ")
        assert digest(target) == snapshot
        with vm(boot, [first, target], proof, "repair") as s:
            s.wizard("setup", 2, 2)
            s.key("ret")
            s.expect("Repair complete.")
            s.expect("DOS > ")
            s.key("ret")
            s.expect("A:\\> ")
        for name, expected in [("WELCOME.TXT", original), ("USER.TXT", user)]:
            got = d / "readback"
            copy("::" + name, got)
            assert got.read_bytes() == expected.read_bytes()
        with vm(None, [target], proof, "native-cold-boot", "x86_64") as s:
            s.type("smartdrv /status\n")
            s.expect("sectors 0")
            s.expect("C:\\> ")
            s.type("type user.txt\n")
            s.expect("keep personal files")
            s.expect("C:\\> ")
            s.type("echo ")
            s.key("ctrl-p")
            s.expect("Pinyin >")
            s.type("zhong\n")
            s.expect("1中")
            s.key("1")
            s.key("ctrl-u")
            s.expect("U+")
            s.type("6587\n")
            s.expect("echo 中文")
            s.key("ret")
            s.expect("C:\\> ")
            s.picture("input-zh")
            s.type("edit note.txt\n")
            s.expect("Ctrl+S 保存")
            s.type("hello")
            s.key("ctrl-s")
            s.expect("已保存")
            s.picture("edit-dos")
            s.key("ctrl-q")
            s.expect("C:\\> ")
            s.type("edit note.txt\n")
            s.expect("hello")
            s.key("end")
            s.type(" dos")
            s.key("ctrl-s")
            s.expect("已保存")
            s.key("ctrl-q")
            s.expect("C:\\> ")
            s.type("type note.txt\n")
            s.expect("hello dos")
            s.expect("C:\\> ")
            s.type("edit large.txt\n")
            s.expect("Ctrl+S 保存")
            s.type("x")
            s.key("ctrl-z")
            s.key("ctrl-y")
            s.key("ctrl-s")
            s.expect("已保存")
            s.key("ctrl-q")
            s.expect("C:\\> ")
            s.type("edit\n")
            s.expect("未命名")
            s.key("ctrl-u")
            s.expect("U+")
            s.type("4e2d\n")
            s.key("ctrl-p")
            s.expect("拼音 >")
            s.type("wen\n")
            s.expect("1文")
            s.key("1")
            s.key("f2")
            s.expect("另存为: ")
            s.type("han.txt\n")
            s.expect("已保存")
            s.picture("edit-zh")
            # Switch languages with an open document, then find its CJK text.
            labels = [
                ("1", "Tastaturhilfe"),
                ("2", "Keyboard help"),
                ("3", "Ayuda del teclado"),
                ("4", "Aide du clavier"),
                ("5", "キー操作"),
                ("6", "키보드 도움말"),
                ("8", "快速鍵說明"),
                ("7", "快捷键帮助"),
            ]
            for key, help_title in labels:
                s.key("f4")
                s.expect("1  Deutsch")
                s.key(key)
                s.key("f1")
                s.expect(help_title)
                if key in ("4", "5", "6", "8"):
                    s.picture("edit-help-" + key)
                s.key("esc")
            s.key("ctrl-f")
            s.expect("查找: ")
            s.key("ctrl-u")
            s.expect("U+")
            s.type("4e2d\n")
            s.key("ret")
            s.expect("已找到")
            s.key("ctrl-q")
            s.expect("C:\\> ")
        copy("::LARGE.TXT", d / "large-check")
        assert (d / "large-check").read_bytes() == b"x" + large.read_bytes()
        copy("::HAN.TXT", d / "han-check")
        assert (d / "han-check").read_bytes() == "中文".encode("gb18030")
        copy("::NOTE.TXT", d / "note-check")
        assert (d / "note-check").read_bytes() == b"hello dos"
        with (d / "fat.img").open("wb") as out, target.open("rb") as src:
            src.seek(2048 * 512)
            shutil.copyfileobj(src, out)
        subprocess.run([a.fsck, "-n", d / "fat.img"], check=True)
        (d / "fat.img").unlink()
        assert digest(first) == before
        # A zero MBR with old payload inside the range must survive the entire wizard.
        with vm(boot, [first], proof, "interior-data") as s:
            s.wizard("setup", 1, 1)
            s.key("ret")
            s.expect("Data found or read failure.")
        assert digest(first) == before
        table = bytearray(512)
        table[:6] = b"BOOTOK"
        table[510:] = b"\x55\xaa"
        table[446] = 128
        table[450] = 0x83
        struct.pack_into("<II", table, 454, 2048, 4096)
        shared = disk_file(d, "shared.img", 16)
        put(shared, 0, table)
        put(shared, 2048 * 512, b"Existing partition payload")
        with vm(boot, [shared], proof, "shared") as s:
            s.wizard("setup", 1, 1)
            s.key("ret")
            s.expect("Verifying files...")
            s.expect("Existing boot selection retained.")
            s.key("ret")
            s.expect("A:\\> ")
        with shared.open("rb") as f:
            actual = f.read(512)
            assert actual[:462] == table[:462] and actual[478:] == table[478:]
            assert actual[462] == 0 and struct.unpack_from("<I", actual, 470)[0] == 6144
            f.seek(2048 * 512)
            assert f.read(26) == b"Existing partition payload"
        for tag, kind, reason in [
            ("gpt", 238, "GPT disk."),
            ("extended", 15, "Extended partitions"),
        ]:
            table[450] = kind
            put(shared, 0, table)
            snapshot = digest(shared)
            with vm(boot, [shared], proof, tag) as s:
                s.wizard("setup", 1)
                s.expect(reason)
            assert digest(shared) == snapshot
        bad_boot = d / "badboot.img"
        shutil.copyfile(boot, bad_boot)
        subprocess.run(
            ["mcopy", "-o", "-i", bad_boot, corrupt, "::WELCOME.TXT"], check=True
        )
        with vm(bad_boot, [target], proof, "bad-payload") as s:
            snapshot = digest(target)
            s.wizard("setup", 1)
            s.expect("Action >")
            s.key("1")
            s.expect("Source file missing or damaged.")
        assert digest(target) == snapshot
        print(
            "PASS cancel, second disk FAT16, cold boot, Chinese input, verification, repair, shared MBR, occupied range, GPT, extended and damaged media"
        )


if __name__ == "__main__":
    main()

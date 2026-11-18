"""Serial assertions and monitor keystrokes for interactive QEMU regressions."""
import os
from pathlib import Path
import subprocess
import time

KEYS = {" ": "spc", ".": "dot", "-": "minus", "=": "equal", ":": "shift-semicolon",
        "$": "shift-4", "?": "shift-slash", "|": "shift-backslash",
        ">": "shift-dot", "<": "shift-comma", "/": "slash", '"': "shift-apostrophe",
        "'": "apostrophe", ";": "semicolon"}

class Shell:
    def __init__(self, cmd, log: Path, work: Path):
        self.log = log
        log.write_bytes(b"")
        self.vm = subprocess.Popen(cmd + ["-display", "none", "-serial", "file:" + str(log),
                                   "-monitor", "stdio", "-nic", "none"], stdin=subprocess.PIPE,
                                   stdout=subprocess.DEVNULL, env=dict(os.environ, TMPDIR=str(work)))

    def monitor(self, cmd):
        self.vm.stdin.write((cmd + "\n").encode())
        self.vm.stdin.flush()

    def key(self, key):
        self.monitor("sendkey " + key + " 20")
        time.sleep(0.045)

    def enter(self, line):
        for c in line:
            self.key("shift-" + c.lower() if "A" <= c <= "Z" else KEYS.get(c, c))
        self.key("ret")

    def wait(self, text, since=0, timeout=40):
        deadline = time.monotonic() + timeout
        while self.vm.poll() is None and time.monotonic() < deadline:
            if text in self.log.read_bytes()[since:]:
                return
            time.sleep(0.06)
        raise RuntimeError(f"{self.log.name}: missing {text!r}: {self.log.read_bytes()[-1400:]!r}")

    def command(self, line, prompt=b"C:\\> ", expect=None, timeout=40):
        start = self.log.stat().st_size
        self.enter(line)
        self.wait(prompt, start, timeout)
        if expect is not None:
            self.wait(expect, start, timeout)

    def close(self):
        try:
            if self.vm.poll() is None:
                self.monitor("quit")
                self.vm.wait(timeout=30)
        finally:
            if self.vm.poll() is None:
                self.vm.kill()
                self.vm.wait()
            self.vm.stdin.close()

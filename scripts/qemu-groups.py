#!/usr/bin/env python3
"""Compare native process/group permissions and Rust identity tools with Linux GNU 9.12."""
import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile

import qemu
from qemu_shell import Shell

TOOLS = ('chgrp', 'chown', 'groups', 'id', 'whoami')
ROOT = qemu.ROOT
spec = importlib.util.spec_from_file_location('linux_tools', ROOT / 'scripts/linux-btrfs.py')
linux = importlib.util.module_from_spec(spec)
spec.loader.exec_module(linux)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def files():
    return {
        'passwd': (ROOT/'userland/etc/passwd').read_bytes() + b'operator:!:70001:70000:Operator:/home/operator:/bin/sh\n',
        'group': (ROOT/'userland/etc/group').read_bytes() + b'operators:x:70000:\nreaders:x:81:operator\narchive:x:70002:operator\nrestricted:x:70003:\n',
    }


def oracle(work, proof, bins, cache, probe):
    linux_kernel = linux.download(linux.BOOT, 'vmlinuz-virt', linux.BOOT_HASHES['vmlinuz-virt'], cache)
    entries = linux.packages(cache)
    for directory in ['bin','etc','proc','dev','tmp','work']:
        entries[directory] = (stat.S_IFDIR | 0o755, b'')
    entries['dev/console'] = (stat.S_IFCHR | 0o600, b'')
    for name, data in files().items():
        entries['etc/' + name] = (stat.S_IFREG | 0o644, data)
    if probe is None:
        probe = work / "linux.elf"
        env = dict(os.environ, ZIG_GLOBAL_CACHE_DIR=str(ROOT / "target/gnu-linux-df/zig-cache"))
        subprocess.run(["zig", "cc", "-target", "aarch64-linux-musl", "-static", "-g", "-O2",
                        ROOT / "tests/native-groups/identity.c", "-o", probe], env=env, check=True)
    entries['probe.elf'] = (stat.S_IFREG | 0o755, probe.read_bytes())
    for name in TOOLS:
        entries['bin/'+name] = (stat.S_IFREG | 0o755, (bins/name).read_bytes())
    script = '''#!/bin/busybox.static sh
/bin/busybox.static --install -s /bin
exec >/dev/console 2>&1
set -e
trap '/bin/busybox.static poweroff -f' EXIT
mount -t devtmpfs devtmpfs /dev
mount -t proc proc /proc
export LC_ALL=C LANG=C TZ=UTC PATH=/bin
for tool in id groups whoami chgrp chown; do /bin/$tool --version | head -1; done
/probe.elf
echo REPORT_BEGIN
base64 /work/report
echo REPORT_END
stat -c 'OWNER %n %u %g %a' /work/change /work/own /work/groupfile
'''
    entries['init'] = (stat.S_IFREG | 0o755,script.encode())
    initrd = work/'oracle-initrd.gz'
    linux.newc(entries, initrd)
    cmd = ['qemu-system-aarch64','-machine','virt','-cpu','cortex-a57','-m','384','-nic','none','-nographic','-no-reboot','-kernel', linux_kernel,'-initrd',initrd,'-append','console=ttyAMA0 loglevel=3 panic=-1 rdinit=/init']
    serial = proof / "gnu-linux.serial"
    with serial.open("wb") as output:
        subprocess.run(cmd, stdout=output, stderr=subprocess.STDOUT, timeout=180, check=True)
    data = serial.read_bytes().replace(b"\r", b"")
    for name in TOOLS:
        if f'{name} (GNU coreutils) 9.12\n'.encode() not in data:
            raise RuntimeError(f'Missing GNU 9.12 {name} in {proof}/gnu-linux.serial')
    if b'REPORT_END\n' not in data or b'identity suite complete' not in data:
        raise RuntimeError(f'Linux permission or utility probe failed: {proof}/gnu-linux.serial')
    owners = {n.split('/')[-1]: [int(u),int(g),int(m,8)] for row in data.decode().splitlines() if row.startswith('OWNER ') for _,n,u,g,m in [row.split()]}
    return base64.b64decode(data.split(b'REPORT_BEGIN\n')[1].split(b'REPORT_END\n')[0]), owners


def verify(arch, work, args, expected, owners):
    mode='bios' if arch=='i686' else 'uefi'
    tree=work/'tree'
    for d in ['etc','bin','work']:
        (tree/d).mkdir(parents=True)
    for name, data in files().items(): (tree/'etc'/name).write_bytes(data)
    for name in TOOLS:
        shutil.copy2(args.rust_bin/arch/name,tree/'bin'/name)
    subprocess.run([sys.executable,ROOT/'ports/musl/cc.py','--arch',arch,'-o',tree/'probe.elf',ROOT/'tests/native-groups/identity.c'],check=True)
    image=work/'root.img'
    with (args.proof/(arch+'-format.log')).open('wb') as log:
        subprocess.run([sys.executable, ROOT/'scripts/linux-btrfs.py','create','--root',tree,'--image',image],stdout=log,stderr=subprocess.STDOUT,check=True)
    kernel = qemu.build(arch, ROOT / 'target', features='exec-trace') if args.kernel is None else args.kernel
    serial=args.proof/(arch+'.serial')
    boot_args = qemu.boot_args(kernel, arch, work / 'boot', 2, mode, esp_mib=36)
    guest=Shell(boot_args+['-drive',f'if=ide,file={image},format=raw','-device','virtio-rng-pci'],serial,work)
    try:
        guest.wait(b'A:\\> ', timeout=240)
        guest.command('c:', timeout=120)
        mark=serial.stat().st_size
        guest.enter('probe.elf')
        guest.wait(b'C:\\> ',since=mark,timeout=480)
        output=serial.read_bytes()[mark:]
        if b'identity suite complete' not in output:
            raise RuntimeError(f'Native identity probe failed: {serial}')
    finally:
        guest.close()
    restored=work/'restored'
    with (args.proof/(arch+'-fsck.log')).open('wb') as log:
        subprocess.run([sys.executable, ROOT/'scripts/linux-btrfs.py','check','--image',image,'--output',restored],stdout=log,stderr=subprocess.STDOUT,check=True)
    actual=(restored/'files/work/report').read_bytes()
    (args.proof/(arch+'.report')).write_bytes(actual)
    records={e['path']:e for e in json.loads((restored/'archive.json').read_text())}
    native={name:[item['uid'],item['gid'],item['mode']&0o7777] for name in owners for item in [records['files/work/'+name]]}
    shutil.copy2(restored/'check.log',args.proof/(arch+'.check'))
    if actual!=expected or native!=owners or (restored/'files/work/groupfile').read_bytes()!=b'G':
        raise RuntimeError(f'{arch} differs from GNU or the restored inode owners: {native} vs {owners}')
    for name in TOOLS:
        if ('path=/bin/'+name+'\r\n').encode() not in output:
            raise RuntimeError('missing executed ELF: '+name)
    result=dict(arch=arch,firmware=mode,kernel=digest(kernel.read_bytes()),probe=digest((tree/'probe.elf').read_bytes()),owners=native,report=digest(actual),gnu={n:digest((args.gnu_bin/n).read_bytes()) for n in TOOLS},rust={n:digest((tree/'bin'/n).read_bytes()) for n in TOOLS})
    (args.proof/(arch+'.json')).write_text(json.dumps(result,indent=2)+'\n')
    print(f'{arch}/{mode}: 16 GNU 9.12 identity and ownership cases matched, restored owners and group permissions verified',flush=True)


if __name__=='__main__':
    cli=argparse.ArgumentParser(description=__doc__)
    cli.add_argument('--ram-dir',required=True,type=Path)
    cli.add_argument('--proof',type=Path,default=ROOT/'target/native-groups-proof')
    cli.add_argument('--linux-probe', type=Path, help='prebuilt AArch64 Linux tests/native-groups/identity.c')
    cli.add_argument('--gnu-bin',required=True,type=Path,help='static GNU 9.12 AArch64 Linux tools')
    cli.add_argument('--rust-bin',required=True,type=Path,help='built i686/ and x86_64/ DOS-RS tools')
    cli.add_argument('--cache',type=Path,default=ROOT/'target/vfs-linux-tools')
    cli.add_argument('--arch',choices=qemu.ARCH,action='append')
    cli.add_argument('--kernel', type=Path, help='use an existing exec-trace kernel for exactly one --arch')
    args=cli.parse_args()
    if args.kernel and len(args.arch or []) != 1:
        cli.error('--kernel requires a single --arch')
    args.proof.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(dir=args.ram_dir) as tmp:
        expected,owners=oracle(Path(tmp),args.proof,args.gnu_bin,args.cache,args.linux_probe)
    (args.proof/'gnu.report').write_bytes(expected)
    for arch in args.arch or qemu.ARCH:
        with tempfile.TemporaryDirectory(dir=args.ram_dir) as tmp:
            verify(arch,Path(tmp),args,expected,owners)

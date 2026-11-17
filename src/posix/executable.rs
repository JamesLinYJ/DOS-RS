//! Native executable loading and checked copies between process and kernel memory.
use super::{io::ProcessContext, threads::Threads};
use crate::{
    arch::frames::UserImage as Image,
    arch::user::{AddressSpace, Thread},
    vfs::{self, Directories},
};
use alloc::{string::String, vec::Vec};
use core::{
    future::poll_fn,
    task::{Context, Poll},
};
use dosrs_native_exec::{Call, END, Machine, STACK};
use dosrs_native_posix::{Errno, Open, Process, Whence};

pub(super) struct Executable {
    image: AddressSpace,
    threads: Threads,
}
fn push_stack(image: &mut Image, sp: &mut usize, data: &[u8]) -> Result<(), &'static str> {
    *sp = sp
        .checked_sub(data.len())
        .filter(|&v| v >= STACK)
        .ok_or("program arguments exceed user stack")?;
    image.write(*sp, data)
}
fn initial_stack<T: AsRef<[u8]>>(
    image: &mut Image,
    argv: &[T],
    env: &[T],
    uid: u32,
    gid: u32,
) -> Result<(usize, usize), &'static str> {
    let mut sp = END;
    let mut pointers = Vec::new();
    pointers
        .try_reserve_exact(argv.len() + env.len())
        .map_err(|_| "insufficient argument memory")?;
    for value in argv.iter().chain(env).rev() {
        push_stack(image, &mut sp, &[0])?;
        push_stack(image, &mut sp, value.as_ref())?;
        pointers.push(sp);
    }
    sp &= !(size_of::<usize>() - 1);
    let (phdr, phent, phnum) = image.phdr.unwrap_or_default();
    for (key, value) in [
        (0usize, 0usize),
        (11, uid as usize),
        (12, uid as usize),
        (13, gid as usize),
        (14, gid as usize),
        (6, 4096),
        (5, phnum),
        (4, phent),
        (3, phdr),
    ] {
        push_stack(image, &mut sp, &value.to_le_bytes())?;
        push_stack(image, &mut sp, &key.to_le_bytes())?;
    }
    push_stack(image, &mut sp, &0usize.to_le_bytes())?;
    for ptr in &pointers[..env.len()] {
        push_stack(image, &mut sp, &ptr.to_le_bytes())?;
    }
    push_stack(image, &mut sp, &0usize.to_le_bytes())?;
    for ptr in &pointers[env.len()..] {
        push_stack(image, &mut sp, &ptr.to_le_bytes())?;
    }
    let vector = sp;
    sp &= !15;
    #[cfg(target_arch = "x86")]
    {
        sp -= 8;
        push_stack(image, &mut sp, &vector.to_le_bytes())?;
        push_stack(image, &mut sp, &argv.len().to_le_bytes())?;
    }
    push_stack(image, &mut sp, &0usize.to_le_bytes())?;
    Ok((sp, vector))
}
impl Executable {
    pub fn load(cwd: Directories, path: &str, argv: &[String]) -> Result<Self, &'static str> {
        let (file, _) = vfs::open(cwd, path, Open::READ).map_err(|e| e.message())?;
        Self::from_file(file, argv, &[], 0, 0)
    }

    pub(super) fn from_file<T: AsRef<[u8]>>(
        file: vfs::File,
        argv: &[T],
        env: &[T],
        uid: u32,
        gid: u32,
    ) -> Result<Self, &'static str> {
        let length = usize::try_from(file.len().map_err(|e| e.message())?)
            .map_err(|_| "ELF file too large")?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| "insufficient memory to load ELF")?;
        bytes.resize(length, 0);
        let mut at = 0;
        while at < length {
            let n = file
                .read(at as u64, &mut bytes[at..])
                .map_err(|e| e.message())?;
            if n == 0 {
                return Err("truncated ELF");
            }
            at += n;
        }
        let mut image = Image::load(
            &bytes,
            if cfg!(target_arch = "x86") {
                Machine::I386
            } else {
                Machine::Amd64
            },
        )?;
        let (sp, vector) = initial_stack(&mut image, argv, env, uid, gid)?;
        let cpu = Thread::try_new(image.entry, sp, argv.len(), vector)?;
        let mut image: AddressSpace = image.into();
        image.prepare()?;
        Ok(Self {
            image,
            threads: Threads::new(cpu)?,
        })
    }
    pub(super) fn fork(
        image: &AddressSpace,
        registers: &Thread,
        stack: core::ops::Range<usize>,
        errno: usize,
    ) -> Result<Self, isize> {
        let mut image: AddressSpace = image.fork().map_err(|_| -12isize)?.into();
        image.prepare().map_err(|_| -12isize)?;
        let cpu = registers.fork().map_err(|_| -12isize)?;
        Ok(Self {
            image,
            threads: Threads::from_caller(cpu, stack, errno).map_err(|_| -12isize)?,
        })
    }
    pub(super) fn inherit_handlers(&mut self, caller: &Threads, i: usize) -> Result<(), isize> {
        self.threads.inherit_handlers(caller, i)
    }
    pub(super) fn set_mask(&mut self, mask: u64) {
        self.threads.set_mask(mask);
    }
    pub(super) fn replace(self, image: &mut AddressSpace, threads: &mut Threads) {
        *image = self.image;
        *threads = self.threads;
    }
    pub async fn run(mut self, process: &mut ProcessContext) -> u8 {
        match poll_fn(|cx| self.threads.poll(&mut self.image, process, cx)).await {
            Ok(code) => code,
            Err((code, message)) => {
                process.wait_signal = match code {
                    128 | 144 | 147 => 8, // divide, x87 and SIMD arithmetic faults
                    134 => 4,             // invalid opcode
                    131 => 5,             // breakpoint
                    _ => 11,
                };
                let _ = process.write_all(2, message.as_bytes()).await;
                code
            }
        }
    }
}

pub(super) fn file_call(
    image: &mut AddressSpace,
    p: &mut ProcessContext,
    cx: &Context,
    call: Call,
    args: [usize; 3],
    tty_read: &mut super::io::ReadClock,
) -> Poll<Result<usize, isize>> {
    let [fd, address, length] = args;
    if call == Call::Getrandom {
        let [address, length, flags] = args;
        if flags & !7 != 0 || flags & 6 == 6 {return Poll::Ready(Err(-22));}
        let mut bytes = [0; 256];
        let n = length.min(bytes.len());
        if n == 0 {return Poll::Ready(Ok(0));}
        image.check(address,n,true).map_err(|_|-14isize)?;
        let count = core::task::ready!(crate::drivers::entropy::fill(&mut bytes[..n],flags & 1 != 0,cx))?;
        image.write(address,&bytes[..count]).map_err(|_|-14isize)?;
        return Poll::Ready(Ok(count));
    }
    if matches!(call, Call::Read | Call::Write) {
        let length = length.min(p.transfer_limit(fd));
        image
            .check(address, length, call == Call::Read)
            .map_err(|_| -14isize)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(|_| -12isize)?;
        bytes.resize(length, 0);
        if call == Call::Write {
            image.read(address, &mut bytes).map_err(|_| -14isize)?;
            return p.poll_write(fd, cx, &bytes).map(|r| r.map_err(errno));
        }
        let count = core::task::ready!(p.poll_read(fd, cx, &mut bytes, tty_read)).map_err(errno)?;
        image
            .write(address, &bytes[..count])
            .map_err(|_| -14isize)?;
        return Poll::Ready(Ok(count));
    }
    Poll::Ready(immediate(image, p, call, args))
}

fn immediate(
    image: &mut AddressSpace,
    p: &mut ProcessContext,
    call: Call,
    args: [usize; 3],
) -> Result<usize, isize> {
    let [a, b, c] = args;
    if let Some(result) = super::paths::dispatch(image, p, call, args) {
        return result;
    }
    match call {
        Call::Uname => super::uts::uname(image, a),
        Call::Sethostname => super::uts::sethostname(image, p.uid, a, b),
        Call::Pipe | Call::Pipe2 => {
            image.check(a, 8, true).map_err(|_| -14isize)?;
            let (r, w) = p.pipe2(if call == Call::Pipe2 { b } else { 0 }).map_err(errno)?;
            image
                .write(a, &(r as i32).to_le_bytes())
                .map_err(|_| -14isize)?;
            image
                .write(a + 4, &(w as i32).to_le_bytes())
                .map_err(|_| -14isize)?;
            Ok(0)
        }
        Call::Dup => p.dup(a).map_err(errno),
        Call::Dup3 => p.dup3(a, b, c).map_err(errno),
        Call::Tcgetpgrp | Call::Tcsetpgrp | Call::Tcgetsid => p.tty_group(a, call, b as i32),
        Call::Getpid => Ok(p.getpid() as usize),
        Call::Getuid => Ok(p.uid as usize),
        Call::Getgid => Ok(p.gid as usize),
        Call::Mapmem => image.map_anonymous(a).map_err(memory_error),
        Call::Unmapmem => image.unmap_anonymous(a, b).map(|_| 0).map_err(memory_error),
        Call::Meminfo => {
            let (total, free) = crate::arch::frames::counts();
            let values = [total, free, image.pages().count()];
            image
                .check(a, size_of_val(&values), true)
                .map_err(|_| -14isize)?;
            for (i, value) in values.into_iter().enumerate() {
                image
                    .write(a + i * size_of::<usize>(), &value.to_le_bytes())
                    .map_err(|_| -14isize)?;
            }
            Ok(0)
        }
        Call::Close => p.close(a).map(|_| 0).map_err(errno),
        Call::Dup2 => p.dup2(a, b).map_err(errno),
        Call::Lseek => {
            let mut bytes = [0; 8];
            image.check(b, 8, true).map_err(|_| -14isize)?;
            image.read(b, &mut bytes).map_err(|_| -14isize)?;
            let whence = match c {
                0 => Whence::Start,
                1 => Whence::Current,
                2 => Whence::End,
                _ => return Err(-22),
            };
            let pos = p
                .lseek(a, i64::from_le_bytes(bytes), whence)
                .map_err(errno)?;
            image.write(b, &pos.to_le_bytes()).map_err(|_| -14isize)?;
            Ok(0)
        }
        _ => Err(-38),
    }
}
fn memory_error(error: &'static str) -> isize {
    match error {
        "invalid anonymous mapping" | "unmapped user address" => -22,
        _ => -12,
    }
}
pub(super) fn errno(error: Errno) -> isize {
    -match error {
        Errno::NoEntry => 2,
        Errno::Exists => 17,
        Errno::BadFd => 9,
        Errno::Access => 13,
        Errno::IsDirectory => 21,
        Errno::Invalid => 22,
        Errno::NameTooLong => 36,
        Errno::NoSpace => 28,
        Errno::TooManyFiles => 24,
        Errno::Io => 5,
        Errno::BrokenPipe => 32,
        Errno::NotDirectory => 20,
        Errno::ReadOnly => 30,
        Errno::NotSupported => 95,
        Errno::IllegalSeek => 29,
        Errno::FileTooLarge => 27,
        Errno::Permission => 1,
        Errno::Loop => 40,
        Errno::NotEmpty => 39,
        Errno::CrossDevice => 18,
        Errno::Range => 34,
        Errno::Busy => 16,
        Errno::Again => 11,
    }
}

fn strings(image: &Image, base: usize, remaining: &mut usize) -> Result<Vec<Vec<u8>>, isize> {
    let mut values = Vec::new();
    if base == 0 {
        return Ok(values);
    }
    let mut cursor = base;
    loop {
        let mut ptr = [0; size_of::<usize>()];
        image.read(cursor, &mut ptr).map_err(|_| -14isize)?;
        cursor = cursor.checked_add(ptr.len()).ok_or(-14isize)?;
        let mut at = usize::from_le_bytes(ptr);
        if at == 0 {
            return Ok(values);
        }
        *remaining = remaining.checked_sub(ptr.len()).ok_or(-7isize)?;
        let mut bytes = Vec::new();
        loop {
            *remaining = remaining.checked_sub(1).ok_or(-7isize)?;
            let mut ch = [0];
            image.read(at, &mut ch).map_err(|_| -14isize)?;
            if ch[0] == 0 {
                break;
            }
            bytes.try_reserve(1).map_err(|_| -12isize)?;
            bytes.push(ch[0]);
            at = at.checked_add(1).ok_or(-14isize)?;
        }
        values.try_reserve(1).map_err(|_| -12isize)?;
        values.push(bytes);
    }
}

pub(super) fn execve(
    image: &AddressSpace,
    p: &ProcessContext,
    args: [usize; 3],
) -> Result<Executable, isize> {
    let name = super::paths::string(image, args[0])?;
    let mut remaining = END - STACK - 1024;
    let argv = strings(image, args[1], &mut remaining)?;
    let env = strings(image, args[2], &mut remaining)?;
    let file = p.open_executable(&name).map_err(errno)?;
    let task = Executable::from_file(file, &argv, &env, p.uid, p.gid).map_err(|e| {
        if e.contains("insufficient") {
            -12isize
        } else if e.contains("exceed user stack") {
            -7
        } else {
            -8
        }
    })?;
    #[cfg(feature = "exec-trace")]
    for byte in alloc::format!(
        "EXEC pid={} path={}\r\n",
        p.getpid(),
        String::from_utf8_lossy(&name)
    )
    .bytes()
    {
        crate::bios::pc::serial_write(0x3f8, byte);
    }
    Ok(task)
}

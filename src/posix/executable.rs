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
use dosrs_native_posix::{Access, Errno, Open, Process, Whence};

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
impl Executable {
    pub fn load(cwd: Directories, path: &str, argv: &[String]) -> Result<Self, &'static str> {
        let (file, _) = vfs::open(cwd, path, Open::READ).map_err(|e| e.message())?;
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
        let mut sp = END;
        let mut pointers = Vec::with_capacity(argv.len());
        for arg in argv.iter().rev() {
            push_stack(&mut image, &mut sp, &[0])?;
            push_stack(&mut image, &mut sp, arg.as_bytes())?;
            pointers.push(sp);
        }
        sp &= !(size_of::<usize>() - 1);
        push_stack(&mut image, &mut sp, &0usize.to_le_bytes())?;
        for ptr in pointers {
            push_stack(&mut image, &mut sp, &ptr.to_le_bytes())?;
        }
        let vector = sp;
        sp &= !15;
        #[cfg(target_arch = "x86")]
        {
            sp -= 8;
            push_stack(&mut image, &mut sp, &vector.to_le_bytes())?;
            push_stack(&mut image, &mut sp, &argv.len().to_le_bytes())?;
        }
        push_stack(&mut image, &mut sp, &0usize.to_le_bytes())?;
        let cpu = Thread::try_new(image.entry, sp, argv.len(), vector)?;
        let mut image: AddressSpace = image.into();
        image.prepare()?;
        Ok(Self {
            image,
            threads: Threads::new(cpu)?,
        })
    }
    pub async fn run(mut self, process: &mut ProcessContext) -> u8 {
        match poll_fn(|cx| self.threads.poll(&mut self.image, process, cx)).await {
            Ok(code) => code,
            Err((code, message)) => {
                let _ = process.write_all(2, message.as_bytes()).await;
                code
            }
        }
    }
}

fn path(image: &Image, address: usize) -> Result<String, isize> {
    let mut bytes = Vec::new();
    for i in 0..256 {
        let mut b = [0];
        image
            .read(address.checked_add(i).ok_or(-14isize)?, &mut b)
            .map_err(|_| -14isize)?;
        if b[0] == 0 {
            return String::from_utf8(bytes).map_err(|_| -22);
        }
        bytes.push(b[0]);
    }
    Err(-36)
}

pub(super) fn file_call(
    image: &mut AddressSpace,
    p: &mut ProcessContext,
    cx: &Context,
    call: Call,
    args: [usize; 3],
) -> Poll<Result<usize, isize>> {
    let [fd, address, length] = args;
    if matches!(call, Call::Read | Call::Write) {
        let length = length.min(4096);
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
        let count = core::task::ready!(p.poll_read(fd, cx, &mut bytes)).map_err(errno)?;
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
    match call {
        Call::Pipe => {
            image.check(a, 8, true).map_err(|_| -14isize)?;
            let (r, w) = p.pipe().map_err(errno)?;
            image
                .write(a, &(r as i32).to_le_bytes())
                .map_err(|_| -14isize)?;
            image
                .write(a + 4, &(w as i32).to_le_bytes())
                .map_err(|_| -14isize)?;
            Ok(0)
        }
        Call::Dup => p.dup(a).map_err(errno),
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
        Call::Open => {
            let access = match b & 3 {
                0 => Access::Read,
                1 => Access::Write,
                2 => Access::ReadWrite,
                _ => return Err(-22),
            };
            if b & !0x6c3 != 0 {
                return Err(-22);
            }
            p.open(
                &path(image, a)?,
                Open {
                    access,
                    create: b & 64 != 0,
                    exclusive: b & 128 != 0,
                    truncate: b & 512 != 0,
                    append: b & 1024 != 0,
                },
            )
            .map_err(errno)
        }
        Call::Unlink => p.unlink(&path(image, a)?).map(|_| 0).map_err(errno),
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
fn errno(error: Errno) -> isize {
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
    }
}

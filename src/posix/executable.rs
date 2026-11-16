//! Native executable loading and checked copies between process and kernel memory.
use super::io::ProcessContext;
use crate::{
    arch::user::{Stop, Thread},
    vfs::{self, Directories},
};
use alloc::{string::String, vec::Vec};
use core::{future::poll_fn, task::Poll};
use dosrs_native_exec::{Call, END, Image, Machine, STACK};
use dosrs_native_posix::{Access, Errno, Open, Process, Whence};

pub(super) struct Executable {
    image: Image,
    cpu: Thread,
}
const MAX_FILE: usize = 2 << 20;
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
            .ok()
            .filter(|&n| n <= MAX_FILE)
            .ok_or("ELF file too large")?;
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
        let cpu = Thread::new(image.entry, sp, argv.len(), vector);
        Ok(Self { image, cpu })
    }
    pub async fn run(mut self, process: &mut ProcessContext) -> u8 {
        loop {
            let stop = match self.cpu.step(&mut self.image) {
                Ok(s) => s,
                Err(e) => {
                    let _ = process.write_all(2, e.as_bytes()).await;
                    return 126;
                }
            };
            match stop {
                Stop::Fault(vector) => {
                    let message = alloc::format!(
                        "native process {}: user exception {:02x}\n",
                        process.getpid(),
                        vector
                    );
                    let _ = process.write_all(2, message.as_bytes()).await;
                    return 128 + (vector.min(127) as u8);
                }
                Stop::Slice => (),
                Stop::Call(number) => {
                    if let Some((call, count)) = Call::decode(number) {
                        let Some(args) = self.cpu.args(&self.image, count) else {
                            return 139;
                        };
                        if call == Call::Exit {
                            return args[0] as u8;
                        }
                        let result = self.call(process, call, args).await;
                        self.cpu.result(result);
                    } else {
                        self.cpu.result(-38);
                    }
                }
            }
            // Even a series of nonblocking system calls yields to other stages.
            let mut first = true;
            poll_fn(|cx| {
                if first {
                    first = false;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            })
            .await;
        }
    }
    fn path(&self, address: usize) -> Result<String, isize> {
        let mut bytes = Vec::new();
        for i in 0..256 {
            let mut b = [0];
            self.image
                .read(address.checked_add(i).ok_or(-14isize)?, &mut b)
                .map_err(|_| -14isize)?;
            if b[0] == 0 {
                return String::from_utf8(bytes).map_err(|_| -22);
            }
            bytes.push(b[0]);
        }
        Err(-36)
    }
    async fn call(&mut self, p: &mut ProcessContext, call: Call, args: [usize; 3]) -> isize {
        let [a, b, c] = args;
        let result: Result<usize, isize> = async {
            match call {
                Call::Pipe => {
                    self.image.check(a, 8, true).map_err(|_| -14isize)?;
                    let (r, w) = p.pipe().map_err(errno)?;
                    self.image
                        .write(a, &(r as i32).to_le_bytes())
                        .map_err(|_| -14isize)?;
                    self.image
                        .write(a + 4, &(w as i32).to_le_bytes())
                        .map_err(|_| -14isize)?;
                    Ok(0)
                }
                Call::Read | Call::Write => {
                    let len = c.min(4096);
                    self.image
                        .check(b, len, call == Call::Read)
                        .map_err(|_| -14isize)?;
                    let mut buf = alloc::vec![0;len];
                    if call == Call::Write {
                        self.image.read(b, &mut buf).map_err(|_| -14isize)?;
                        p.write(a, &buf).await.map_err(errno)
                    } else {
                        let count = p.read(a, &mut buf).await.map_err(errno)?;
                        self.image.write(b, &buf[..count]).map_err(|_| -14isize)?;
                        Ok(count)
                    }
                }
                Call::Dup => p.dup(a).map_err(errno),
                Call::Getpid => Ok(p.getpid() as usize),
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
                        &self.path(a)?,
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
                Call::Unlink => p.unlink(&self.path(a)?).map(|_| 0).map_err(errno),
                Call::Close => p.close(a).map(|_| 0).map_err(errno),
                Call::Dup2 => p.dup2(a, b).map_err(errno),
                Call::Lseek => {
                    let mut bytes = [0; 8];
                    self.image.check(b, 8, true).map_err(|_| -14isize)?;
                    self.image.read(b, &mut bytes).map_err(|_| -14isize)?;
                    let whence = match c {
                        0 => Whence::Start,
                        1 => Whence::Current,
                        2 => Whence::End,
                        _ => return Err(-22),
                    };
                    let pos = p
                        .lseek(a, i64::from_le_bytes(bytes), whence)
                        .map_err(errno)?;
                    self.image
                        .write(b, &pos.to_le_bytes())
                        .map_err(|_| -14isize)?;
                    Ok(0)
                }
                Call::Exit => unreachable!(),
            }
        }
        .await;
        match result {
            Ok(v) => v as isize,
            Err(e) => e,
        }
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

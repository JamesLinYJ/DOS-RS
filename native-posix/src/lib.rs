//! Source interface for native DOS-RS programs. Blocking I/O suspends the
//! calling task. The kernel owns descriptor tables and all device access.
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;
use alloc::string::String;
use core::future::Future;
mod utilities;

pub type Fd = usize;
pub type Result<T> = core::result::Result<T, Errno>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Errno {
    NoEntry,
    Exists,
    BadFd,
    Access,
    IsDirectory,
    Invalid,
    NameTooLong,
    NoSpace,
    TooManyFiles,
    Io,
    BrokenPipe,
    NotDirectory,
    ReadOnly,
    NotSupported,
    IllegalSeek,
    FileTooLarge,
    Permission,
    Loop,
    NotEmpty,
    CrossDevice,
    Range,
    Busy,
}
impl Errno {
    pub fn message(self) -> &'static str {
        match self {
            Self::NoEntry => "no such file or directory",
            Self::Exists => "file exists",
            Self::BadFd => "bad file descriptor",
            Self::Access => "permission denied",
            Self::IsDirectory => "is a directory",
            Self::Invalid => "invalid argument",
            Self::NameTooLong => "file name too long",
            Self::NoSpace => "no space left",
            Self::TooManyFiles => "too many open files",
            Self::Io => "I/O error",
            Self::BrokenPipe => "broken pipe",
            Self::NotDirectory => "not a directory",
            Self::ReadOnly => "read-only file system",
            Self::NotSupported => "operation not supported",
            Self::IllegalSeek => "illegal seek",
            Self::FileTooLarge => "file too large",
            Self::Permission => "operation not permitted",
            Self::Loop => "too many symbolic links",
            Self::NotEmpty => "directory not empty",
            Self::CrossDevice => "cross-device link",
            Self::Range => "buffer too small",
            Self::Busy => "resource busy",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    ReadWrite,
}
impl Access {
    pub fn readable(self) -> bool {
        self != Self::Write
    }
    pub fn writable(self) -> bool {
        self != Self::Read
    }
}

#[derive(Clone, Copy)]
pub struct Open {
    pub access: Access,
    pub create: bool,
    pub exclusive: bool,
    pub truncate: bool,
    pub append: bool,
}
impl Open {
    pub const READ: Self = Self {
        access: Access::Read,
        create: false,
        exclusive: false,
        truncate: false,
        append: false,
    };
    pub const OUTPUT: Self = Self {
        access: Access::Write,
        create: true,
        exclusive: false,
        truncate: true,
        append: false,
    };
    pub const APPEND: Self = Self {
        append: true,
        truncate: false,
        ..Self::OUTPUT
    };
    pub const NEW: Self = Self {
        access: Access::ReadWrite,
        create: true,
        exclusive: true,
        truncate: false,
        append: false,
    };
}

#[derive(Clone, Copy)]
pub enum Whence {
    Start,
    Current,
    End,
}

pub trait Process {
    fn lseek(&mut self, fd: Fd, delta: i64, whence: Whence) -> Result<u64>;
    fn getpid(&self) -> u32;
    fn open(&mut self, path: &str, options: Open) -> Result<Fd>;
    fn unlink(&mut self, path: &str) -> Result<()>;
    fn dup(&mut self, fd: Fd) -> Result<Fd>;
    fn dup2(&mut self, fd: Fd, target: Fd) -> Result<Fd>;
    fn close(&mut self, fd: Fd) -> Result<()>;
    fn pipe(&mut self) -> Result<(Fd, Fd)>;
    fn read(&mut self, fd: Fd, out: &mut [u8]) -> impl Future<Output = Result<usize>> + Send;
    fn write(&mut self, fd: Fd, bytes: &[u8]) -> impl Future<Output = Result<usize>> + Send;
    fn write_all(&mut self, fd: Fd, bytes: &[u8]) -> impl Future<Output = Result<()>> + Send
    where
        Self: Send,
    {
        async move {
            let mut rest = bytes;
            while !rest.is_empty() {
                let count = self.write(fd, rest).await?;
                if count == 0 {
                    return Err(Errno::Io);
                }
                rest = &rest[count..];
            }
            Ok(())
        }
    }
}

#[derive(Clone, Copy)]
pub enum Program {
    Cat,
    Wc,
    FdCheck,
    Unlink,
}
impl Program {
    pub fn lookup(name: &str) -> Option<Self> {
        match name {
            "cat" => Some(Self::Cat),
            "wc" => Some(Self::Wc),
            "unlink" => Some(Self::Unlink),
            "fdcheck" => Some(Self::FdCheck),
            _ => None,
        }
    }
    pub async fn run(self, process: &mut (impl Process + Send), argv: &[String]) -> u8 {
        let result = match self {
            Self::Unlink => match &argv[1..] {
                [path] => process.unlink(path),
                _ => Err(Errno::Invalid),
            },
            Self::Cat => utilities::cat(process, &argv[1..]).await,
            Self::Wc => utilities::wc(process, &argv[1..]).await,
            Self::FdCheck => utilities::fdcheck(process, &argv[1..]).await,
        };
        match result {
            Ok(()) => 0,
            Err(error) => {
                let message = alloc::format!("{}: {}\n", argv[0], error.message());
                let _ = process.write_all(2, message.as_bytes()).await;
                1
            }
        }
    }
}

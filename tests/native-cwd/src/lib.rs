//! Exercise libc and the separately installed Shell from long and unnamed cwds.
#![no_std]
extern crate alloc;

use alloc::{ffi::CString, vec, vec::Vec};
use core::ffi::CStr;
use dosrs_user::{Args, cli, working_directory as cwd, write_all};

mod abi {
    use super::*;
    unsafe extern "C" {
        fn mkdir(path: *const i8, mode: u32) -> i32;
        fn rmdir(path: *const i8) -> i32;
        fn chmod(path: *const i8, mode: u32) -> i32;
        fn setuid(uid: u32) -> i32;
        fn fchdir(fd: i32) -> i32;
        fn getcwd(out: *mut u8, size: usize) -> *mut u8;
        fn pipe(fds: *mut i32) -> i32;
        fn fork() -> i32;
        fn dup2(from: i32, to: i32) -> i32;
        fn execve(path: *const i8, argv: *const *const i8, env: *const *const i8) -> i32;
        fn waitpid(pid: i32, status: *mut i32, flags: i32) -> i32;
    }
    fn check(n: i32) -> Result<i32, i32> {
        if n < 0 {
            Err(dosrs_user::errno())
        } else {
            Ok(n)
        }
    }
    pub fn create(path: &CStr) -> Result<(), i32> {
        // The borrowed pathname is terminated and live for the entire call.
        check(unsafe { mkdir(path.as_ptr(), 0o700) }).map(|_| ())
    }
    pub fn remove(path: &CStr) -> Result<(), i32> {
        // The borrowed pathname is terminated and live for the entire call.
        check(unsafe { rmdir(path.as_ptr()) }).map(|_| ())
    }
    pub fn mode(path: &CStr, mode: u32) -> Result<(), i32> {
        // The borrowed path remains valid for the whole operation.
        check(unsafe { chmod(path.as_ptr(), mode) }).map(|_| ())
    }
    pub struct Directory(i32);
    impl Directory {
        pub fn current() -> Result<Self, i32> {
            dosrs_user::open(c".", 0x10000 | 0x80000).map(Self)
        }
        pub fn enter(&self) -> Result<(), i32> {
            // This owner keeps its directory descriptor open until Drop.
            check(unsafe { fchdir(self.0) }).map(|_| ())
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = dosrs_user::close(self.0);
        }
    }
    pub fn name_into(out: &mut [u8]) -> Result<(), i32> {
        // libc may write only within this exclusive initialized slice.
        if unsafe { getcwd(out.as_mut_ptr(), out.len()) }.is_null() {
            Err(dosrs_user::errno())
        } else {
            Ok(())
        }
    }
    pub fn shell(script: &CStr, limit: usize) -> Result<(Vec<u8>, i32), i32> {
        shell_as(script, limit, None)
    }
    pub fn shell_as(script: &CStr, limit: usize, uid: Option<u32>) -> Result<(Vec<u8>, i32), i32> {
        let mut fds = [-1; 2];
        // pipe initializes exactly two descriptors in this array.
        check(unsafe { pipe(fds.as_mut_ptr()) })?;
        let child = unsafe { fork() };
        if child < 0 {
            let error = dosrs_user::errno();
            for fd in fds {
                let _ = dosrs_user::close(fd);
            }
            return Err(error);
        }
        if child == 0 {
            if let Some(uid) = uid
                && unsafe { setuid(uid) } < 0
            {
                dosrs_user::exit(125);
            }
            let _ = dosrs_user::close(fds[0]);
            if unsafe { dup2(fds[1], 1) } < 0 {
                dosrs_user::exit(126);
            }
            if fds[1] != 1 {
                let _ = dosrs_user::close(fds[1]);
            }
            let argv = [
                c"sh".as_ptr(),
                c"-c".as_ptr(),
                script.as_ptr(),
                core::ptr::null(),
            ];
            let env = [
                c"PATH=/bin".as_ptr(),
                c"LC_ALL=C".as_ptr(),
                core::ptr::null(),
            ];
            // Both vectors are terminated and all pointed-to strings remain live.
            unsafe { execve(c"/bin/sh".as_ptr(), argv.as_ptr(), env.as_ptr()) };
            dosrs_user::exit(127);
        }
        let _ = dosrs_user::close(fds[1]);
        let mut output = Vec::new();
        let mut error = None;
        let mut buffer = [0; 512];
        loop {
            match dosrs_user::read(fds[0], &mut buffer) {
                Ok(0) => break,
                Ok(n) if output.len() + n <= limit && error.is_none() => {
                    output.extend_from_slice(&buffer[..n]);
                }
                Ok(_) => error = Some(27),
                Err(e) => {
                    error = Some(e);
                    break;
                }
            }
        }
        let _ = dosrs_user::close(fds[0]);
        let mut status = 0;
        loop {
            // The status pointer refers to this initialized, exclusive local.
            if unsafe { waitpid(child, &mut status, 0) } >= 0 {
                break;
            }
            let e = dosrs_user::errno();
            if e != 4 {
                return Err(e);
            }
        }
        error.map_or(Ok((output, status)), Err)
    }
}

fn require(ok: bool) -> Result<(), i32> {
    if ok { Ok(()) } else { Err(5) }
}

fn long_directory() -> Result<(), i32> {
    let root = abi::Directory::current()?;
    let original = cwd::current()?;
    abi::create(c"LONGCWD")?;
    cwd::change(c"LONGCWD")?;
    let component = CString::new(vec![b'd'; 240]).unwrap();
    let mut depth = 0;
    let mut leaf = false;
    let result = (|| {
        let mut expected = original.clone();
        if expected.last() == Some(&b'/') {
            expected.pop();
        }
        expected.extend_from_slice(b"/LONGCWD");
        for _ in 0..18 {
            abi::create(&component)?;
            cwd::change(&component)?;
            depth += 1;
            expected.push(b'/');
            expected.extend_from_slice(component.to_bytes());
        }
        require(expected.len() > 4096)?;
        let mut small = [0x5a; 4096];
        let error = abi::name_into(&mut small).err().ok_or(5)?;
        if error != 34 {
            return Err(error);
        }
        require(small.iter().all(|&b| b == 0x5a))?;
        let mut exact = vec![0x5a; expected.len() + 1];
        require(abi::name_into(&mut exact[..expected.len()]) == Err(34))?;
        abi::name_into(&mut exact)?;
        require(exact[..expected.len()] == expected && exact[expected.len()] == 0)?;
        require(cwd::current()? == expected)?;
        abi::create(c"leaf")?;
        leaf = true;
        let (output, status) = abi::shell(c"cd leaf && pwd -P", expected.len() + 32)?;
        expected.extend_from_slice(b"/leaf\n");
        require(status == 0 && output == expected)?;
        write_all(
            1,
            b"cwd: long physical path, ERANGE and relative Shell cd passed\n",
        )
    })();
    // Clean up even on the old kernel's expected getcwd failure. A path longer
    // than the host's PATH_MAX must never be left for host-side restoration.
    let cleanup = (|| {
        if leaf {
            abi::remove(c"leaf")?;
        }
        while depth > 0 {
            cwd::change(c"..")?;
            abi::remove(&component)?;
            depth -= 1;
        }
        root.enter()?;
        abi::remove(c"LONGCWD")?;
        require(cwd::current()? == original)
    })();
    result.and(cleanup)
}

fn unnamed_directory() -> Result<(), i32> {
    let root = abi::Directory::current()?;
    abi::create(c"UNCWD")?;
    cwd::change(c"UNCWD")?;
    let result = (|| {
        abi::remove(c"../UNCWD")?;
        require(cwd::current() == Err(2))?;
        let (output, status) = abi::shell(
            c"cd -Pe . 2>&-; echo pe:$?; cd -P .; echo p:$?; pwd -P 2>&-; echo pwd:$?; cd /; pwd -P",
            64,
        )?;
        require(status == 0 && output == b"pe:1\np:0\npwd:1\n/\n")?;
        write_all(1, b"cwd: unlinked directory and Shell cd -Pe passed\n")
    })();
    let cleanup = root.enter();
    result.and(cleanup)
}

fn raw_name() -> Result<(), i32> {
    let root = abi::Directory::current()?;
    let name = c"raw-\xff";
    abi::create(name)?;
    let result = (|| {
        cwd::change(name)?;
        let (paths, status) = abi::shell(c"pwd -P && pwd -L", 32)?;
        require(status == 0 && paths == b"/raw-\xff\n/raw-\xff\n")?;
        write_all(1, b"cwd: physical pwd preserves raw pathname bytes\n")
    })();
    root.enter()?;
    abi::remove(name)?;
    result
}

fn directory_permissions() -> Result<(), i32> {
    abi::create(c"SEARCH")?;
    abi::create(c"PRIVATE")?;
    let result = (|| {
        abi::mode(c"SEARCH", 0o111)?;
        let (output, status) = abi::shell_as(
            c"cd /SEARCH || exit 4; cd - >&- 2>&-; echo output:$?; pwd -P; echo $PWD:$OLDPWD; cd -Pe /PRIVATE 2>&-; echo denied:$?; pwd -P; cd -Pe - >&- 2>&-; echo physical:$?; pwd -P; cd -; pwd -P",
            160,
            Some(65534),
        )?;
        if status != 0
            || output
                != b"output:1\n/SEARCH\n/SEARCH:/\ndenied:2\n/SEARCH\nphysical:2\n/SEARCH\n/\n/\n"
        {
            write_all(2, &output)?;
            return Err(5);
        }
        write_all(
            1,
            b"cwd: search-only directory, denied cd and output rollback passed\n",
        )
    })();
    abi::remove(c"PRIVATE")?;
    abi::remove(c"SEARCH")?;
    result
}

fn run(_: Args<'_>) -> i32 {
    let result = long_directory()
        .and_then(|_| unnamed_directory())
        .and_then(|_| raw_name())
        .and_then(|_| directory_permissions());
    match result {
        Ok(()) => 0,
        Err(error) => {
            cli::diagnostic(b"cwdprobe", b"directory assertions", error);
            1
        }
    }
}
dosrs_user::utility!(
    run,
    "cwdprobe",
    "Check native long and unnamed working directories.",
    0
);

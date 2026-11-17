//! Small documents fit in an empty pipe even on systems with the POSIX
//! minimum PIPE_BUF. Larger documents are backed by an unlinked private file.
use super::{close, error, pipe, write_all};
use alloc::{ffi::CString, format};
use core::ffi::c_char;

unsafe extern "C" {
    fn mkstemp(template: *mut c_char) -> i32;
    fn unlink(path: *const c_char) -> i32;
    fn lseek(fd: i32, offset: i64, whence: i32) -> i64;
}

pub(super) fn open(text: &[u8], tmpdir: &str) -> Result<i32, i32> {
    if text.len() <= 512 {
        let mut ends = [-1; 2];
        if unsafe { pipe(ends.as_mut_ptr()) } < 0 {
            return Err(error());
        }
        let result = write_all(ends[1], text);
        let _ = close(ends[1]);
        if let Err(e) = result {
            let _ = close(ends[0]);
            return Err(e);
        }
        return Ok(ends[0]);
    }
    let mut last = 2;
    for dir in [tmpdir, "/tmp", "."] {
        if dir.is_empty() {
            continue;
        }
        let Ok(name) = CString::new(format!("{}/HDXXXXXX", dir.trim_end_matches('/'))) else {
            continue;
        };
        let mut bytes = name.into_bytes_with_nul();
        let fd = unsafe { mkstemp(bytes.as_mut_ptr().cast()) };
        if fd < 0 {
            last = error();
            continue;
        }
        let result = if unsafe { unlink(bytes.as_ptr().cast()) } < 0 {
            Err(error())
        } else {
            write_all(fd, text).and_then(|_| {
                if unsafe { lseek(fd, 0, 0) } < 0 {
                    Err(error())
                } else {
                    Ok(())
                }
            })
        };
        if let Err(e) = result {
            let _ = close(fd);
            return Err(e);
        }
        return Ok(fd);
    }
    Err(last)
}

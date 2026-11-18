use super::abi;
use dosrs_user::{close, errno, open, read, seek, stat::Stat, write_all};

unsafe extern "C" {
    fn symlink(target: *const i8, link: *const i8) -> i32;
    fn unlink(path: *const i8) -> i32;
    fn ftruncate(fd: i32, size: i64) -> i32;
}

pub(super) fn check() -> Result<(), i32> {
    let device = c"/dev/null";
    let alias = c"/tmp/null-probe";
    let info = Stat::file(Some(device))?;
    if info.mode & 0o170777 != 0o020666 || info.rdev != 0x103 {
        write_all(
            2,
            alloc::format!("null stat mode={:o} rdev={}\n", info.mode, info.rdev).as_bytes(),
        )?;
        return Err(5);
    }
    // Both paths are terminated constants. The alias verifies inode-based dispatch.
    if unsafe { symlink(device.as_ptr(), alias.as_ptr()) } != 0 {
        return Err(errno());
    }
    let result = (|| {
        let fd = open(alias, 2 | 0x80000)?;
        let io = (|| {
            let info = Stat::fd(fd)?;
            if info.mode & 0o170000 != 0o020000 || info.rdev != 0x103 {
                write_all(
                    2,
                    alloc::format!("null fstat mode={:o} rdev={}\n", info.mode, info.rdev)
                        .as_bytes(),
                )?;
                return Err(5);
            }
            write_all(fd, b"discarded")?;
            let pos = seek(fd, 8, 0)?;
            let bytes = read(fd, &mut [0; 32])?;
            if pos != 0 || bytes != 0 {
                write_all(
                    2,
                    alloc::format!("null seek={pos}, read={bytes}\n").as_bytes(),
                )?;
                return Err(5);
            }
            // fd remains owned by this test until both I/O paths finish.
            let truncated = unsafe { ftruncate(fd, 1) };
            let error = errno();
            if truncated != -1 || error != 22 {
                write_all(
                    2,
                    alloc::format!("null truncate={truncated} errno={error}\n").as_bytes(),
                )?;
                return Err(5);
            }
            Ok(())
        })();
        let closed = close(fd);
        io.and(closed)?;
        let (out, status) = abi::shell_as(
            c"cd /dev && echo discarded > null && cat /tmp/null-probe && echo null-ok",
            64,
            Some(65534),
        )?;
        if status != 0 || out != b"null-ok\n" {
            write_all(
                2,
                alloc::format!("null shell status={status}, stdout={out:?}\n").as_bytes(),
            )?;
            return Err(5);
        }
        Ok(())
    })();
    let removed = if unsafe { unlink(alias.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(errno())
    };
    result.and(removed)?;
    write_all(
        1,
        b"null: char device 1:3, EOF, discard, seek and unprivileged redirection passed\n",
    )
}

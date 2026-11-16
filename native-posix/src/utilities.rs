use super::{Errno, Fd, Open, Process, Result};
use alloc::{format, string::String};

async fn copy(p: &mut (impl Process + Send), fd: Fd) -> Result<()> {
    let mut buffer = [0; 1024];
    loop {
        let n = p.read(fd, &mut buffer).await?;
        if n == 0 {
            return Ok(());
        }
        p.write_all(1, &buffer[..n]).await?;
    }
}

pub(super) async fn cat(p: &mut (impl Process + Send), paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return copy(p, 0).await;
    }
    for path in paths {
        if path == "-" {
            copy(p, 0).await?;
        } else {
            let fd = p.open(path, Open::READ)?;
            copy(p, fd).await?;
            p.close(fd)?;
        }
    }
    Ok(())
}

pub(super) async fn wc(p: &mut (impl Process + Send), args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) != Some("-c") || args.len() > 2 {
        return Err(Errno::Invalid);
    }
    let path = args.get(1).filter(|p| p.as_str() != "-");
    let fd = match path {
        Some(path) => p.open(path, Open::READ)?,
        None => 0,
    };
    let mut total = 0u64;
    let mut buffer = [0; 256];
    loop {
        let count = p.read(fd, &mut buffer).await?;
        if count == 0 {
            break;
        }
        total += count as u64;
    }
    let line = match path {
        Some(path) => {
            p.close(fd)?;
            format!("{total} {path}\n")
        }
        None => format!("{total}\n"),
    };
    p.write_all(1, line.as_bytes()).await
}

/// Conformance probe. O_EXCL ensures the path is ours before testing unlink.
pub(super) async fn fdcheck(p: &mut (impl Process + Send), args: &[String]) -> Result<()> {
    let [path] = args else {
        return Err(Errno::Invalid);
    };
    let (r, w) = p.pipe()?;
    let alias = p.dup(w)?;
    p.close(w)?;
    p.write_all(alias, b"pipe").await?;
    p.close(alias)?;
    let mut data = [0; 4];
    if p.read(r, &mut data).await? != 4 || &data != b"pipe" || p.read(r, &mut data).await? != 0 {
        return Err(Errno::Io);
    }
    p.close(r)?;
    let (r, w) = p.pipe()?;
    p.close(r)?;
    if p.write(w, b"x").await != Err(Errno::BrokenPipe) {
        return Err(Errno::Io);
    }
    p.close(w)?;
    let writer = p.open(path, Open::NEW)?;
    p.write_all(writer, b"original").await?;
    let reader = p.open(path, Open::READ)?;
    let shared = p.dup(reader)?;
    p.unlink(path)?;
    if p.open(path, Open::READ) != Err(Errno::NoEntry) {
        return Err(Errno::Io);
    }
    let new = p.open(path, Open::NEW)?;
    p.write_all(new, b"replacement").await?;
    p.close(new)?;
    p.write_all(writer, b"!").await?;
    p.close(writer)?;
    let mut old = [0; 9];
    if p.read(reader, &mut old[..3]).await? != 3 {
        return Err(Errno::Io);
    }
    p.close(reader)?;
    if p.read(shared, &mut old[3..]).await? != 6
        || &old != b"original!"
        || p.read(shared, &mut old).await? != 0
    {
        return Err(Errno::Io);
    }
    p.close(shared)?;
    let new = p.open(path, Open::READ)?;
    let mut fresh = [0; 11];
    if p.read(new, &mut fresh).await? != fresh.len() || &fresh != b"replacement" {
        return Err(Errno::Io);
    }
    p.close(new)?;
    p.unlink(path)?;
    p.write_all(1, b"unlink: ENOENT; old fd: ").await?;
    p.write_all(1, &old).await?;
    p.write_all(1, b"; new file: ").await?;
    p.write_all(1, &fresh).await?;
    p.write_all(1, format!("; pid {}\n", p.getpid()).as_bytes())
        .await
}

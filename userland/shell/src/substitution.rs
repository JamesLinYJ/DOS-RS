//! Run command substitutions in their own process and drain stdout before wait.
use super::{Shell, close, diagnostic, dup2, exit, fork, pipe, read, syntax::Pipeline, wait};
use alloc::{string::String, vec::Vec};

impl Shell {
    pub(super) fn capture(&mut self, body: &[Pipeline]) -> Result<String, ()> {
        let mut ends = [-1; 2];
        if unsafe { pipe(ends.as_mut_ptr()) } < 0 {
            diagnostic("cannot create command substitution pipe");
            return Err(());
        }
        let [reader, writer] = ends;
        let pid = unsafe { fork() };
        if pid == 0 {
            let _ = close(reader);
            if writer != 1 {
                if unsafe { dup2(writer, 1) } < 0 {
                    exit(1);
                }
                let _ = close(writer);
            }
            self.monitor = false;
            self.interactive = false;
            self.jobs.clear();
            self.background.clear();
            exit(if body.is_empty() {
                0
            } else {
                self.run_list(body)
            });
        }
        let _ = close(writer);
        if pid < 0 {
            let _ = close(reader);
            diagnostic("cannot fork command substitution");
            return Err(());
        }
        let output = drain(reader);
        let _ = close(reader);
        self.substitution_status =
            Some(wait(pid).map_err(|_| diagnostic("cannot wait for command substitution"))?);
        output.map_err(|e| diagnostic(&alloc::format!("command substitution read error {e}")))
    }
}

fn drain(fd: i32) -> Result<String, i32> {
    let mut buffer = [0; 4096];
    let mut data = Vec::new();
    loop {
        match read(fd, &mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                data.try_reserve(n).map_err(|_| 12)?;
                data.extend_from_slice(&buffer[..n]);
            }
            Err(4) => continue,
            Err(e) => return Err(e),
        }
    }
    while data.last() == Some(&b'\n') {
        data.pop();
    }
    String::from_utf8(data).map_err(|_| 84)
}

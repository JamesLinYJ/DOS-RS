#define _GNU_SOURCE
#include <dirent.h>
#include <signal.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <sys/ioctl.h>
#include <termios.h>
#include <time.h>
_Static_assert(sizeof(struct termios) == 60 && offsetof(struct termios, c_cc) == 17,
               "terminal wire ABI");
#include "ksigaction.h"
#include "pthread_impl.h"
#include <fcntl.h>
#include <stdarg.h>
#include <sys/mman.h>
#include <sys/uio.h>
#undef sys_open
#include "dos_calls.h"

_Noreturn void __restore(void)
{
    sys_sigreturn();
    a_crash();
    __builtin_unreachable();
}
#ifndef __restore
void __restore_rt(void) __attribute__((alias("__restore")));
#endif

int __set_thread_area(void *tp) { return sys_settls((long)tp); }

int __clone(int (*run)(void *), void *sp, int flags, void *arg, ...)
{
    va_list ap;
    va_start(ap, arg);
    int *ptid = va_arg(ap, int *);
    pthread_t td = va_arg(ap, void *);
    int *ctid = va_arg(ap, int *);
    va_end(ap);
    if (!(flags & CLONE_THREAD) || !(flags & CLONE_SETTLS))
        return -ENOSYS;
    uintptr_t rec[] = {(uintptr_t)td->stack - td->stack_size, (uintptr_t)sp, (uintptr_t)td,
                       (uintptr_t)ptid, (uintptr_t)ctid};
    return sys_pthreadspawn((long)run, (long)arg, (long)rec);
}

void __unmapself(void *p, size_t n)
{
    sys_threadfinish((long)p, n / 4096);
    a_crash();
}

/* Native wire time is two signed 64-bit words on both CPU families. */
static void get_time(int64_t out[2], long ptr, int wide)
{
    if (wide)
        memcpy(out, (void *)ptr, 16);
    else {
        out[0] = ((long *)ptr)[0];
        out[1] = ((long *)ptr)[1];
    }
}
static long put_time(long ptr, const int64_t in[2], int wide)
{
    if (wide)
        memcpy((void *)ptr, in, 16);
    else {
        if ((long)in[0] != in[0])
            return -EOVERFLOW;
        ((long *)ptr)[0] = in[0];
        ((long *)ptr)[1] = in[1];
    }
    return 0;
}
static long clock_read(long clk, long ptr, int resolution, int wide)
{
    int64_t time[2];
    long r = sys_clockread(clk, ptr ? (long)time : 0, resolution);
    return r || !ptr ? r : put_time(ptr, time, wide);
}
static long clock_sleep(long clk, long flags, long ptr, long rem, int wide)
{
    if (!ptr)
        return -EFAULT;
    if (flags & ~1)
        return -EINVAL;
    int64_t req[2], remain[2] = {0, 0};
    get_time(req, ptr, wide);
    long r = sys_clocksleep(clk | (flags ? 256 : 0), (long)req, rem ? (long)remain : 0);
    if (r == -EINTR && !flags && rem)
        put_time(rem, remain, wide);
    return r;
}
static long signal_action(long sig, struct k_sigaction *sa, struct k_sigaction *old)
{
    struct {
        uint64_t handler, restorer, mask;
        uint32_t flags, pad;
    } in = {0}, out;
    if (sa) {
        in.handler = (uintptr_t)sa->handler;
        in.restorer = (uintptr_t)__restore;
        in.flags = sa->flags;
#ifdef SA_RESTORER
        in.flags &= ~SA_RESTORER;
#endif
        memcpy(&in.mask, sa->mask, 8);
    }
    long r = sys_sigaction(sig, sa ? (long)&in : 0, old ? (long)&out : 0);
    if (!r && old) {
        old->handler = (void *)(uintptr_t)out.handler;
        old->flags = out.flags;
#ifdef SA_RESTORER
        old->restorer = (void *)(uintptr_t)out.restorer;
#endif
        memcpy(old->mask, &out.mask, 8);
    }
    return r;
}

long __dos_dispatch(long n, long a, long b, long c, long d, long e, long f)
{
    switch (n) {
    case SYS_getrandom:
        return sys_getrandom(a, b, c);
    case SYS_clock_gettime:
        return clock_read(a, b, 0, sizeof(long) == 8);
    case SYS_clock_getres:
        return clock_read(a, b, 1, sizeof(long) == 8);
    case SYS_nanosleep:
        return clock_sleep(CLOCK_REALTIME, 0, a, b, sizeof(long) == 8);
    case SYS_clock_nanosleep:
        return clock_sleep(a, b, c, d, sizeof(long) == 8);
#ifdef SYS_clock_gettime64
    case SYS_clock_gettime64:
        return clock_read(a, b, 0, 1);
    case SYS_clock_getres_time64:
        return clock_read(a, b, 1, 1);
    case SYS_clock_nanosleep_time64:
        return clock_sleep(a, b, c, d, 1);
#endif
    case SYS_rt_sigaction:
        return d == 8 ? signal_action(a, (void *)b, (void *)c) : -EINVAL;
    case SYS_rt_sigpending:
        return b == 8 ? sys_sigpending(a) : -EINVAL;
    case SYS_rt_sigsuspend:
        return b == 8 ? sys_sigsuspend(a) : -EINVAL;
    case SYS_tkill:
        return sys_tkill(a, b);
    case SYS_kill:
        return sys_kill(a, b);
    case SYS_ioctl:
        if (b == TIOCGPGRP || b == TIOCGSID) {
            long value = b == TIOCGPGRP ? sys_tcgetpgrp(a) : sys_tcgetsid(a);
            if (value >= 0)
                *(int *)c = (int)value;
            return value >= 0 ? 0 : value;
        }
        if (b == TIOCSPGRP)
            return sys_tcsetpgrp(a, *(int *)c);
        return sys_ttyio(a, b, c);
    case SYS_fcntl: {
        int ofd = b == F_OFD_GETLK || b == F_OFD_SETLK || b == F_OFD_SETLKW;
        if (!ofd && b != F_GETLK && b != F_SETLK && b != F_SETLKW)
            return sys_fcntl(a, b, c);
        if (!c)
            return -EFAULT;
        struct flock *lock = (struct flock *)c;
        if (ofd && lock->l_pid != 0)
            return -EINVAL;
        struct {
            int16_t kind, whence;
            uint32_t pad;
            int64_t start, len;
            int32_t pid;
            uint32_t reserved;
        } wire = {lock->l_type, lock->l_whence, 0, lock->l_start, lock->l_len, 0, 0};
        _Static_assert(sizeof(wire) == 32, "native record lock layout");
        long cmd = ofd ? b : b == F_GETLK ? 5 : b == F_SETLK ? 6 : 7;
        long result = sys_fcntl(a, cmd, (long)&wire);
        if (result == 0 && (b == F_GETLK || b == F_OFD_GETLK)) {
            lock->l_type = wire.kind;
            lock->l_whence = wire.whence;
            lock->l_start = wire.start;
            lock->l_len = wire.len;
            lock->l_pid = wire.pid;
        }
        return result;
    }
    case SYS_ftruncate:
    case SYS_truncate: {
#if __SIZEOF_LONG__ == 4
        int64_t len = (uint64_t)(uint32_t)c << 32 | (uint32_t)b;
#else
        int64_t len = b;
#endif
        return n == SYS_ftruncate ? sys_ftruncate(a, (long)&len) : sys_truncate(a, (long)&len);
    }
    case SYS_fdatasync:
    case SYS_fsync:
        return sys_fsync(a);
    case SYS_fchmod:
        return sys_fchmod(a, b);
    case SYS_fchown:
        return sys_fchown(a, b, c);
    case SYS_access:
        return sys_access(a, b);
    case SYS_fchdir:
        return sys_fchdir(a);
    case SYS_faccessat:
    case SYS_faccessat2: {
        uintptr_t options[] = {c, n == SYS_faccessat2 ? d : 0};
        return sys_faccessat(a, b, (long)options);
    }
    case SYS_fchmodat:
    case SYS_fchmodat2: {
        uintptr_t options[] = {c, n == SYS_fchmodat2 ? d : 0};
        return sys_fchmodat(a, b, (long)options);
    }
    case SYS_fchownat: {
        uintptr_t options[] = {c, d, e};
        return sys_fchownat(a, b, (long)options);
    }
    case SYS_utimensat:
#if defined(SYS_utimensat_time64) && SYS_utimensat_time64 != SYS_utimensat
    case SYS_utimensat_time64:
#endif
    {
        int wide = sizeof(long) == 8;
#ifdef SYS_utimensat_time64
        if (n == SYS_utimensat_time64)
            wide = 1;
#endif
        int64_t times[4];
        if (c) {
            get_time(times, c, wide);
            get_time(times + 2, c + (wide ? 16 : 2 * sizeof(long)), wide);
        }
        uintptr_t ptr = c ? (uintptr_t)times : 0;
        if (!b)
            return d ? -EINVAL : sys_futimens(a, ptr);
        uintptr_t options[] = {ptr, d};
        return sys_utimensat(a, b, (long)options);
    }
    case SYS_pread:
    case SYS_pwrite: {
#if __SIZEOF_LONG__ == 4
        uint64_t offset = (uint64_t)(uint32_t)e << 32 | (uint32_t)d;
#else
        uint64_t offset = d;
#endif
        uint64_t options[] = {offset, (unsigned long)c};
        return n == SYS_pread ? sys_pread(a, b, (long)options) : sys_pwrite(a, b, (long)options);
    }
    case SYS_mkdirat:
        return sys_mkdirat(a, b, c);
    case SYS_renameat: {
        uintptr_t to[] = {c, d};
        return sys_renameat(a, b, (long)to);
    }
    case SYS_linkat: {
        uintptr_t to[] = {c, d, e};
        return sys_linkat(a, b, (long)to);
    }
    case SYS_readlinkat: {
        uintptr_t out[] = {c, d};
        return sys_readlinkat(a, b, (long)out);
    }
    case SYS_symlinkat:
        return sys_symlinkat(a, b, c);
    case SYS_setuid:
        return sys_setuid(a);
    case SYS_setgid:
        return sys_setgid(a);
    case SYS_chown:
        return sys_chown(a, b, c);
    case SYS_poll:
        return sys_poll(a, b, c);
    case SYS_ppoll:
#if defined(SYS_ppoll_time64) && SYS_ppoll_time64 != SYS_ppoll
    case SYS_ppoll_time64:
#endif
    {
        if (d && e != 8)
            return -EINVAL;
        int wide = sizeof(long) == 8;
#ifdef SYS_ppoll_time64
        if (n == SYS_ppoll_time64)
            wide = 1;
#endif
        int64_t timeout[2];
        if (c)
            get_time(timeout, c, wide);
        const uintptr_t options[2] = {c ? (uintptr_t)timeout : 0, (uintptr_t)d};
        return sys_ppoll(a, b, (long)options);
    }
    case SYS_fork:
        return sys_fork();
    case SYS_execve:
        return sys_execve(a, b, c);
    case SYS_wait4:
        return d ? -ENOTSUP : sys_waitpid(a, b, c);
    case SYS_getppid:
        return sys_getppid();
    case SYS_getpgid:
        return sys_getpgid(a);
    case SYS_setpgid:
        return sys_setpgid(a, b);
    case SYS_getsid:
        return sys_getsid(a);
    case SYS_setsid:
        return sys_setsid();
    case SYS_chdir:
        return sys_chdir(a);
    case SYS_getcwd:
        return sys_getcwd(a, b);
    case SYS_mkdir:
        return sys_mkdir(a, b);
    case SYS_rmdir:
        return sys_rmdir(a);
    case SYS_link:
        return sys_link(a, b);
    case SYS_symlink:
        return sys_symlink(a, b);
    case SYS_readlink:
        return sys_readlink(a, b, c);
    case SYS_chmod:
        return sys_chmod(a, b);
    case SYS_umask:
        return sys_umask(a);
    case SYS_rename:
        return sys_rename(a, b);
    case SYS_getdents: {
        unsigned char wire[8 + 1 + NAME_MAX + 1];
        struct dirent *entry = (void *)b;
        if ((size_t)c < offsetof(struct dirent, d_name) + 1)
            return -EINVAL;
        size_t available = (size_t)c - offsetof(struct dirent, d_name) + 9;
        long r = sys_getdents(a, (long)wire, available < sizeof wire ? available : sizeof wire);
        if (r <= 0)
            return r;
        if (r < 10 || r > (long)sizeof wire || wire[r - 1])
            return -EIO;
        memcpy(&entry->d_ino, wire, 8);
        entry->d_type = wire[8];
        size_t len = offsetof(struct dirent, d_name) + (size_t)r - 9;
        entry->d_reclen = len;
        int64_t off = 0;
        long pos = sys_lseek(a, (long)&off, 1);
        entry->d_off = pos == 0 ? off : 0;
        memcpy(entry->d_name, wire + 9, r - 9);
        return len;
    }
    case SYS_getpid:
        return sys_getpid();
    case SYS_gettid:
        return sys_gettid();
    case SYS_exit_group:
        return sys_exit(a);
    case SYS_exit:
        return sys_threadfinish(0, 0);
    case SYS_set_tid_address:
        return sys_settidx(a);
    case SYS_rt_sigprocmask:
        return d == 8 ? sys_sigmask(a, b, c) : -EINVAL;
    case SYS_read:
        return sys_read(a, b, c);
    case SYS_write:
        return sys_write(a, b, c);
    case SYS_writev:
        return sys_writev(a, b, c);
    case SYS_readv:
        return sys_readv(a, b, c);
    case SYS_close:
        return sys_close(a);
    case SYS_pipe:
        return sys_pipe(a);
    case SYS_pipe2:
        return sys_pipe2(a, b);
    case SYS_dup3:
        return sys_dup3(a, b, c);
    case SYS_dup:
        return sys_dup(a);
    case SYS_dup2:
        return sys_dup2(a, b);
    case SYS_unlink:
        return sys_unlink(a);
    case SYS_unlinkat:
        return sys_unlinkat(a, b, c);
    case SYS_open:
        return sys_openmode(a, b & ~O_LARGEFILE, c);
    case SYS_openat: {
        uint32_t options[] = {(uint32_t)c & ~O_LARGEFILE, (uint32_t)d};
        return sys_openat(a, b, (long)options);
    }
    case SYS_getuid:
    case SYS_geteuid:
        return sys_getuid();
    case SYS_getgid:
    case SYS_getegid:
        return sys_getgid();
#if defined(SYS_getuid32) && SYS_getuid32 != SYS_getuid
    case SYS_getuid32:
    case SYS_geteuid32:
        return sys_getuid();
    case SYS_getgid32:
    case SYS_getegid32:
        return sys_getgid();
#endif
    case SYS_lseek: {
        int64_t pos = b;
        long r = sys_lseek(a, (long)&pos, c);
        if (r < 0)
            return r;
        return (long)pos == pos ? (long)pos : -EOVERFLOW;
    }
#ifdef SYS__llseek
    case SYS__llseek: {
        int64_t pos = (uint64_t)(uint32_t)b << 32 | (uint32_t)c;
        long r = sys_lseek(a, (long)&pos, e);
        if (!r)
            *(int64_t *)d = pos;
        return r;
    }
#endif
#ifdef SYS_mmap2
    case SYS_mmap2:
#else
    case SYS_mmap:
#endif
        if (a || d != (MAP_PRIVATE | MAP_ANON) || e != -1 || f)
            return -ENOTSUP;
        if (!b || (unsigned long)b > -4096UL)
            return -EINVAL;
        return sys_mapanon(((unsigned long)b + 4095) / 4096, c);
    case SYS_mprotect:
    case SYS_munmap:
        if (a % 4096 || !b || (unsigned long)b > -4096UL)
            return -EINVAL;
        return n == SYS_munmap ? sys_unmapmem(a, ((unsigned long)b + 4095) / 4096)
                               : sys_protectmem(a, ((unsigned long)b + 4095) / 4096, c);
    case SYS_futex:
#if defined(SYS_futex_time64) && SYS_futex_time64 != SYS_futex
    case SYS_futex_time64:
#endif
        switch (b & ~FUTEX_PRIVATE) {
        case FUTEX_WAIT: {
            if (!d)
                return sys_futexwait(a, c);
            int wide = sizeof(long) == 8;
#ifdef SYS_futex_time64
            if (n == SYS_futex_time64)
                wide = 1;
#endif
            int64_t timeout[2];
            get_time(timeout, d, wide);
            return sys_futextimed(a, c, (long)timeout);
        }
        case FUTEX_WAKE:
            return sys_futexwake(a, c);
        case FUTEX_REQUEUE: {
            if (c < 0 || d < 0)
                return -EINVAL;
            uint32_t limits[] = {(uint32_t)c, (uint32_t)d};
            return sys_futexmove(a, e, (long)limits);
        }
        default:
            return -ENOSYS;
        }
    default:
        return -ENOSYS;
    }
}

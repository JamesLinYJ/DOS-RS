#include <errno.h>
#include <unistd.h>
#include <fcntl.h>
#include <dos_thread.h>
#include <dos_memory.h>

extern long sys_read(int, void *, size_t), sys_write(int, const void *, size_t);
extern long sys_open(const char *, int), sys_unlink(const char *), sys_close(int);
extern long sys_dup(int), sys_dup2(int, int), sys_pipe(int *), sys_getpid(void);
extern long sys_lseek(int, off_t *, int);
extern _Noreturn void sys_exit(int);
extern long sys_getuid(void), sys_getgid(void), sys_gettid(void), sys_threrrno(void);
extern long sys_thrspawn(void (*)(void *), void *, const size_t *);
extern long sys_thrjoin(int);
extern _Noreturn void sys_threxit(int);
extern long sys_futexwait(uint32_t *, uint32_t), sys_futexwake(uint32_t *, unsigned int);
extern long sys_mapmem(size_t), sys_unmapmem(void *, size_t), sys_meminfo(struct dos_meminfo *);
int *__dos_errno_location(void) {
    static int initial;
    long address = sys_threrrno();
    return address > 0 ? (int *)address : &initial;
}
static long result(long value) {
    if (value < 0) {
        errno = (int)-value;
        return -1;
    }
    return value;
}
ssize_t read(int fd, void *p, size_t n) { return result(sys_read(fd, p, n)); }
ssize_t write(int fd, const void *p, size_t n) { return result(sys_write(fd, p, n)); }
int open(const char *p, int flags, ...) { return (int)result(sys_open(p, flags)); }
int unlink(const char *p) { return (int)result(sys_unlink(p)); }
int close(int fd) { return (int)result(sys_close(fd)); }
int dup(int fd) { return (int)result(sys_dup(fd)); }
int dup2(int fd, int dest) { return (int)result(sys_dup2(fd, dest)); }
int pipe(int fds[2]) { return (int)result(sys_pipe(fds)); }
int getpid(void) { return (int)result(sys_getpid()); }
unsigned int getuid(void) { return (unsigned int)sys_getuid(); }
unsigned int getgid(void) { return (unsigned int)sys_getgid(); }
off_t lseek(int fd, off_t offset, int whence) {
    return result(sys_lseek(fd, &offset, whence)) < 0 ? -1 : offset;
}
_Noreturn void _exit(int status) { sys_exit(status); }

struct start_record { int (*function)(void *); void *argument; };
static void run_thread(void *record) {
    struct start_record start = *(struct start_record *)record;
    dos_thread_exit(start.function(start.argument));
}
int dos_thread_create(int (*fn)(void *), void *arg, void *stack, size_t size) {
    if (!fn || !stack || ((uintptr_t)stack & 15) || size < 4096 ||
        size > UINTPTR_MAX - (uintptr_t)stack) {
        errno = EINVAL;
        return -1;
    }
    /* Pass the start record from the caller's stack. The kernel copies its two
     * words to the new stack before it publishes a runnable thread. */
    struct start_record start = {fn, arg};
    const size_t range[3] = {(uintptr_t)stack, size, sizeof start};
    return (int)result(sys_thrspawn(run_thread, &start, range));
}
int dos_thread_join(int tid, int *status) {
    long value = sys_thrjoin(tid);
    if (value < 0) return (int)result(value);
    if (status) *status = (int)value;
    return 0;
}
_Noreturn void dos_thread_exit(int status) { sys_threxit(status); }
int dos_gettid(void) { return (int)sys_gettid(); }
int dos_futex_wait(uint32_t *p, uint32_t v) { return (int)result(sys_futexwait(p, v)); }
int dos_futex_wake(uint32_t *p, unsigned int n) { return (int)result(sys_futexwake(p, n)); }
void *dos_map_pages(size_t pages) {
    long address = result(sys_mapmem(pages));
    return address < 0 ? NULL : (void *)address;
}
int dos_unmap_pages(void *base, size_t pages) { return (int)result(sys_unmapmem(base, pages)); }
int dos_meminfo(struct dos_meminfo *info) { return (int)result(sys_meminfo(info)); }

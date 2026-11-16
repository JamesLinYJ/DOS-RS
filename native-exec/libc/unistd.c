#include <errno.h>
#include <unistd.h>
#include <fcntl.h>

int errno;
extern long sys_read(int, void *, size_t), sys_write(int, const void *, size_t);
extern long sys_open(const char *, int), sys_unlink(const char *), sys_close(int);
extern long sys_dup(int), sys_dup2(int, int), sys_pipe(int *), sys_getpid(void);
extern long sys_lseek(int, off_t *, int);
extern _Noreturn void sys_exit(int);
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
off_t lseek(int fd, off_t offset, int whence) {
    return result(sys_lseek(fd, &offset, whence)) < 0 ? -1 : offset;
}
_Noreturn void _exit(int status) { sys_exit(status); }

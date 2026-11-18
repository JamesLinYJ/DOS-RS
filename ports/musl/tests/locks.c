#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/wait.h>
#include <unistd.h>

/* Owner IDs and >32-bit byte ranges must round trip on both ELF ABIs. */
int main(void)
{
    int fd = open("musl-lock", O_CREAT | O_EXCL | O_RDWR, 0600);
    if (fd < 0)
        return 1;
    struct flock lock = {
        .l_type = F_WRLCK, .l_whence = SEEK_SET, .l_start = 0x100000042LL, .l_len = 37};
    if (fcntl(fd, F_SETLK, &lock))
        return 2;
    pid_t owner = getpid(), pid = fork();
    if (!pid) {
        if (fcntl(fd, F_GETLK, &lock) || lock.l_type != F_WRLCK || lock.l_pid != owner ||
            lock.l_start != 0x100000042LL || lock.l_len != 37)
            _exit(3);
        if (fcntl(fd, F_SETLK, &lock) != -1 || (errno != EAGAIN && errno != EACCES))
            _exit(4);
        _exit(0);
    }
    int status;
    if (pid < 0 || waitpid(pid, &status, 0) != pid || status != 0)
        return 5;
    lock.l_type = F_UNLCK;
    if (fcntl(fd, F_SETLKW, &lock))
        return 7;
    int peer = open("musl-lock", O_RDWR), extra = dup(fd);
    if (peer < 0 || extra < 0)
        return 8;
    lock.l_type = F_WRLCK;
    lock.l_pid = 0;
    if (fcntl(fd, F_OFD_SETLK, &lock) || close(fd) || fcntl(peer, F_OFD_GETLK, &lock) ||
        lock.l_type != F_WRLCK || lock.l_pid != -1 || lock.l_start != 0x100000042LL)
        return 9;
    lock.l_pid = 0;
    if (fcntl(peer, F_OFD_SETLK, &lock) != -1 || errno != EAGAIN || close(extra) ||
        fcntl(peer, F_OFD_SETLKW, &lock))
        return 10;
    if (close(peer) || unlink("musl-lock"))
        return 6;
    int channel[2];
    if (pipe2(channel, O_CLOFORK | O_CLOEXEC | O_NONBLOCK) ||
        fcntl(channel[0], F_GETFD) != (FD_CLOFORK | FD_CLOEXEC))
        return 11;
    int inherited = dup(channel[1]), forkclosed = fcntl(channel[1], F_DUPFD_CLOFORK, 8);
    if (inherited < 0 || forkclosed < 8 || dup3(channel[1], 14, O_CLOFORK) != 14)
        return 12;
    pid = fork();
    if (!pid) {
        if (fcntl(channel[0], F_GETFD) != -1 || errno != EBADF ||
            fcntl(forkclosed, F_GETFD) != -1 || errno != EBADF || fcntl(14, F_GETFD) != -1 ||
            errno != EBADF || write(inherited, "A", 1) != 1)
            _exit(13);
        _exit(0);
    }
    char value;
    if (pid < 0 || waitpid(pid, &status, 0) != pid || status || read(channel[0], &value, 1) != 1 ||
        value != 'A')
        return 14;
    puts("musl fcntl process locks and 64-bit ranges OK");
    return 0;
}

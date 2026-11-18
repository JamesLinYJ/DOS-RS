#include <errno.h>
#include <fcntl.h>
#include <sys/wait.h>
#include <unistd.h>

#define CHECK(n, expr)                                                                             \
    do {                                                                                           \
        if (!(expr)) {                                                                             \
            const char msg[] = "record lock check failed " #n "\n";                                \
            write(2, msg, sizeof msg - 1);                                                         \
            return n;                                                                              \
        }                                                                                          \
    } while (0)
static struct flock range(int kind, off_t pos, off_t len) {
    return (struct flock){.l_type = kind, .l_whence = SEEK_SET, .l_start = pos, .l_len = len};
}

int main(void) {
    int fd = open("/record", O_CREAT | O_EXCL | O_RDWR, 0600);
    CHECK(1, fd >= 0 && link("/record", "/record-alias") == 0);
    int second = open("/record-alias", O_RDWR), rd = open("/record", O_RDONLY);
    CHECK(2, second >= 0 && rd >= 0 && write(fd, "advisory", 8) == 8);
    struct flock l = range(F_WRLCK, 30, -20);
    CHECK(3, fcntl(rd, F_SETLK, &l) < 0 && errno == EBADF && fcntl(fd, F_SETLK, &l) == 0);
    l = range(F_UNLCK, 15, 5);
    CHECK(4, fcntl(fd, F_SETLK, &l) == 0);
    int note[2], go[2];
    CHECK(5, pipe(note) == 0 && pipe(go) == 0);
    int owner = getpid(), kid = fork();
    CHECK(6, kid >= 0);
    char ch;
    if (kid == 0) {
        close(note[0]);
        close(go[1]);
        struct flock q = range(F_RDLCK, 10, 1);
        CHECK(7, fcntl(second, F_GETLK, &q) == 0 && q.l_type == F_WRLCK && q.l_pid == owner &&
                     q.l_start == 10 && q.l_len == 5);
        q = range(F_WRLCK, 20, 0);
        CHECK(8, fcntl(second, F_GETLK, &q) == 0 && q.l_start == 20 && q.l_len == 10);
        q = range(F_WRLCK, 16, 1);
        CHECK(9, fcntl(second, F_GETLK, &q) == 0 && q.l_type == F_UNLCK);
        q = range(F_RDLCK, 11, 1);
        CHECK(10, fcntl(second, F_SETLK, &q) < 0 && (errno == EAGAIN || errno == EACCES));
        CHECK(11, pwrite(second, "x", 1, 11) == 1 && write(note[1], "r", 1) == 1 &&
                      read(go[0], &ch, 1) == 1);
        CHECK(12, fcntl(second, F_SETLKW, &q) == 0 && write(note[1], "g", 1) == 1 &&
                      read(go[0], &ch, 1) == 1);
        _exit(0);
    }
    close(note[1]);
    close(go[0]);
    CHECK(13, read(note[0], &ch, 1) == 1 && ch == 'r' && write(go[1], "w", 1) == 1);
    CHECK(14, close(rd) == 0 && read(note[0], &ch, 1) == 1 && ch == 'g');
    l = range(F_WRLCK, 11, 1);
    CHECK(15, fcntl(fd, F_GETLK, &l) == 0 && l.l_type == F_RDLCK && l.l_pid == kid);
    CHECK(16, write(go[1], "e", 1) == 1);
    int status;
    CHECK(17, waitpid(kid, &status, 0) == kid && WIFEXITED(status) && WEXITSTATUS(status) == 0);
    l = range(F_WRLCK, 0, 0);
    CHECK(18, fcntl(fd, F_GETLK, &l) == 0 && l.l_type == F_UNLCK);
    CHECK(19, fcntl(second, F_SETFD, FD_CLOEXEC) == 0 && dup2(fd, second) == second &&
                  fcntl(second, F_GETFD) == 0);
    int other = open("/record-alias", O_RDWR);
    CHECK(22, other >= 0);
    l = range(F_WRLCK, 0x100000000LL, 42);
    CHECK(23, fcntl(fd, F_OFD_SETLK, &l) == 0);
    struct flock probe = range(F_RDLCK, 0x100000001LL, 1);
    CHECK(24, fcntl(other, F_OFD_GETLK, &probe) == 0 && probe.l_type == F_WRLCK &&
                  probe.l_pid == -1 && probe.l_start == 0x100000000LL);
    CHECK(25, fcntl(other, F_OFD_SETLK, &probe) < 0 && errno == EINVAL);
    probe = range(F_RDLCK, 0x100000001LL, 1);
    CHECK(26, fcntl(fd, F_GETLK, &probe) == 0 && probe.l_pid == -1);
    int dup_ofd = dup(fd);
    CHECK(27, dup_ofd >= 0 && close(dup_ofd) == 0);
    probe = range(F_WRLCK, 0x100000001LL, 1);
    CHECK(28, fcntl(second, F_OFD_GETLK, &probe) == 0 && probe.l_type == F_UNLCK);
    int isolate = open("/record", O_CLOFORK | O_RDONLY);
    int d = fcntl(fd, F_DUPFD_CLOFORK, 11), endpoints[2];
    CHECK(29, isolate >= 0 && d >= 11 && fcntl(isolate, F_GETFD) == FD_CLOFORK &&
                  fcntl(d, F_GETFD) == FD_CLOFORK);
    CHECK(30, pipe2(endpoints, O_NONBLOCK | O_CLOEXEC | O_CLOFORK) == 0 &&
                  fcntl(endpoints[0], F_GETFD) == (FD_CLOEXEC | FD_CLOFORK));
    CHECK(31, read(endpoints[0], &ch, 1) < 0 && errno == EAGAIN);
    CHECK(32, dup3(fd, 25, O_CLOFORK | O_CLOEXEC) == 25 && dup3(fd, fd, 0) < 0 && errno == EINVAL);
    kid = fork();
    CHECK(33, kid >= 0);
    if (kid == 0) {
        CHECK(34, fcntl(isolate, F_GETFD) < 0 && errno == EBADF && fcntl(25, F_GETFD) < 0 &&
                      errno == EBADF);
        CHECK(35, fcntl(d, F_GETFD) < 0 && errno == EBADF && fcntl(endpoints[1], F_GETFD) < 0 &&
                      errno == EBADF);
        struct flock shared = range(F_RDLCK, 0x100000001LL, 1);
        CHECK(36, fcntl(fd, F_OFD_GETLK, &shared) == 0 && shared.l_type == F_UNLCK);
        shared = range(F_RDLCK, 0x100000001LL, 1);
        CHECK(40, fcntl(other, F_OFD_SETLK, &shared) < 0 && errno == EAGAIN);
        _exit(0);
    }
    CHECK(37, waitpid(kid, &status, 0) == kid && status == 0 &&
                  fcntl(25, F_GETFD) == (FD_CLOFORK | FD_CLOEXEC));
    CHECK(38, close(d) == 0 && close(25) == 0 && close(isolate) == 0 && close(endpoints[0]) == 0 &&
                  close(endpoints[1]) == 0);
    CHECK(20, close(go[1]) == 0 && close(note[0]) == 0 && close(fd) == 0 && close(second) == 0);
    probe = range(F_WRLCK, 0x100000001LL, 1);
    CHECK(39,
          fcntl(other, F_OFD_GETLK, &probe) == 0 && probe.l_type == F_UNLCK && close(other) == 0);
    CHECK(21, unlink("/record") == 0 && unlink("/record-alias") == 0);
    const char msg[] = "advisory lock ranges ownership and wakeup OK\n";
    return write(1, msg, sizeof msg - 1) != sizeof msg - 1;
}

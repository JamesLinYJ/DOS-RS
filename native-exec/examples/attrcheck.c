// Exercise public libc entry points, including the 64-bit fields on i686.
#include <errno.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

#define CHECK(n, expr)                                                                             \
    do {                                                                                           \
        if (!(expr)) {                                                                             \
            const char msg[] = "attribute check failed " #n "\n";                                  \
            write(2, msg, sizeof msg - 1);                                                         \
            return n;                                                                              \
        }                                                                                          \
    } while (0)

int main(void) {
    char buf[40];
    struct stat st;
    struct timespec epoch[] = {{-123456789, 123}, {4567890123LL, 456}};
    struct timespec omit[] = {{0, UTIME_OMIT}, {42, 123456789}};
    struct timespec skip[] = {{0, UTIME_OMIT}, {0, UTIME_OMIT}};
    struct timespec invalid[] = {{0, 1000000000}, {0, 0}};
    int root = open("/", O_RDONLY | O_DIRECTORY);
    CHECK(1, root >= 0 && mkdirat(root, "attrs", 0750) == 0);
    int dir = openat(root, "attrs", O_RDONLY | O_DIRECTORY);
    CHECK(2, dir >= 0 && fchdir(dir) == 0 && getcwd(buf, sizeof buf) && buf[0] == '/' &&
                 buf[1] == 'a' && buf[6] == 0);
    int fd = open("file", O_RDWR | O_CREAT | O_EXCL | O_APPEND, 0640);
    CHECK(3, fd >= 0 && symlinkat("file", dir, "sym") == 0);
    CHECK(4, write(fd, "abc", 3) == 3 && pwrite(fd, "X", 1, 1) == 1 && lseek(fd, 0, SEEK_CUR) == 3);
    CHECK(5, pread(fd, buf, 3, 0) == 3 && buf[0] == 'a' && buf[1] == 'X' && buf[2] == 'c' &&
                 lseek(fd, 0, SEEK_CUR) == 3);
    CHECK(6, pread(fd, buf, 1, -1) < 0 && errno == EINVAL && pwrite(fd, "", 0, 0) == 0);
    CHECK(7, utimensat(dir, "sym", epoch, AT_SYMLINK_NOFOLLOW) == 0 && lstat("sym", &st) == 0 &&
                 st.st_atim.tv_sec == -123456789 && st.st_mtim.tv_sec == 4567890123LL &&
                 st.st_mtim.tv_nsec == 456);
    CHECK(8, futimens(fd, epoch) == 0 && utimensat(dir, "file", omit, 0) == 0 &&
                 fstat(fd, &st) == 0 && st.st_atim.tv_sec == -123456789 &&
                 st.st_atim.tv_nsec == 123 && st.st_mtim.tv_sec == 42 &&
                 st.st_mtim.tv_nsec == 123456789);
    CHECK(9, futimens(fd, invalid) < 0 && errno == EINVAL && futimens(fd, skip) == 0 &&
                 fstat(fd, &st) == 0 && st.st_mtim.tv_sec == 42);
    CHECK(10, fchownat(dir, "sym", 321, 654, AT_SYMLINK_NOFOLLOW) == 0 &&
                  fchmodat(dir, "sym", 0604, 0) == 0);
    CHECK(11, lstat("sym", &st) == 0 && st.st_uid == 321 && fstat(fd, &st) == 0 && st.st_uid == 0 &&
                  (st.st_mode & 0777) == 0604);
    CHECK(12, faccessat(dir, "file", R_OK | W_OK, AT_EACCESS) == 0 &&
                  faccessat(dir, "file", X_OK, 0) < 0 && errno == EACCES);
    CHECK(13, fchdir(fd) < 0 && errno == ENOTDIR && fchdir(AT_FDCWD) < 0 && errno == EBADF);
    int pair[2];
    CHECK(14, pipe(pair) == 0 && pread(pair[0], buf, 1, 0) < 0 && errno == ESPIPE &&
                  close(pair[0]) == 0 && close(pair[1]) == 0);
    CHECK(15, fchdir(root) == 0 && close(fd) == 0 && close(dir) == 0);
    CHECK(16, mkdir("/gone", 0700) == 0 && mkdir("/replacement", 0700) == 0);
    int old = open("/gone", O_RDONLY | O_DIRECTORY);
    CHECK(17, old >= 0 && fchdir(old) == 0 && fstat(old, &st) == 0);
    uint64_t oldino = st.st_ino;
    CHECK(18, rename("/replacement", "/gone") == 0 && fstat(old, &st) == 0 && st.st_nlink == 0 &&
                  st.st_ino == oldino);
    CHECK(19, close(old) == 0 && stat(".", &st) == 0 && st.st_nlink == 0 && st.st_ino == oldino);
    CHECK(20, getcwd(buf, sizeof buf) == 0 && errno == ENOENT && mkdir("lost", 0700) < 0 &&
                  errno == ENOENT);
    CHECK(21, fchdir(root) == 0 && stat("/gone", &st) == 0 && st.st_ino != oldino &&
                  rmdir("/gone") == 0);
    CHECK(22, mkdir("newgone", 0700) == 0 && chdir("newgone") == 0 &&
                  unlinkat(root, "newgone", AT_REMOVEDIR) == 0);
    CHECK(23, open("lost", O_WRONLY | O_CREAT, 0600) < 0 && errno == ENOENT &&
                  stat(".", &st) == 0 && st.st_nlink == 0);
    CHECK(24, fchdir(root) == 0 && close(root) == 0);
    const char msg[] = "file times and positioned I/O OK\n";
    return write(1, msg, sizeof msg - 1) != sizeof msg - 1;
}

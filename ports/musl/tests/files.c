#define _POSIX_C_SOURCE 200809L
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <unistd.h>

int main(void)
{
    char cwd[256], dir[64], target[32], buf[16] = {0};
    struct stat before, after;
    if (!getcwd(cwd, sizeof cwd))
        return 1;
    snprintf(dir, sizeof dir, "/musl-%d", getpid());
    if (mkdir(dir, 0750) || chdir(dir))
        return 2;
    mode_t mask = umask(0027);
    int fd = open("old", O_CREAT | O_EXCL | O_RDWR, 0666);
    umask(mask);
    if (fd < 0 || write(fd, "old bytes", 9) != 9 || fstat(fd, &before) ||
        (before.st_mode & 0777) != 0640 || before.st_size != 9)
        return 3;
    if (link("old", "hard") || symlink("hard", "symbolic") || chmod("hard", 0600))
        return 4;
    if (stat("symbolic", &after) || after.st_ino != before.st_ino || after.st_nlink != 2 ||
        (after.st_mode & 0777) != 0600 || lstat("symbolic", &after) || !S_ISLNK(after.st_mode) ||
        readlink("symbolic", target, sizeof target) != 4 || memcmp(target, "hard", 4))
        return 5;
    DIR *d = opendir(".");
    if (!d)
        return 6;
    int names = 0;
    struct dirent *ent;
    errno = 0;
    while ((ent = readdir(d))) {
        if (!strcmp(ent->d_name, "old"))
            names |= 1;
        if (!strcmp(ent->d_name, "hard"))
            names |= 2;
        if (!strcmp(ent->d_name, "symbolic"))
            names |= 4;
    }
    if (errno || closedir(d) || names != 7)
        return 7;
    if (unlink("old") || unlink("hard") || fstat(fd, &after) || after.st_nlink != 0 ||
        rename("symbolic", "alias"))
        return 8;
    int other = open("old", O_CREAT | O_EXCL | O_RDWR, 0600);
    if (other < 0 || fstat(other, &after) || after.st_ino == before.st_ino ||
        lseek(fd, 0, SEEK_SET) || read(fd, buf, sizeof buf) != 9 || memcmp(buf, "old bytes", 9) ||
        close(other) || close(fd))
        return 9;
    if (unlink("old") || unlink("alias"))
        return 10;
    int dirfd = open(".", O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if (dirfd < 0 || !(fcntl(dirfd, F_GETFD) & FD_CLOEXEC))
        return 11;
    int relative = openat(dirfd, "relative", O_CREAT | O_EXCL | O_RDWR, 0644);
    if (relative < 0 || write(relative, "abcdef", 6) != 6 || ftruncate(relative, 3) ||
        fsync(relative) || fchmod(relative, 0600) || fstatat(dirfd, "relative", &after, 0) ||
        after.st_size != 3 || (after.st_mode & 0777) != 0600)
        return 12;
    struct statvfs root, volume, dos;
    if (statvfs(".", &root) || fstatvfs(relative, &volume) || statvfs("/dos/c/", &dos) ||
        root.f_fsid != volume.f_fsid || root.f_fsid == dos.f_fsid || root.f_namemax != 255 ||
        dos.f_namemax != 12 || root.f_bavail == 0 || root.f_bavail > root.f_bfree ||
        root.f_bfree > root.f_blocks || !(root.f_flag & ST_NOSUID) || (dos.f_flag & ST_RDONLY) ||
        fstatvfs(-1, &volume) != -1 || errno != EBADF || statvfs("absent", &volume) != -1 ||
        errno != ENOENT)
        return 21;
    int copy = fcntl(relative, F_DUPFD, 8);
    if (copy < 8 || lseek(copy, 0, SEEK_SET) || read(relative, buf, 3) != 3 ||
        memcmp(buf, "abc", 3) || close(copy))
        return 13;
    if (fchdir(dirfd) || faccessat(dirfd, "relative", R_OK | W_OK, 0) ||
        fchmodat(dirfd, "relative", 0644, 0) || fchownat(dirfd, "relative", getuid(), getgid(), 0))
        return 16;
    if (fcntl(relative, F_SETFL, fcntl(relative, F_GETFL) | O_APPEND) ||
        pwrite(relative, "X", 1, 1) != 1 || lseek(relative, 0, SEEK_CUR) != 3 ||
        pread(relative, buf, 3, 0) != 3 || memcmp(buf, "aXc", 3) ||
        lseek(relative, 0, SEEK_CUR) != 3)
        return 17;
    const struct timespec times[2] = {{2200000000LL, 7654321}, {2200000001LL, 123456789}};
    if (futimens(relative, times) || fstatat(dirfd, "relative", &after, 0) ||
        after.st_atim.tv_sec != times[0].tv_sec || after.st_atim.tv_nsec != times[0].tv_nsec ||
        after.st_mtim.tv_sec != times[1].tv_sec || after.st_mtim.tv_nsec != times[1].tv_nsec)
        return 18;
    const struct timespec modify[2] = {{0, UTIME_OMIT}, {2200000002LL, 321}};
    if (utimensat(dirfd, "relative", modify, 0) || fstat(relative, &after) ||
        after.st_atim.tv_sec != times[0].tv_sec || after.st_mtim.tv_sec != modify[1].tv_sec ||
        after.st_mtim.tv_nsec != 321)
        return 19;
    if (mkdirat(dirfd, "sub", 0700) || linkat(dirfd, "relative", dirfd, "sub/hard", 0) ||
        symlinkat("relative", dirfd, "sym") ||
        readlinkat(dirfd, "sym", target, sizeof target) != 8 || memcmp(target, "relative", 8) ||
        fstatat(dirfd, "sym", &after, AT_SYMLINK_NOFOLLOW) || !S_ISLNK(after.st_mode) ||
        renameat(dirfd, "relative", dirfd, "renamed") || truncate("renamed", 0) ||
        fstat(relative, &after) || after.st_size || access("renamed", R_OK | W_OK))
        return 14;
    if (unlinkat(dirfd, "sub/hard", 0) || unlinkat(dirfd, "sub", AT_REMOVEDIR) ||
        unlinkat(dirfd, "sym", 0) || unlinkat(dirfd, "renamed", 0) || close(relative) ||
        close(dirfd) || chdir(cwd) || rmdir(dir))
        return 15;
    puts("musl Btrfs directory enumeration permissions links and unlink OK");
    return 0;
}

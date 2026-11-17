#define _POSIX_C_SOURCE 200809L
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
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
    if (unlink("old") || unlink("alias") || chdir(cwd) || rmdir(dir))
        return 10;
    puts("musl Btrfs directory enumeration permissions links and unlink OK");
    return 0;
}

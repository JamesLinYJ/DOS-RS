// Exercise libc directory operations through the same installed ELF on FAT and Btrfs.
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define CHECK(x) do { if (!(x)) { fprintf(stderr, "dirprobe line %d errno %d\n", __LINE__, errno); return 1; } } while (0)

int main(void)
{
    char cwd[128], c = 0;
    struct stat st, root_stat;
    int root = open(".", O_RDONLY | O_DIRECTORY);
    CHECK(root >= 0 && fstat(root, &root_stat) == 0 && S_ISDIR(root_stat.st_mode));
    int sub = openat(root, "SUB", O_DIRECTORY | O_RDONLY);
    CHECK(sub >= 0);
    int file = openat(sub, "ONE.TX", O_RDONLY);
    CHECK(file >= 0 && read(file, &c, 1) == 1 && c == 'o' && close(file) == 0);
    CHECK(fstatat(sub, "ONE.TX", &st, 0) == 0 && S_ISREG(st.st_mode) && st.st_size == 3);
    CHECK(openat(sub, "ONE.TX", O_RDONLY | O_DIRECTORY) == -1 && errno == ENOTDIR);
    CHECK(openat(sub, "ABSENT", O_RDONLY | O_DIRECTORY) == -1 && errno == ENOENT);
    CHECK(fchdir(sub) == 0 && getcwd(cwd, sizeof cwd) && !strcmp(cwd, "/SUB"));
    DIR *stream = opendir(".");
    CHECK(stream != NULL);
    unsigned names = 0;
    for (struct dirent *ent; (ent = readdir(stream));) {
        if (!strcmp(ent->d_name, "ONE.TX")) names |= 1;
        if (!strcmp(ent->d_name, "TWO.TX")) names |= 2;
    }
    CHECK(names == 3 && closedir(stream) == 0);
    CHECK(chdir("..") == 0 && getcwd(cwd, sizeof cwd) && !strcmp(cwd, "/"));
    CHECK(chdir("..") == 0 && fstatat(AT_FDCWD, ".", &st, 0) == 0);
    CHECK(st.st_dev == root_stat.st_dev && st.st_ino == root_stat.st_ino);
    CHECK(fchdir(root) == 0 && close(root) == 0 && close(sub) == 0);
    puts("directory: enumeration, openat, fstatat, fchdir and root parent passed");
    return 0;
}

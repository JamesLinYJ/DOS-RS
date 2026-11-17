#ifndef DOSRS_SYS_STAT_H
#define DOSRS_SYS_STAT_H
#include <stdint.h>
typedef uint32_t mode_t;
struct dos_stat_time { int64_t tv_sec; uint32_t tv_nsec, __pad; };
struct stat {
    uint64_t st_dev, st_ino;
    int64_t st_size, st_blocks;
    uint32_t st_mode, st_nlink, st_uid, st_gid;
    struct dos_stat_time st_atim, st_mtim, st_ctim;
};
_Static_assert(sizeof(struct stat) == 96, "DOS-RS stat ABI");
#define st_atime st_atim.tv_sec
#define st_mtime st_mtim.tv_sec
#define st_ctime st_ctim.tv_sec
#define S_IFMT 0170000u
#define S_ISDIR(m) (((m) & S_IFMT) == 0040000u)
#define S_ISREG(m) (((m) & S_IFMT) == 0100000u)
#define S_ISLNK(m) (((m) & S_IFMT) == 0120000u)
#define S_ISFIFO(m) (((m) & S_IFMT) == 0010000u)
#define S_ISCHR(m) (((m) & S_IFMT) == 0020000u)
int stat(const char *, struct stat *);
int lstat(const char *, struct stat *);
int fstat(int, struct stat *);
int mkdir(const char *, mode_t);
int chmod(const char *, mode_t);
mode_t umask(mode_t);
int mkdirat(int,const char *,mode_t);
int fstatat(int,const char *,struct stat *,int);
int fchmod(int,mode_t);
#include <time.h>
#define UTIME_NOW ((1L<<30)-1)
#define UTIME_OMIT ((1L<<30)-2)
int fchmodat(int,const char *,mode_t,int);
int utimensat(int,const char *,const struct timespec [2],int);
int futimens(int,const struct timespec [2]);
#endif

#ifndef DOSRS_SYS_STATVFS_H
#define DOSRS_SYS_STATVFS_H
#include <stdint.h>
struct statvfs {
    uint64_t f_bsize, f_frsize, f_blocks, f_bfree, f_bavail;
    uint64_t f_files, f_ffree, f_favail, f_fsid, f_flag, f_namemax;
};
#define ST_RDONLY 1
#define ST_NOSUID 2
int statvfs(const char *, struct statvfs *);
int fstatvfs(int, struct statvfs *);
#endif

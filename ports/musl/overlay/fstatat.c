#define _GNU_SOURCE
#include <sys/stat.h>
#include <fcntl.h>
#include <stdint.h>
#include <errno.h>
#include "syscall.h"
#undef sys_open
#include "dos_calls.h"

struct stamp {
    int64_t seconds;
    uint32_t nanos, pad;
};
struct metadata {
    uint64_t dev, ino, size, blocks;
    uint32_t mode, nlink, uid, gid;
    struct stamp access, change_data, change_inode;
};
_Static_assert(sizeof(struct metadata) == 96, "native stat layout");

int __fstatat(int fd, const char *restrict path, struct stat *restrict dst, int flags)
{
    if (flags & ~(AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW))
        return __syscall_ret(-EINVAL);
    struct metadata v;
    long r;
    uintptr_t options[] = {(uintptr_t)&v, (uintptr_t)flags};
    r = sys_fstatat(fd, (long)path, (long)options);
    if (r < 0)
        return __syscall_ret(r);
    *dst = (struct stat){.st_dev = v.dev,
                         .st_ino = v.ino,
                         .st_size = v.size,
                         .st_blocks = v.blocks,
                         .st_blksize = 4096,
                         .st_mode = v.mode,
                         .st_nlink = v.nlink,
                         .st_uid = v.uid,
                         .st_gid = v.gid,
                         .st_atim = {v.access.seconds, v.access.nanos},
                         .st_mtim = {v.change_data.seconds, v.change_data.nanos},
                         .st_ctim = {v.change_inode.seconds, v.change_inode.nanos}};
#if _REDIR_TIME64
    /* musl's legacy i386 stat symbols copy the prefix containing these time32 fields. */
    dst->__st_atim32.tv_sec = v.access.seconds;
    dst->__st_atim32.tv_nsec = v.access.nanos;
    dst->__st_mtim32.tv_sec = v.change_data.seconds;
    dst->__st_mtim32.tv_nsec = v.change_data.nanos;
    dst->__st_ctim32.tv_sec = v.change_inode.seconds;
    dst->__st_ctim32.tv_nsec = v.change_inode.nanos;
#endif
    return 0;
}
weak_alias(__fstatat, fstatat);

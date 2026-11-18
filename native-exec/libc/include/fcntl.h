#ifndef DOSRS_FCNTL_H
#define DOSRS_FCNTL_H
#include <stdint.h>
#define O_RDONLY 0
#define O_WRONLY 1
#define O_RDWR 2
#define O_CREAT 64
#define O_EXCL 128
#define O_TRUNC 512
#define O_APPEND 1024
#define O_DIRECTORY 65536
#define O_CLOEXEC 524288
#define O_NONBLOCK 2048
#define O_NOFOLLOW 131072
#define AT_FDCWD (-100)
#define AT_SYMLINK_NOFOLLOW 0x100
#define AT_REMOVEDIR 0x200
#define AT_SYMLINK_FOLLOW 0x400
#define AT_EACCESS 0x200
#define AT_EMPTY_PATH 0x1000
#define O_CLOFORK 0x04000000
#define FD_CLOFORK 2
#define FD_CLOEXEC 1
#define F_DUPFD 0
#define F_GETFD 1
#define F_SETFD 2
#define F_GETFL 3
#define F_SETFL 4
#define F_DUPFD_CLOEXEC 1030
#define F_DUPFD_CLOFORK 1100
#define F_OFD_GETLK 36
#define F_OFD_SETLK 37
#define F_OFD_SETLKW 38
#define F_GETLK 5
#define F_SETLK 6
#define F_SETLKW 7
#define F_RDLCK 0
#define F_WRLCK 1
#define F_UNLCK 2
struct flock {
    int16_t l_type, l_whence;
    uint32_t __pad1;
    int64_t l_start, l_len;
    int32_t l_pid;
    uint32_t __pad2;
};
_Static_assert(sizeof(struct flock) == 32, "DOS-RS lock ABI");
int open(const char *, int flags, ...);
int openat(int,const char *,int,...);
int fcntl(int,int,...);
#endif

#ifndef DOSRS_FCNTL_H
#define DOSRS_FCNTL_H
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
#define AT_EMPTY_PATH 0x1000
#define FD_CLOEXEC 1
#define F_DUPFD 0
#define F_GETFD 1
#define F_SETFD 2
#define F_GETFL 3
#define F_SETFL 4
#define F_DUPFD_CLOEXEC 1030
int open(const char *, int flags, ...);
int openat(int,const char *,int,...);
int fcntl(int,int,...);
#endif

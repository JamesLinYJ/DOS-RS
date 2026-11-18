#ifndef DOSRS_ERRNO_H
#define DOSRS_ERRNO_H
int *__dos_errno_location(void);
#define errno (*__dos_errno_location())
#define EPERM 1
#define ENOENT 2
#define ESRCH 3
#define EAGAIN 11
#define ENOMEM 12
#define EIO 5
#define EBADF 9
#define EACCES 13
#define EFAULT 14
#define EEXIST 17
#define EXDEV 18
#define ENOTDIR 20
#define EISDIR 21
#define EINVAL 22
#define EMFILE 24
#define EFBIG 27
#define ENOSPC 28
#define ESPIPE 29
#define EROFS 30
#define EPIPE 32
#define EDEADLK 35
#define ENAMETOOLONG 36
#define ERANGE 34
#define ENOTEMPTY 39
#define ELOOP 40
#define ENOSYS 38
#define ENOTSUP 95
#endif

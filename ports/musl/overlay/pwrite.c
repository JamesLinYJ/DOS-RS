#include <unistd.h>
#include "syscall.h"

/* DOS-RS honors the explicit offset even for a descriptor opened O_APPEND. */
ssize_t pwrite(int fd, const void *buf, size_t size, off_t offset)
{
    return syscall_cp(SYS_pwrite, fd, buf, size, __SYSCALL_LL_PRW(offset));
}

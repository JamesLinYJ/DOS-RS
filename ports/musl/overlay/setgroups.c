#define _GNU_SOURCE
#include <grp.h>
#include "syscall.h"

/* DOS-RS keeps the group list in the process shared by all its threads. */
int setgroups(size_t count, const gid_t *groups)
{
    return syscall(SYS_setgroups, count, groups);
}

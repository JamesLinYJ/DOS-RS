#include "syscall.h"
#include "libc.h"

/* Credentials belong to a process and are observed by every sibling thread. */
int __setxid(int nr, int id, int eid, int sid)
{
    return __syscall_ret(__syscall(nr, id, eid, sid));
}

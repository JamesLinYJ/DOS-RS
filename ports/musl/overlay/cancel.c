#include "pthread_impl.h"
extern long __cancel(void);
long __syscall_cp_asm(volatile void *flag, long n, long a, long b, long c, long d, long e, long f)
{
    if (*(const volatile int *)flag)
        return __cancel();
    return __dos_dispatch(n, a, b, c, d, e, f);
}

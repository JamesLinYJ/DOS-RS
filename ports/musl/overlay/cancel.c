#include "pthread_impl.h"

extern long __cancel(void);
static _Thread_local volatile int cancellation_point;

/* DOS-RS delivers an ordinary ring-3 signal on a thread's own stack. The
 * interrupted system call has already left the kernel on this CPU. */
void __dos_cancel_signal(int sig)
{
    (void)sig;
    pthread_t self = __pthread_self();
    if (self->cancel && self->canceldisable == PTHREAD_CANCEL_ENABLE &&
        (self->cancelasync || cancellation_point)) {
        __cancel();
    }
}

long __syscall_cp_asm(volatile void *flag, long n, long a, long b, long c, long d, long e, long f)
{
    cancellation_point = 1;
    a_barrier();
    long r = *(const volatile int *)flag ? __cancel() : __dos_dispatch(n, a, b, c, d, e, f);
    a_barrier();
    cancellation_point = 0;
    return r;
}

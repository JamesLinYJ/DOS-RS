/* musl's internal word packing, with calls dispatched to DOS-RS INT 40h. */
#ifdef __i386__
#define __SYSCALL_LL_E(x)                                                                         \
    ((union {                                                                                     \
        long long ll;                                                                             \
        long l[2];                                                                                \
    }){.ll = (x)})                                                                                \
        .l[0],                                                                                    \
        ((union {                                                                                 \
            long long ll;                                                                         \
            long l[2];                                                                            \
        }){.ll = (x)})                                                                            \
            .l[1]
#define __SYSCALL_LL_O(x) __SYSCALL_LL_E(x)
#else
#define __SYSCALL_LL_E(x) (x)
#define __SYSCALL_LL_O(x) (x)
#endif
extern long __dos_dispatch(long, long, long, long, long, long, long);
static inline long __syscall0(long n) { return __dos_dispatch(n, 0, 0, 0, 0, 0, 0); }
static inline long __syscall1(long n, long a) { return __dos_dispatch(n, a, 0, 0, 0, 0, 0); }
static inline long __syscall2(long n, long a, long b)
{
    return __dos_dispatch(n, a, b, 0, 0, 0, 0);
}
static inline long __syscall3(long n, long a, long b, long c)
{
    return __dos_dispatch(n, a, b, c, 0, 0, 0);
}
static inline long __syscall4(long n, long a, long b, long c, long d)
{
    return __dos_dispatch(n, a, b, c, d, 0, 0);
}
static inline long __syscall5(long n, long a, long b, long c, long d, long e)
{
    return __dos_dispatch(n, a, b, c, d, e, 0);
}
static inline long __syscall6(long n, long a, long b, long c, long d, long e, long f)
{
    return __dos_dispatch(n, a, b, c, d, e, f);
}

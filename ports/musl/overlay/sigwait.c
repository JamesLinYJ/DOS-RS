#include <signal.h>
#include <errno.h>

int sigwait(const sigset_t *restrict mask, int *restrict result)
{
    int saved = errno;
    int n;
    do {
        n = sigtimedwait(mask, 0, 0);
    } while (n == -1 && errno == EINTR);
    if (n < 0) {
        int error = errno;
        errno = saved;
        return error;
    }
    *result = n;
    return 0;
}

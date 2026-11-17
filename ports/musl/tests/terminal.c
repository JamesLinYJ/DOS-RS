#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>

#define CHECK(expr)                                                                                \
    do {                                                                                           \
        if (!(expr)) {                                                                             \
            dprintf(2, "tty failed at %d errno=%d\n", __LINE__, errno);                            \
            goto fail;                                                                             \
        }                                                                                          \
    } while (0)
#define MARK(s)                                                                                    \
    do {                                                                                           \
        if (write(1, s, sizeof(s) - 1) != sizeof(s) - 1)                                           \
            goto fail;                                                                             \
    } while (0)

static volatile sig_atomic_t signal_number;
static void handle(int sig) { signal_number = sig; }
static long long nanoseconds(void)
{
    struct timespec t;
    return clock_gettime(CLOCK_MONOTONIC, &t) ? -1 : t.tv_sec * 1000000000LL + t.tv_nsec;
}

int main(void)
{
    struct termios saved, raw, actual;
    struct winsize win;
    char bytes[16];
    if (tcgetattr(0, &saved) < 0)
        return 1;
    CHECK(isatty(0) && isatty(1) && isatty(2));
    int fds[2];
    CHECK(pipe(fds) == 0);
    CHECK(!isatty(fds[0]) && errno == ENOTTY && !isatty(fds[1]) && errno == ENOTTY);
    CHECK(tcgetattr(fds[0], &actual) < 0 && errno == ENOTTY);
    CHECK(!isatty(-1) && errno == EBADF);
    CHECK(close(fds[0]) == 0 && close(fds[1]) == 0);
    CHECK(ioctl(1, TIOCGWINSZ, &win) == 0 && win.ws_row == 25 && win.ws_col == 80);
    CHECK(ioctl(0, TIOCSWINSZ, &win) == 0);
    CHECK(tcdrain(1) == 0 && tcflush(0, TCIFLUSH) == 0);
    struct pollfd writeable = {1, POLLOUT, 0};
    CHECK(tcflow(1, TCOOFF) == 0 && poll(&writeable, 1, 0) == 0);
    CHECK(tcflow(1, TCOON) == 0 && poll(&writeable, 1, 0) == 1 && writeable.revents == POLLOUT);
    struct sigaction action = {0};
    action.sa_handler = handle;
    CHECK(sigaction(SIGINT, &action, 0) == 0);
    CHECK((saved.c_lflag & (ICANON | ECHO | ISIG)) == (ICANON | ECHO | ISIG));
    raw = saved;
    cfmakeraw(&raw);
    raw.c_cc[VMIN] = 0;
    raw.c_cc[VTIME] = 0;
    CHECK(tcsetattr(0, TCSAFLUSH, &raw) == 0);
    CHECK(tcgetattr(1, &actual) == 0 && !(actual.c_lflag & (ICANON | ECHO | ISIG)));
    CHECK(read(0, bytes, sizeof bytes) == 0);
    raw.c_cc[VTIME] = 1;
    CHECK(tcsetattr(1, TCSADRAIN, &raw) == 0);
    long long start = nanoseconds();
    CHECK(read(0, bytes, sizeof bytes) == 0 && nanoseconds() - start >= 100000000LL);
    raw.c_cc[VMIN] = 2;
    raw.c_cc[VTIME] = 0;
    CHECK(tcsetattr(0, TCSANOW, &raw) == 0);
    MARK("TTY RAW READY\n");
    CHECK(read(0, bytes, sizeof bytes) == 2 && bytes[0] == 3 && bytes[1] == 4 && !signal_number);
    raw.c_cc[VMIN] = 4;
    raw.c_cc[VTIME] = 3;
    CHECK(tcsetattr(0, TCSANOW, &raw) == 0);
    MARK("TTY INTERBYTE READY\n");
    CHECK(read(0, bytes, sizeof bytes) == 2 && !memcmp(bytes, "xy", 2));
    struct termios canonical = saved;
    canonical.c_cc[VINTR] = 23;
    CHECK(tcsetattr(1, TCSAFLUSH, &canonical) == 0);
    MARK("TTY CANONICAL READY\n");
    CHECK(read(0, bytes, sizeof bytes) == 3 && !memcmp(bytes, "ac\n", 3));
    MARK("TTY SIGNAL READY\n");
    CHECK(read(0, bytes, sizeof bytes) == -1 && errno == EINTR && signal_number == SIGINT);
    MARK("TTY EOF READY\n");
    CHECK(read(0, bytes, sizeof bytes) == 2 && !memcmp(bytes, "de", 2));
    CHECK(read(0, bytes, sizeof bytes) == 0);
    CHECK(tcsetattr(0, TCSAFLUSH, &saved) == 0);
    MARK("tty: descriptors, VGA 80x25, canonical, raw, timers, echo, signals and "
         "EOF passed\n");
    return 0;
fail:
    tcsetattr(0, TCSAFLUSH, &saved);
    return 1;
}

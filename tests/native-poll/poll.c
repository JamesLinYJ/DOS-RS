// Link with DOS-RS musl and run as a separate native ELF.
#define _GNU_SOURCE
#include <errno.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <time.h>
#include <unistd.h>
#include <sys/wait.h>

#define CHECK(expr) do { if (!(expr)) { dprintf(2, "poll check failed at line %d: %s (errno %d)\n", __LINE__, #expr, errno); return 1; } } while (0)

static volatile sig_atomic_t caught;
static volatile sig_atomic_t handler_mask;

static void handle_signal(int signo)
{
    sigset_t mask;
    if (sigprocmask(SIG_SETMASK, NULL, &mask) == 0)
        handler_mask = sigismember(&mask, SIGUSR1) && !sigismember(&mask, SIGUSR2);
    caught = signo;
}

int main(void)
{
    struct pollfd fd[] = {{-1, POLLIN, 99}, {999, 0, 99}, {1, POLLOUT, 99}};
    CHECK(poll(fd, 3, 0) == 2);
    CHECK(fd[0].revents == 0 && fd[1].revents == POLLNVAL && fd[2].revents == POLLOUT);
    int p[2];
    CHECK(pipe(p) == 0);
    fd[0] = (struct pollfd){p[0], POLLIN | POLLRDNORM, 99};
    fd[1] = (struct pollfd){p[1], POLLOUT | POLLWRNORM, 99};
    CHECK(poll(fd, 2, 0) == 1 && fd[0].revents == 0 && fd[1].revents == fd[1].events);
    char fill[4096] = {0};
    CHECK(write(p[1], fill, sizeof fill) == sizeof fill);
    CHECK(poll(fd, 2, 0) == 1 && fd[0].revents == fd[0].events && fd[1].revents == 0);
    CHECK(read(p[0], fill, sizeof fill) == sizeof fill);
    pid_t child = fork();
    CHECK(child >= 0);
    if (!child) {
        close(p[0]);
        struct timespec t = {0, 50000000};
        if (nanosleep(&t, NULL) || write(p[1], "Q", 1) != 1) _exit(5);
        _exit(0);
    }
    CHECK(close(p[1]) == 0);
    CHECK(poll(fd, 1, -1) == 1 && (fd[0].revents & POLLIN));
    int status;
    CHECK(waitpid(child, &status, 0) == child && status == 0);
    CHECK(poll(fd, 1, 100) == 1 && fd[0].revents == (fd[0].events | POLLHUP));
    CHECK(read(p[0], fill, 1) == 1 && fill[0] == 'Q');
    fd[0].events = 0;
    CHECK(poll(fd, 1, 0) == 1 && fd[0].revents == POLLHUP);
    CHECK(read(p[0], fill, 1) == 0 && close(p[0]) == 0);
    CHECK(pipe(p) == 0 && close(p[0]) == 0);
    fd[0] = (struct pollfd){p[1], 0, 99};
    CHECK(poll(fd, 1, 0) == 1 && fd[0].revents == POLLERR);
    CHECK(close(p[1]) == 0);
    struct timespec before, after, delay = {0, 20000000}, invalid = {0, 1000000000};
    CHECK(clock_gettime(CLOCK_MONOTONIC, &before) == 0);
    CHECK(ppoll(NULL, 0, &delay, NULL) == 0);
    CHECK(clock_gettime(CLOCK_MONOTONIC, &after) == 0);
    CHECK((after.tv_sec - before.tv_sec) * 1000000000LL + after.tv_nsec - before.tv_nsec >= delay.tv_nsec);
    CHECK(ppoll(NULL, 0, &invalid, NULL) == -1 && errno == EINVAL);
    struct sigaction action = {.sa_handler = handle_signal};
    sigset_t old, blocked, temporary, restored;
    sigemptyset(&action.sa_mask);
    sigemptyset(&blocked);
    sigaddset(&blocked, SIGUSR1);
    sigaddset(&blocked, SIGUSR2);
    sigemptyset(&temporary);
    CHECK(sigaction(SIGUSR1, &action, NULL) == 0);
    CHECK(sigprocmask(SIG_BLOCK, &blocked, &old) == 0);
    CHECK(kill(getpid(), SIGUSR1) == 0 && !caught);
    delay = (struct timespec){0, 0};
    CHECK(ppoll(NULL, 0, &delay, &temporary) == -1 && errno == EINTR);
    CHECK(caught == SIGUSR1 && handler_mask);
    CHECK(sigprocmask(SIG_SETMASK, &old, &restored) == 0);
    CHECK(sigismember(&restored, SIGUSR1) && sigismember(&restored, SIGUSR2));
    puts("poll: descriptors, blocking pipe, peer close, timeout and signal mask passed");
    return 0;
}

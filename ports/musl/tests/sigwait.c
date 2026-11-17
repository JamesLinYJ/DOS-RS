#define _GNU_SOURCE
#include <errno.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdio.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define CHECK(x)                                                                                   \
    do {                                                                                           \
        if (!(x)) {                                                                                \
            fprintf(stderr, "signal wait failed at %d: errno=%d\n", __LINE__, errno);              \
            return 1;                                                                              \
        }                                                                                          \
    } while (0)
static sigset_t usrs;
static atomic_int ready;
static int results[2];

static void *consumer(void *ptr)
{
    int id = *(int *)ptr;
    siginfo_t info;
    atomic_fetch_add(&ready, 1);
    int n = sigwaitinfo(&usrs, &info);
    int expected_pid = getpid();
    if (n < 0 || info.si_signo != n || info.si_pid != expected_pid || info.si_uid != getuid())
        return 0;
    int report[3] = {id, n, info.si_code};
    return write(results[1], report, sizeof report) == sizeof report ? ptr : 0;
}

static volatile sig_atomic_t alarm_count;
static void alarm_handler(int signo)
{
    if (signo == SIGALRM)
        alarm_count++;
}

static long long now(void)
{
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts))
        return -1;
    return ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

int main(void)
{
    int ids[] = {0, 1};
    pthread_t threads[2];
    sigset_t old;
    sigemptyset(&usrs);
    sigaddset(&usrs, SIGUSR1);
    sigaddset(&usrs, SIGUSR2);
    CHECK(!pthread_sigmask(SIG_BLOCK, &usrs, &old) && pipe(results) == 0);
    for (int i = 0; i < 2; i++)
        CHECK(!pthread_create(&threads[i], 0, consumer, &ids[i]));
    while (atomic_load(&ready) != 2)
        sched_yield();
    CHECK(kill(getpid(), SIGUSR1) == 0);
    int report[3];
    CHECK(read(results[0], report, sizeof report) == sizeof report &&
          (report[0] == 0 || report[0] == 1) && report[1] == SIGUSR1 && report[2] == SI_USER);
    int other = 1 - report[0];
    struct pollfd reader = {results[0], POLLIN, 0};
    CHECK(poll(&reader, 1, 20) == 0);
    CHECK(!pthread_kill(threads[other], SIGUSR2));
    CHECK(read(results[0], report, sizeof report) == sizeof report && report[0] == other &&
          report[1] == SIGUSR2 && report[2] == SI_TKILL);
    for (int i = 0; i < 2; i++) {
        void *value;
        CHECK(!pthread_join(threads[i], &value) && value == &ids[i]);
    }
    struct timespec zero = {0, 0}, short_wait = {0, 50000000}, invalid = {0, 1000000000};
    siginfo_t info;
    CHECK(sigtimedwait(&usrs, &info, &zero) == -1 && errno == EAGAIN);
    CHECK(sigtimedwait(&usrs, &info, &invalid) == -1 && errno == EINVAL);
    long long begin = now();
    CHECK(sigtimedwait(&usrs, &info, &short_wait) == -1 && errno == EAGAIN &&
          now() - begin >= 50000000LL);
    CHECK(kill(getpid(), SIGUSR1) == 0);
    int signo;
    CHECK(sigwait(&usrs, &signo) == 0 && signo == SIGUSR1);
    sigset_t rt;
    sigemptyset(&rt);
    sigaddset(&rt, SIGRTMIN);
    sigaddset(&rt, SIGRTMIN + 1);
    CHECK(!pthread_sigmask(SIG_BLOCK, &rt, 0));
    pid_t pid = fork();
    CHECK(pid >= 0);
    if (pid == 0) {
        pid_t parent = getppid();
        _exit(kill(parent, SIGRTMIN + 1) || kill(parent, SIGRTMIN) || kill(parent, SIGRTMIN));
    }
    int status;
    CHECK(waitpid(pid, &status, 0) == pid && status == 0);
    for (int i = 0; i < 3; i++) {
        int expected = i < 2 ? SIGRTMIN : SIGRTMIN + 1;
        CHECK(sigtimedwait(&rt, &info, &zero) == expected && info.si_code == SI_USER &&
              info.si_pid == pid && info.si_uid == getuid());
    }
    CHECK(sigtimedwait(&rt, 0, &zero) == -1 && errno == EAGAIN);
    struct sigaction alarm = {0};
    alarm.sa_handler = alarm_handler;
    CHECK(sigaction(SIGALRM, &alarm, 0) == 0);
    pid = fork();
    CHECK(pid >= 0);
    if (!pid) {
        struct timespec pause = {0, 60000000};
        nanosleep(&pause, 0);
        _exit(kill(getppid(), SIGALRM));
    }
    struct timespec long_wait = {2, 0};
    CHECK(sigtimedwait(&rt, 0, &long_wait) == -1 && errno == EINTR && alarm_count == 1);
    CHECK(waitpid(pid, &status, 0) == pid && status == 0);
    sigset_t children;
    sigemptyset(&children);
    sigaddset(&children, SIGCHLD);
    CHECK(!pthread_sigmask(SIG_BLOCK, &children, 0));
    pid = fork();
    CHECK(pid >= 0);
    if (!pid)
        _exit(37);
    CHECK(sigwaitinfo(&children, &info) == SIGCHLD && info.si_pid == pid &&
          info.si_code == CLD_EXITED && info.si_status == 37);
    CHECK(waitpid(pid, &status, 0) == pid && WEXITSTATUS(status) == 37);
    CHECK(!pthread_sigmask(SIG_SETMASK, &old, 0));
    puts("sigwait: process, targeted thread, competing waiters, realtime order, sender, child and "
         "timeout passed");
    return 0;
}

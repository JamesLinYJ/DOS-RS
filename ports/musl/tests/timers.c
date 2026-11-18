#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define CHECK(expression) do { if (!(expression)) { \
    fprintf(stderr, "timer line %d errno=%d: %s\n", __LINE__, errno, #expression); \
    return 1; \
} } while (0)

static volatile sig_atomic_t alarm_count;
static void alarm_handler(int sig) { if (sig == SIGALRM) ++alarm_count; }
extern long sys_sigwait(long set, long info, long timeout);

static int create(timer_t *timer, int signal, uintptr_t value)
{
    struct sigevent ev = {0};
    ev.sigev_notify = SIGEV_SIGNAL;
    ev.sigev_signo = signal;
    ev.sigev_value.sival_ptr = (void *)value;
    return timer_create(CLOCK_MONOTONIC, &ev, timer);
}

static int arm(timer_t timer, long first, long repeat)
{
    struct itimerspec setting = {{0, repeat}, {0, first}};
    return timer_settime(timer, 0, &setting, NULL);
}

static int sleep_ms(long ms)
{
    struct timespec remaining = {ms / 1000, (ms % 1000) * 1000000};
    while (nanosleep(&remaining, &remaining)) {
        if (errno != EINTR) return -1;
    }
    return 0;
}

static int wait_signal(sigset_t *set, siginfo_t *info)
{
    return sigtimedwait(set, info, &(struct timespec){2, 0});
}

static int periodic(sigset_t *set)
{
    timer_t timer;
    uintptr_t tag = sizeof(void *) == 8 ? (uintptr_t)UINT64_C(0x12345678abcdef90) : 0xabcdef90U;
    CHECK(create(&timer, SIGUSR1, tag) == 0);
    struct itimerspec setting;
    CHECK(timer_gettime(timer, &setting) == 0 && !setting.it_value.tv_sec && !setting.it_value.tv_nsec);
    CHECK(arm(timer, 10000000, 5000000) == 0);
    CHECK(sleep_ms(70) == 0);
    siginfo_t info;
    CHECK(wait_signal(set, &info) == SIGUSR1);
    CHECK(info.si_code == SI_TIMER && (uintptr_t)info.si_value.sival_ptr == tag);
    CHECK(info.si_timerid == (int)(intptr_t)timer && info.si_overrun >= 5);
    CHECK(timer_getoverrun(timer) == info.si_overrun);
    int saved_overrun = info.si_overrun;
    CHECK(timer_gettime(timer, &setting) == 0 && setting.it_interval.tv_nsec == 5000000);
    CHECK(setting.it_value.tv_sec == 0 && setting.it_value.tv_nsec <= 5000000);
    struct itimerspec bad = {{0, 0}, {0, 1000000000}};
    CHECK(timer_settime(timer, 0, &bad, NULL) == -1 && errno == EINVAL);
    CHECK(timer_settime(timer, 2, &setting, NULL) == -1 && errno == EINVAL);
    CHECK(timer_delete(timer) == 0);
    CHECK(timer_gettime(timer, &setting) == -1 && errno == EINVAL);
    CHECK(sigtimedwait(set, &info, &(struct timespec){0, 0}) == -1 && errno == EAGAIN);
    printf("periodic value=%llx overrun=%d\n", (unsigned long long)tag, saved_overrun);
    return 0;
}

static int multiple(sigset_t *set)
{
    timer_t first, second;
    CHECK(create(&first, SIGUSR1, 101) == 0 && create(&second, SIGUSR1, 202) == 0);
    CHECK(arm(first, 10000000, 0) == 0 && arm(second, 15000000, 0) == 0);
    CHECK(sleep_ms(40) == 0);
    siginfo_t a, b;
    CHECK(wait_signal(set, &a) == SIGUSR1 && wait_signal(set, &b) == SIGUSR1);
    CHECK((uintptr_t)a.si_value.sival_ptr == 101 && (uintptr_t)b.si_value.sival_ptr == 202);
    CHECK(a.si_overrun == 0 && b.si_overrun == 0);
    CHECK(timer_delete(first) == 0 && timer_delete(second) == 0);
    return 0;
}

static int absolute(sigset_t *set)
{
    struct sigevent ev = {.sigev_notify = SIGEV_NONE};
    timer_t timer;
    CHECK(timer_create(CLOCK_REALTIME, &ev, &timer) == 0);
    struct itimerspec setting = {0}, old;
    CHECK(clock_gettime(CLOCK_REALTIME, &setting.it_value) == 0);
    setting.it_value.tv_nsec += 30000000;
    if (setting.it_value.tv_nsec >= 1000000000) {
        ++setting.it_value.tv_sec;
        setting.it_value.tv_nsec -= 1000000000;
    }
    CHECK(timer_settime(timer, TIMER_ABSTIME, &setting, &old) == 0);
    CHECK(!old.it_value.tv_sec && !old.it_value.tv_nsec);
    CHECK(sleep_ms(50) == 0);
    CHECK(timer_gettime(timer, &setting) == 0 && !setting.it_value.tv_sec && !setting.it_value.tv_nsec);
    CHECK(timer_delete(timer) == 0);
    CHECK(timer_create(-1, &ev, &timer) == -1 && errno == EINVAL);
    ev.sigev_notify = SIGEV_THREAD_ID;
    ev.sigev_signo = SIGUSR1;
    ev.sigev_notify_thread_id = getpid();
    CHECK(timer_create(CLOCK_MONOTONIC, &ev, &timer) == -1 && errno == EOPNOTSUPP);
    CHECK(create(&timer, SIGUSR1, 303) == 0);
    CHECK(clock_gettime(CLOCK_MONOTONIC, &setting.it_value) == 0);
    setting.it_value.tv_nsec -= 10000000;
    if (setting.it_value.tv_nsec < 0) {
        --setting.it_value.tv_sec;
        setting.it_value.tv_nsec += 1000000000;
    }
    CHECK(timer_settime(timer, TIMER_ABSTIME, &setting, NULL) == 0);
    siginfo_t info;
    CHECK(sigtimedwait(set, &info, &(struct timespec){0, 0}) == SIGUSR1);
    CHECK(info.si_code == SI_TIMER && (uintptr_t)info.si_value.sival_ptr == 303);
    CHECK(timer_delete(timer) == 0);
    return 0;
}

static int lifecycle(void)
{
    timer_t parent;
    CHECK(create(&parent, SIGUSR2, 77) == 0);
    for (int round = 0; round < 4; ++round) {
        pid_t child = fork();
        CHECK(child >= 0);
        if (!child) {
            struct itimerspec remaining;
            if (timer_gettime(parent, &remaining) != -1 || errno != EINVAL) _exit(2);
            for (int n = 0; n < 128; ++n) {
                timer_t t;
                if (create(&t, SIGUSR2, n) || arm(t, 800000000, 800000000)) _exit(3);
            }
            _exit(0);
        }
        int status;
        CHECK(waitpid(child, &status, 0) == child && WIFEXITED(status) && WEXITSTATUS(status) == 0);
    }
    pid_t child = fork();
    CHECK(child >= 0);
    if (!child) {
        timer_t t;
        if (create(&t, SIGUSR2, 88) || arm(t, 800000000, 0)) _exit(4);
        char number[32];
        snprintf(number, sizeof number, "%lu", (unsigned long)(uintptr_t)t);
        char *args[] = {"timers.elf", "after-exec", number, NULL};
        execv("timers.elf", args);
        _exit(5);
    }
    int status;
    CHECK(waitpid(child, &status, 0) == child && WIFEXITED(status) && WEXITSTATUS(status) == 0);
    CHECK(timer_delete(parent) == 0);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc == 3 && !strcmp(argv[1], "after-exec")) {
        timer_t old = (timer_t)(uintptr_t)strtoul(argv[2], NULL, 10);
        struct itimerspec value;
        CHECK(timer_gettime(old, &value) == -1 && errno == EINVAL);
        return 0;
    }
    sigset_t set;
    sigemptyset(&set);
    sigaddset(&set, SIGUSR1);
    sigaddset(&set, SIGUSR2);
    sigaddset(&set, SIGALRM);
    CHECK(sigprocmask(SIG_BLOCK, &set, NULL) == 0);
    struct { uint32_t signo; int32_t code, pid; uint32_t uid; int32_t status; uint32_t guard; } legacy = {0};
    legacy.guard = 0x76543210;
    CHECK(kill(getpid(), SIGUSR1) == 0);
    CHECK(sys_sigwait((long)&set, (long)&legacy, 0) == SIGUSR1);
    CHECK(legacy.signo == SIGUSR1 && legacy.code == SI_USER && legacy.pid == getpid());
    CHECK(legacy.guard == 0x76543210);
    CHECK(periodic(&set) == 0 && multiple(&set) == 0 && absolute(&set) == 0 && lifecycle() == 0);
    timer_t timer;
    CHECK(timer_create(CLOCK_MONOTONIC, NULL, &timer) == 0 && arm(timer, 10000000, 0) == 0);
    siginfo_t info;
    CHECK(wait_signal(&set, &info) == SIGALRM && info.si_code == SI_TIMER);
    CHECK((uintptr_t)info.si_value.sival_ptr == (uintptr_t)timer);
    CHECK(timer_delete(timer) == 0);
    struct sigaction action = {.sa_handler = alarm_handler};
    sigemptyset(&action.sa_mask);
    action.sa_flags = SA_SIGINFO;
    CHECK(sigaction(SIGALRM, &action, NULL) == -1 && errno == EOPNOTSUPP);
    action.sa_flags = 0;
    CHECK(sigaction(SIGALRM, &action, NULL) == 0);
    CHECK(sigprocmask(SIG_UNBLOCK, &set, NULL) == 0);
    CHECK(create(&timer, SIGALRM, 9) == 0 && arm(timer, 10000000, 0) == 0);
    CHECK(sleep_ms(50) == 0 && alarm_count == 1);
    CHECK(timer_delete(timer) == 0);
    puts("timers: values, overruns, shared signal, absolute clock, fork, exec, exit and handler passed");
    return 0;
}

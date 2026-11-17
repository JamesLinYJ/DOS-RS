#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

#define CHECK(test) do { if (!(test)) { \
    fprintf(stderr, "limits line %d errno=%d: %s\n", __LINE__, errno, #test); \
    return 1; \
} } while (0)

static volatile size_t input_count;

__attribute__((noinline)) static void touch_stack(size_t size, int syscall_only)
{
    volatile unsigned char bytes[size];
    if (syscall_only) {
        /* Keep the VLA live across the zero-byte system call. */
        read(STDIN_FILENO, (void *)bytes, input_count);
        return;
    }
    for (size_t at = 0; at < size; at += 4096) bytes[at] = 42;
    bytes[size - 1] = 42;
    for (size_t at = 0; at < size; at += 4096) if (bytes[at] != 42) _exit(3);
}

static int child_stack(size_t size, int syscall_only, int fault)
{
    pid_t pid = fork();
    CHECK(pid >= 0);
    if (!pid) {
        struct rlimit limit;
        if (getrlimit(RLIMIT_STACK, &limit) || limit.rlim_cur != 100 * 1024 ||
            limit.rlim_max != 100 * 1024) _exit(2);
        touch_stack(size, syscall_only);
        _exit(0);
    }
    int status;
    CHECK(waitpid(pid, &status, 0) == pid);
    CHECK(fault ? WIFSIGNALED(status) && WTERMSIG(status) == SIGSEGV
                : WIFEXITED(status) && WEXITSTATUS(status) == 0);
    return 0;
}

int main(int argc, char **argv)
{
    struct rlimit before, limit;
    CHECK(getrlimit(RLIMIT_STACK, &before) == 0);
    if (argc == 2 && !strcmp(argv[1], "after-exec")) {
        CHECK(before.rlim_cur == 100 * 1024 && before.rlim_max == 100 * 1024);
        touch_stack(80 * 1024, 0);
        return 0;
    }
    CHECK(before.rlim_cur == 8 * 1024 * 1024 && before.rlim_max == before.rlim_cur);
    limit = (struct rlimit){100 * 1024, 100 * 1024};
    struct rlimit saved;
    CHECK(prlimit(0, RLIMIT_STACK, &limit, &saved) == 0);
    CHECK(saved.rlim_cur == before.rlim_cur && saved.rlim_max == before.rlim_max);
    CHECK(getrlimit(RLIMIT_STACK, &saved) == 0 && saved.rlim_cur == 100 * 1024);
    CHECK(setrlimit(RLIMIT_STACK, &(struct rlimit){101 * 1024, 100 * 1024}) == -1 && errno == EINVAL);
    CHECK(setrlimit(RLIMIT_STACK, &(struct rlimit){100 * 1024, 9 * 1024 * 1024}) == -1 && errno == EPERM);
    CHECK(getrlimit(-1, &limit) == -1 && errno == EINVAL);
    unsigned long legacy[2];
    CHECK(syscall(SYS_getrlimit, RLIMIT_STACK, legacy) == 0);
    CHECK(legacy[0] == 100 * 1024 && legacy[1] == 100 * 1024);
#ifdef SYS_ugetrlimit
    CHECK(syscall(SYS_ugetrlimit, RLIMIT_STACK, legacy) == 0);
    CHECK(legacy[0] == 100 * 1024 && legacy[1] == 100 * 1024);
#endif
    CHECK(child_stack(80 * 1024, 0, 0) == 0);
    CHECK(child_stack(120 * 1024, 0, 1) == 0);
    CHECK(child_stack(120 * 1024, 1, 1) == 0);
    pid_t pid = fork();
    CHECK(pid >= 0);
    if (!pid) {
        char *args[] = {argv[0], "after-exec", NULL};
        execv(argv[0], args);
        _exit(4);
    }
    int status;
    CHECK(waitpid(pid, &status, 0) == pid && WIFEXITED(status) && WEXITSTATUS(status) == 0);
    pid = fork();
    CHECK(pid >= 0);
    if (!pid) {
        char *big = malloc(8192);
        if (!big) _exit(5);
        memset(big, 'x', 8191);
        big[8191] = 0;
        if (setrlimit(RLIMIT_STACK, &(struct rlimit){4096, 4096})) _exit(6);
        char *args[] = {argv[0], big, NULL};
        if (execv(argv[0], args) != -1 || errno != E2BIG) _exit(7);
        if (getrlimit(RLIMIT_STACK, &limit) || limit.rlim_cur != 4096 || getpid() <= 0) _exit(8);
        _exit(0);
    }
    CHECK(waitpid(pid, &status, 0) == pid && WIFEXITED(status) && WEXITSTATUS(status) == 0);
    CHECK(getrlimit(RLIMIT_STACK, &limit) == 0 && limit.rlim_cur == 100 * 1024);
    pid = fork();
    CHECK(pid >= 0);
    if (!pid) {
        if (setuid(1000)) _exit(9);
        if (setrlimit(RLIMIT_STACK, &before) != -1 || errno != EPERM) _exit(10);
        if (setrlimit(RLIMIT_STACK, &(struct rlimit){80 * 1024, 100 * 1024})) _exit(11);
        if (getrlimit(RLIMIT_STACK, &limit) || limit.rlim_cur != 80 * 1024) _exit(12);
        _exit(0);
    }
    CHECK(waitpid(pid, &status, 0) == pid && WIFEXITED(status) && WEXITSTATUS(status) == 0);
    CHECK(setrlimit(RLIMIT_STACK, &before) == 0);
    puts("limits: stack growth, syscall entry, fork, exec and failed exec passed");
    return 0;
}

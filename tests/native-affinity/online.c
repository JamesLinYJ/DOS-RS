#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

#define CHECK(test) do { if (!(test)) return __LINE__; } while (0)

static int started[2], release[2];

static void *worker(void *unused)
{
    (void)unused;
    pid_t tid = syscall(SYS_gettid);
    char ack;
    if (write(started[1], &tid, sizeof tid) != sizeof tid ||
        read(release[0], &ack, 1) != 1) _exit(90);
    return NULL;
}

static int check_affinity(pid_t tid, int cpus)
{
    cpu_set_t set;
    memset(&set, 0xff, sizeof set);
    if (sched_getaffinity(tid, sizeof set, &set) || CPU_COUNT(&set) != cpus)
        return 0;
    for (int i = 0; i < cpus; ++i)
        if (!CPU_ISSET(i, &set)) return 0;
    return 1;
}

int main(int argc, char **argv)
{
    CHECK(argc == 2);
    int cpus = atoi(argv[1]);
    CHECK(cpus > 0 && cpus <= 8);
    CHECK(sysconf(_SC_NPROCESSORS_ONLN) == cpus);
    CHECK(sysconf(_SC_NPROCESSORS_CONF) == cpus);
    CHECK(check_affinity(0, cpus) && check_affinity(getpid(), cpus));
    unsigned long mask[2] = {0, ~0UL};
    CHECK(syscall(SYS_sched_getaffinity, 0, sizeof mask, mask) == sizeof *mask);
    CHECK(mask[0] == (1UL << cpus) - 1 && mask[1] == ~0UL);
    cpu_set_t set;
    CHECK(sched_getaffinity(0, 0, &set) == -1 && errno == EINVAL);
    CHECK(sched_getaffinity(0, sizeof(long)-1, &set) == -1 && errno == EINVAL);
    CHECK(sched_getaffinity(0, sizeof(long)+1, &set) == -1 && errno == EINVAL);
    CHECK(syscall(SYS_sched_getaffinity, 0, sizeof(long), (void *)1) == -1 && errno == EFAULT);
    CHECK(sched_getaffinity(-1, sizeof set, &set) == -1 && errno == ESRCH);
    CHECK(pipe(started) == 0 && pipe(release) == 0);
    pid_t child = fork();
    CHECK(child >= 0);
    if (!child) {
        close(started[0]);
        close(release[1]);
        pthread_t peer;
        if (pthread_create(&peer, NULL, worker, NULL) || pthread_join(peer, NULL)) _exit(91);
        _exit(0);
    }
    close(started[1]);
    close(release[0]);
    pid_t tid;
    CHECK(read(started[0], &tid, sizeof tid) == sizeof tid && tid != child);
    CHECK(check_affinity(child, cpus) && check_affinity(tid, cpus));
    CHECK(write(release[1], "x", 1) == 1);
    int status;
    CHECK(waitpid(child, &status, 0) == child && WIFEXITED(status) && WEXITSTATUS(status) == 0);
    CHECK(sched_getaffinity(child, sizeof set, &set) == -1 && errno == ESRCH);
    CHECK(sched_getaffinity(tid, sizeof set, &set) == -1 && errno == ESRCH);
    printf("online=%d mask=%lx foreign-process-and-thread=ok\n", cpus, mask[0]);
    return 0;
}

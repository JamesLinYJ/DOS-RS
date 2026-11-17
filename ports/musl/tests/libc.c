#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <poll.h>
#include <stdatomic.h>
#include <signal.h>
#include <time.h>
#include <sys/wait.h>
#include <sys/stat.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static _Thread_local int initialized = 37;
static _Thread_local char zeroed[113];
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cond = PTHREAD_COND_INITIALIZER;
static int arrived, sum;
static int *addresses[2];
static volatile sig_atomic_t signals;
static void handle(int sig)
{
    if (sig == SIGUSR1 && initialized == 37)
        signals++;
}

static void *worker(void *p)
{
    int i = *(int *)p;
    addresses[i] = &initialized;
    if (initialized != 37 || zeroed[111] != 0)
        return 0;
    initialized = 10 + i;
    zeroed[111] = (char)initialized;
    errno = i ? EINVAL : EBADF;
    pthread_mutex_lock(&lock);
    ++arrived;
    pthread_cond_broadcast(&cond);
    while (arrived < 2)
        pthread_cond_wait(&cond, &lock);
    sum += initialized;
    pthread_mutex_unlock(&lock);
    return errno == (i ? EINVAL : EBADF) && zeroed[111] == initialized ? p : 0;
}

static int blocked_pipe[2];
static atomic_int cancel_ready;
static int cleaned;
static void cleanup(void *data) { *(int *)data = 1; }
static void *cancel_worker(void *unused)
{
    (void)unused;
    pthread_cleanup_push(cleanup, &cleaned);
    atomic_store(&cancel_ready, 1);
    char byte;
    read(blocked_pipe[0], &byte, 1);
    pthread_cleanup_pop(0);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc == 2 && !strcmp(argv[1], "child")) {
        const char *value = getenv("DOS_MUSL_ENV");
        char ignored;
        const char *fd_env = getenv("DOS_CLOSEFD");
        if (!fd_env || read(atoi(fd_env), &ignored, 1) != -1 || errno != EBADF)
            return 21;
        return value && !strcmp(value, "from-parent") && initialized == 37 ? 23 : 20;
    }
    if (argc != 2 || strcmp(argv[1], "probe") || initialized != 37 || zeroed[0])
        return 1;
    int *memory = calloc(2048, sizeof *memory);
    if (!memory || memory[1900])
        return 2;
    memory[1900] = 0x72;
    memory = realloc(memory, 16384);
    if (!memory || memory[1900] != 0x72)
        return 3;
    free(memory);
    pthread_t tids[2];
    int ids[] = {0, 1};
    for (int i = 0; i < 2; i++)
        if (pthread_create(&tids[i], 0, worker, &ids[i]))
            return 4;
    for (int i = 0; i < 2; i++) {
        void *value;
        if (pthread_join(tids[i], &value) || value != &ids[i])
            return 5;
    }
    if (sum != 21 || initialized != 37 || addresses[0] == addresses[1] ||
        addresses[0] == &initialized || addresses[1] == &initialized)
        return 6;
    FILE *f = fopen("musl.txt", "wb");
    if (!f)
        return 7;
    if (fprintf(f, "argc=%d sum=%d tls=%d\n", argc, sum, initialized) < 0 || fclose(f))
        return 8;
    int fd = open("musl.txt", O_RDONLY);
    char buf[64];
    struct stat st;
    if (fd < 0 || fstat(fd, &st) || st.st_size != 21 || read(fd, buf, sizeof buf) != 21 ||
        close(fd))
        return 9;
    int gone = open("musl.txt", O_RDONLY | O_CLOEXEC);
    if (gone < 0)
        return 17;
    char close_env[40];
    snprintf(close_env, sizeof close_env, "DOS_CLOSEFD=%d", gone);
    pid_t pid = fork();
    if (!pid) {
        char *const av[] = {"libtest.elf", "child", 0};
        char *const env[] = {"DOS_MUSL_ENV=from-parent", close_env, 0};
        execve("libtest.elf", av, env);
        _exit(42);
    }
    int status;
    if (pid < 0 || waitpid(pid, &status, 0) != pid || !WIFEXITED(status) ||
        WEXITSTATUS(status) != 23)
        return 10;
    if (close(gone))
        return 18;
    int pipefd[2];
    if (pipe(pipefd))
        return 19;
    struct pollfd watcher = {pipefd[0], POLLIN, 0};
    if (poll(&watcher, 1, 0) != 0 || write(pipefd[1], "X", 1) != 1 || poll(&watcher, 1, 0) != 1 ||
        !(watcher.revents & POLLIN) || close(pipefd[0]) || close(pipefd[1]))
        return 22;
    struct timespec real, start, end, res, delay = {0, 30000000};
    if (clock_gettime(CLOCK_REALTIME, &real) || real.tv_sec < 1704067200 ||
        clock_gettime(CLOCK_MONOTONIC, &start) || clock_getres(CLOCK_MONOTONIC, &res) ||
        !(res.tv_sec || res.tv_nsec) || nanosleep(&delay, 0) ||
        clock_gettime(CLOCK_MONOTONIC, &end))
        return 11;
    if ((end.tv_sec - start.tv_sec) * 1000000000LL + end.tv_nsec - start.tv_nsec < delay.tv_nsec)
        return 12;
    if (clock_gettime(CLOCK_REALTIME, &end))
        return 13;
    end.tv_nsec += 20000000;
    if (end.tv_nsec >= 1000000000) {
        end.tv_sec++;
        end.tv_nsec -= 1000000000;
    }
    pthread_mutex_lock(&lock);
    int timed = pthread_cond_timedwait(&cond, &lock, &end);
    pthread_mutex_unlock(&lock);
    if (timed != ETIMEDOUT)
        return 14;
    struct sigaction action = {0};
    action.sa_handler = handle;
    sigemptyset(&action.sa_mask);
    sigset_t set, old, pending;
    sigemptyset(&set);
    sigaddset(&set, SIGUSR1);
    if (sigaction(SIGUSR1, &action, 0) || sigprocmask(SIG_BLOCK, &set, &old) ||
        kill(getpid(), SIGUSR1) || sigpending(&pending) || sigismember(&pending, SIGUSR1) != 1 ||
        signals || sigprocmask(SIG_SETMASK, &old, 0) || signals != 1)
        return 15;
    pid_t parent = getpid();
    pid = fork();
    if (!pid) {
        struct timespec gap = {0, 80000000};
        if (nanosleep(&gap, 0) || kill(parent, SIGUSR1))
            _exit(43);
        _exit(0);
    }
    struct timespec sec = {2, 0}, rem = {0, 0};
    if (pid < 0 || nanosleep(&sec, &rem) != -1 || errno != EINTR || rem.tv_sec != 1 ||
        signals != 2 || waitpid(pid, &status, 0) != pid || status != 0)
        return 16;
    pthread_t cancelled;
    void *result;
    if (pipe(blocked_pipe) || pthread_create(&cancelled, 0, cancel_worker, 0))
        return 24;
    while (!atomic_load(&cancel_ready)) {
    }
    if (pthread_cancel(cancelled) || pthread_join(cancelled, &result) ||
        result != PTHREAD_CANCELED || !cleaned || close(blocked_pipe[0]) || close(blocked_pipe[1]))
        return 25;
    puts("musl static TLS pthreads condvar malloc stdio fork exec wait sleep signal OK");

    return 0;
}

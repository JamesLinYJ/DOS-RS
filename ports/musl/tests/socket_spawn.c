#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

#define CHECK(expression) do { if (!(expression)) { \
    fprintf(stderr, "socket/spawn line %d (errno %d): %s\n", __LINE__, errno, #expression); \
    return 1; \
} } while (0)

static volatile sig_atomic_t broken_pipes;
static void pipe_handler(int sig) { if (sig == SIGPIPE) ++broken_pipes; }

static int packet(int require_seqpipe)
{
    int s[2];
    char buf[8] = {0};
    struct stat sb;
    CHECK(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0, s) == 0);
    CHECK(fcntl(s[0], F_GETFD) == FD_CLOEXEC);
    CHECK(fstat(s[1], &sb) == 0 && S_ISSOCK(sb.st_mode));
    CHECK(recv(s[0], buf, sizeof buf, MSG_DONTWAIT) == -1 && errno == EAGAIN);
    CHECK(send(s[1], "first", 5, 0) == 5 && send(s[1], "two", 3, 0) == 3);
    CHECK(recv(s[0], buf, 2, MSG_PEEK) == 2 && !memcmp(buf, "fi", 2));
    CHECK(recv(s[0], buf, 2, MSG_TRUNC) == 5 && !memcmp(buf, "fi", 2));
    CHECK(read(s[0], buf, sizeof buf) == 3 && !memcmp(buf, "two", 3));
    CHECK(send(s[1], buf, 0, 0) == 0);
    CHECK(send(s[1], "x", 1, 0) == 1 && recv(s[0], buf, sizeof buf, 0) == 0);
    CHECK(recv(s[0], buf, sizeof buf, 0) == 1 && buf[0] == 'x');
    CHECK(write(s[0], "other", 5) == 5);
    CHECK(recv(s[1], buf, sizeof buf, 0) == 5 && !memcmp(buf, "other", 5));
    close(s[1]);
    struct pollfd p = {s[0], POLLIN, 0};
    CHECK(poll(&p, 1, 0) == 1 && (p.revents & (POLLHUP|POLLIN)) == (POLLHUP|POLLIN));
    CHECK(recv(s[0], buf, sizeof buf, 0) == 0);
    CHECK(send(s[0], buf, 1, MSG_NOSIGNAL) == -1 && errno == EPIPE && !broken_pipes);
    CHECK(send(s[0], buf, 1, 0) == -1 && errno == EPIPE);
    printf("SEQPACKET SIGPIPE %d\n", (int)broken_pipes);
    if (require_seqpipe) CHECK(broken_pipes == 1);
    broken_pipes = 0;
    close(s[0]);
    return 0;
}

static int stream(void)
{
    int s[2];
    char buf[1024];
    memset(buf, 'k', sizeof buf);
    CHECK(socketpair(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK, 0, s) == 0);
    CHECK((fcntl(s[1], F_GETFL) & (O_NONBLOCK | O_ACCMODE)) == (O_NONBLOCK | O_RDWR));
    size_t count = 0;
    ssize_t n;
    while ((n = write(s[0], buf, sizeof buf)) > 0) count += n;
    CHECK(count && n == -1 && errno == EAGAIN);
    struct pollfd out = {s[0], POLLOUT, 0};
    CHECK(!poll(&out, 1, 0));
    int copy = dup(s[0]);
    CHECK(copy >= 0 && close(s[0]) == 0);
    size_t got = 0;
    while (got < count) {
        n = read(s[1], buf, 700);
        CHECK(n > 0);
        for (ssize_t i = 0; i < n; ++i) CHECK(buf[i] == 'k');
        got += n;
    }
    CHECK(read(s[1], buf, 1) == -1 && errno == EAGAIN);
    out.fd = copy;
    CHECK(poll(&out, 1, 0) == 1 && (out.revents & POLLOUT));
    CHECK(close(copy) == 0 && read(s[1], buf, sizeof buf) == 0);
    CHECK(send(s[1], buf, 1, MSG_NOSIGNAL) == -1 && errno == EPIPE && !broken_pipes);
    CHECK(send(s[1], buf, 1, 0) == -1 && errno == EPIPE && broken_pipes == 1);
    close(s[1]);
    return 0;
}

static int blocking(void)
{
    int s[2], status;
    CHECK(socketpair(AF_UNIX, SOCK_STREAM, 0, s) == 0);
    pid_t pid = fork();
    CHECK(pid >= 0);
    if (!pid) {
        close(s[0]);
        usleep(20000);
        unsigned total = 0;
        char buf[1024];
        ssize_t n;
        while ((n = read(s[1], buf, sizeof buf)) > 0) {
            for (ssize_t i = 0; i < n; ++i) {
                if ((unsigned char)buf[i] != ((total + i) & 255)) _exit(71);
            }
            total += n;
        }
        if (n != 0 || total != 100000) _exit(72);
        close(s[1]);
        _exit(0);
    }
    close(s[1]);
    unsigned total = 0;
    unsigned char buf[4096];
    while (total < 100000) {
        size_t count = 100000 - total;
        if (count > sizeof buf) count = sizeof buf;
        for (size_t i = 0; i < count; ++i) buf[i] = (total + i) & 255;
        ssize_t n = write(s[0], buf, count);
        CHECK(n > 0);
        total += n;
    }
    close(s[0]);
    CHECK(waitpid(pid, &status, 0) == pid && WIFEXITED(status) && !WEXITSTATUS(status));
    return 0;
}

static int descriptors(void)
{
    int fds[64], n = 0, result[2] = {-11, -22};
    int channel[2];
    char dummy;
    CHECK(pipe(channel) == 0);
    CHECK(recv(channel[0], &dummy, 1, MSG_DONTWAIT) == -1 && errno == ENOTSOCK);
    CHECK(close(channel[0]) == 0 && close(channel[1]) == 0);
    while (n < (int)(sizeof fds / sizeof fds[0])) {
        int next = dup(STDOUT_FILENO);
        if (next < 0) break;
        fds[n++] = next;
    }
    if (n < (int)(sizeof fds / sizeof fds[0])) {
        CHECK(n > 0 && errno == EMFILE);
        CHECK(close(fds[--n]) == 0);
        CHECK(socketpair(AF_UNIX, SOCK_STREAM, 0, result) == -1 && errno == EMFILE);
        CHECK(result[0] == -11 && result[1] == -22);
        fds[n] = dup(STDOUT_FILENO);
        CHECK(fds[n++] >= 0);
    }
    while (n) CHECK(close(fds[--n]) == 0);
    return 0;
}

static int spawn(const char *self)
{
    int s[2], fd, status;
    pid_t pid;
    posix_spawn_file_actions_t actions;
    posix_spawnattr_t attr;
    sigset_t mask;
    char data[8];
    char *av[] = {"socktest", "child", NULL};
    char *env[] = {"SPAWN_PROBE=ok", NULL};
    CHECK(socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, s) == 0);
    CHECK(posix_spawn_file_actions_init(&actions) == 0);
    CHECK(posix_spawn_file_actions_adddup2(&actions, s[1], 20) == 0);
    CHECK(posix_spawn_file_actions_addclose(&actions, s[0]) == 0);
    CHECK(posix_spawn_file_actions_addopen(&actions, 21, "spawn.out",
                                          O_WRONLY|O_CREAT|O_TRUNC, 0600) == 0);
    CHECK(posix_spawnattr_init(&attr) == 0);
    sigemptyset(&mask);
    sigaddset(&mask, SIGUSR1);
    CHECK(posix_spawnattr_setsigmask(&attr, &mask) == 0);
    CHECK(posix_spawnattr_setpgroup(&attr, 0) == 0);
    CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETSIGMASK|POSIX_SPAWN_SETPGROUP) == 0);
    CHECK(posix_spawn(&pid, self, &actions, &attr, av, env) == 0);
    CHECK(close(s[1]) == 0);
    CHECK(send(s[0], "parent", 6, 0) == 6);
    CHECK(recv(s[0], data, sizeof data, 0) == 5 && !memcmp(data, "child", 5));
    CHECK(recv(s[0], data, sizeof data, 0) == 0);
    CHECK(waitpid(pid, &status, 0) == pid && WIFEXITED(status) && WEXITSTATUS(status) == 37);
    close(s[0]);
    posix_spawnattr_destroy(&attr);
    posix_spawn_file_actions_destroy(&actions);
    fd = open("spawn.out", O_RDONLY);
    CHECK(fd >= 0 && read(fd, data, sizeof data) == 6 && !memcmp(data, "parent", 6));
    close(fd);
    CHECK(posix_spawn(&pid, "missing.elf", NULL, NULL, av, env) == ENOENT);
    CHECK(waitpid(-1, &status, WNOHANG) == -1 && errno == ECHILD);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc == 2 && !strcmp(argv[1], "child")) {
        char buf[8];
        sigset_t mask;
        CHECK(getenv("SPAWN_PROBE") && !strcmp(getenv("SPAWN_PROBE"), "ok"));
        CHECK(getpid() == getpgrp());
        CHECK(sigprocmask(SIG_SETMASK, NULL, &mask) == 0 && sigismember(&mask, SIGUSR1));
        CHECK(recv(20, buf, sizeof buf, 0) == 6 && !memcmp(buf, "parent", 6));
        CHECK(write(21, buf, 6) == 6 && write(20, "child", 5) == 5);
        return 37;
    }
    struct sigaction action = {.sa_handler = pipe_handler};
    sigemptyset(&action.sa_mask);
    CHECK(sigaction(SIGPIPE, &action, NULL) == 0);
    CHECK(!packet(argc == 2 && !strcmp(argv[1], "issue8")) && !stream() && !blocking() && !descriptors() && !spawn("socktest.elf"));
    puts("local sockets and posix_spawn passed");
    return 0;
}

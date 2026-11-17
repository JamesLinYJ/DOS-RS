#include <sys/wait.h>
#include <unistd.h>

static __attribute__((noinline)) int deep(int fd)
{
    volatile unsigned char scratch[256 * 1024];
    if (read(fd, (void *)scratch, 1) != 1 || scratch[0] != 'S')
        return 1;
    for (unsigned i = 4096; i < sizeof scratch; i += 4096)
        scratch[i] = (unsigned char)(i / 4096);
    pid_t pid = fork();
    if (pid == 0) {
        for (unsigned i = 4096; i < sizeof scratch; i += 4096) {
            if (scratch[i] != (unsigned char)(i / 4096)) _exit(2);
            scratch[i] = 0;
        }
        _exit(31);
    }
    int status;
    if (pid < 0 || waitpid(pid, &status, 0) != pid || !WIFEXITED(status) || WEXITSTATUS(status) != 31)
        return 3;
    for (unsigned i = 4096; i < sizeof scratch; i += 4096)
        if (scratch[i] != (unsigned char)(i / 4096)) return 4;
    return 0;
}

static __attribute__((noinline)) int overflow(unsigned remaining)
{
    volatile unsigned char page[32768];
    page[0] = (unsigned char)remaining;
    if (remaining) {
        int n = overflow(remaining - 1);
        return n + page[0];
    }
    return page[0];
}

int main(void)
{
    int fds[2];
    if (pipe(fds) || write(fds[1], "S", 1) != 1 || close(fds[1])) return 5;
    if (deep(fds[0]) || close(fds[0])) return 6;
    pid_t pid = fork();
    if (!pid) _exit(overflow(270));
    int status;
    if (pid < 0 || waitpid(pid, &status, 0) != pid || !WIFSIGNALED(status) || WTERMSIG(status) != 11)
        return 7;
    static const char pass[] = "main stack: lazy 256 KiB, fork, limit and parent recovery OK\n";
    write(1, pass, sizeof pass - 1);
    return 0;
}

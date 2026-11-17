#define _GNU_SOURCE
#include <errno.h>
#include <string.h>
#include <sys/utsname.h>
#include <sys/wait.h>
#include <unistd.h>
#define CHECK(test) do { if (!(test)) return __LINE__; } while (0)
int main(void)
{
    struct utsname info;
    char host[65], maximum[65];
    CHECK(uname(&info) == 0 && gethostname(host, sizeof host) == 0);
    CHECK(!strcmp(info.nodename, "dos-rs") && !strcmp(host, info.nodename) && !strcmp(info.sysname, "DOS-RS"));
    CHECK(write(1, &info, sizeof info) == sizeof info);
    memset(maximum, 'x', sizeof maximum);
    CHECK(sethostname(maximum, 64) == 0 && gethostname(host, sizeof host) == 0 &&
          !memcmp(host, maximum, 64) && !host[64]);
    CHECK(sethostname(maximum, 65) == -1 && errno == EINVAL);
    CHECK(sethostname((void *)1, 10) == -1 && errno == EFAULT);
    CHECK(gethostname(host, sizeof host) == 0 && !memcmp(host, maximum, 64));
    CHECK(sethostname("guest-node", 10) == 0);
    pid_t pid = fork();
    CHECK(pid >= 0);
    if (!pid) {
        if (setuid(1001) || sethostname("different", 9) != -1 || errno != EPERM ||
            gethostname(host, sizeof host) || strcmp(host, "guest-node")) _exit(3);
        _exit(0);
    }
    int status;
    CHECK(waitpid(pid, &status, 0) == pid && WIFEXITED(status) && WEXITSTATUS(status) == 0);
    return 0;
}

// The p-suffixed exec functions run an executable text file through the shell
// when the native loader returns ENOEXEC. Plain execve keeps its original error.
#include <errno.h>
#include <stdint.h>
#include <stdlib.h>
#include <unistd.h>

hidden int __dos_exec_script(const char *path, char *const argv[], char *const envp[])
{
    // Empty PATH components can yield a bare filename beginning with '-'.
    size_t prefix = path[0] == '-' ? 3 : 2;
    size_t count = 0;
    while (argv[count]) {
        if (count >= SIZE_MAX / sizeof(char *) - prefix) {
            errno = E2BIG;
            return -1;
        }
        count++;
    }
    size_t tail = count ? count - 1 : 0;
    char **args = malloc((tail + prefix + 1) * sizeof *args);
    if (!args) return -1;
    args[0] = "sh";
    if (prefix == 3) args[1] = "--";
    args[prefix - 1] = (char *)path;
    for (size_t i = 0; i < tail; i++) args[i + prefix] = argv[i + 1];
    args[tail + prefix] = NULL;
    execve("/bin/sh", args, envp);
    if (errno == ENOENT)
        execve("/dos/a/bin/sh", args, envp);
    int error = errno;
    free(args);
    errno = error;
    return -1;
}

// Direct execve and PATH-searching exec functions have different ENOEXEC rules.
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void)
{
    static const char body[] =
        "case \"$0:$#:$1:$2:$SCRIPT_VALUE\" in\n"
        "'./plaincmd:2:two words::inherited') exit 29 ;;\n"
        "'./plaincmd:2:two words::explicit') exit 31 ;;\n"
        "*) exit 73 ;; esac\n";
    int fd = open("plaincmd", O_WRONLY | O_CREAT | O_TRUNC, 0755);
    if (fd < 0 || write(fd, body, sizeof body - 1) != sizeof body - 1 || close(fd)) {
        perror("prepare script");
        return 1;
    }
    // The runner supplies this directory on both FAT and Btrfs. Keep the exec
    // assertions independent of directory-creation support on a shared mount.
    struct stat directory;
    if (stat("execdir/plaincmd", &directory) || !S_ISDIR(directory.st_mode)) {
        fputs("exec: missing directory fixture execdir/plaincmd\n", stderr);
        return 8;
    }
    if (setenv("PATH", "execdir:.:/bin", 1) || setenv("SCRIPT_VALUE", "inherited", 1)) return 2;
    char *av[] = {"ignored argv0", "two words", "", NULL};
    char *ev[] = {"SCRIPT_VALUE=explicit", NULL};
    const char *directories[] = {".", "/", "./execdir/plaincmd", "./execdir/plaincmd/"};
    for (unsigned i = 0; i < sizeof directories / sizeof *directories; ++i) {
        if (execve(directories[i], av, ev) != -1 || errno != EACCES) {
            perror("direct execve directory expected EACCES");
            return 9;
        }
    }
    if (execve("./plaincmd", av, ev) != -1 || errno != ENOEXEC) {
        perror("direct execve expected ENOEXEC");
        return 3;
    }
    for (int variant = 0; variant < 4; variant++) {
        pid_t child = fork();
        if (child < 0) return 4;
        if (!child) {
            switch (variant) {
            case 0: execvp("plaincmd", av); break;
            case 1: execlp("./plaincmd", av[0], av[1], av[2], (char *)0); break;
            case 2: execvpe("plaincmd", av, ev); break;
            case 3: execlp("plaincmd", av[0], av[1], av[2], (char *)0); break;
            }
            _exit(99);
        }
        int status;
        if (waitpid(child, &status, 0) != child || !WIFEXITED(status)
            || WEXITSTATUS(status) != (variant == 2 ? 31 : 29)) {
            fprintf(stderr, "script variant %d failed\n", variant);
            return 5;
        }
    }
    fd = open("-c", O_WRONLY | O_CREAT | O_TRUNC, 0755);
    static const char dash_body[] = "case \"$0:$1\" in '-c:arg') exit 41 ;; *) exit 74 ;; esac\n";
    if (fd < 0 || write(fd, dash_body, sizeof dash_body - 1) != sizeof dash_body - 1 || close(fd)) return 6;
    pid_t child = fork();
    if (!child) {
        if (!setenv("PATH", ":/bin", 1))
            execlp("-c", "ignored", "arg", (char *)0);
        _exit(99);
    }
    int status;
    if (child < 0 || waitpid(child, &status, 0) != child || !WIFEXITED(status) || WEXITSTATUS(status) != 41) {
        fputs("option-like script name failed\n", stderr);
        return 7;
    }
    puts("exec: ENOEXEC, PATH, argv and environment passed");
    puts("exec: directories rejected with EACCES; PATH skipped directories");
    return 0;
}

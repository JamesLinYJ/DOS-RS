/* Read a bounded argv fixture and execute each tool with isolated stdio files.
 * The host checks the files on the stopped image using the recorded wait status. */
#include <errno.h>
#include <fcntl.h>
#include <sys/wait.h>
#include <sys/stat.h>
#include <unistd.h>

static int exact(int fd, char *p, int n) {
    while (n) {
        int r = read(fd, p, n);
        if (r <= 0) return -1;
        p += r;
        n -= r;
    }
    return 0;
}
static int string(int fd, char *p, int n) {
    while (n-- > 0) {
        if (exact(fd, p, 1)) return -1;
        if (!*p++) return 0;
    }
    return -1;
}
static int redirect_to(const char *name, int dst, int append) {
    int fd = open(name, dst ? O_CREAT | (append ? O_APPEND : O_TRUNC) | O_WRONLY : O_RDONLY, 0600);
    if (fd < 0 || dup2(fd, dst) != dst) return -1;
    return fd == dst ? 0 : close(fd);
}
int main(void) {
    umask(022);
    int cfg = open("cases.bin", O_RDONLY);
    int results = open("status.bin", O_CREAT | O_TRUNC | O_WRONLY, 0600);
    int signals = open("signals.bin", O_CREAT | O_TRUNC | O_WRONLY, 0600);
    if (cfg < 0 || results < 0 || signals < 0) return 101;
    for (int idx = 0; idx < 1000; idx++) {
        unsigned char count;
        if (exact(cfg, (char *)&count, 1) || count > 24) return 102;
        if (!count) {
            if (close(signals) || close(results) || close(cfg)) return 103;
            return write(1, "coreutils cases finished\n", 25) == 25 ? 0 : 104;
        }
        unsigned char append;
        if (exact(cfg, (char *)&append, 1)) return 105;
        char input[32], locale[32], values[24][160], path[100], *args[25];
        if (string(cfg, input, sizeof input) || string(cfg, locale, sizeof locale) || string(cfg, path, sizeof path)) return 105;
        for (int i = 0; i < count; i++) {
            if (string(cfg, values[i], sizeof values[i])) return 106;
            args[i] = values[i];
        }
        args[count] = 0;
        char out[] = "q000.out", err[] = "q000.err";
        out[1] = err[1] = '0' + idx / 100;
        out[2] = err[2] = '0' + idx / 10 % 10;
        out[3] = err[3] = '0' + idx % 10;
        int child = fork();
        if (child < 0) return 107;
        if (!child) {
            char *env[] = {locale, "LANG=C", "TZ=UTC", "PATH=/bin", 0};
            close(cfg); close(results); close(signals);
            if (redirect_to(input, 0, 0) || redirect_to(out, 1, append) || redirect_to(err, 2, 0)) _exit(108);
            execve(path, args, env);
            _exit(109);
        }
        int status;
        if (waitpid(child, &status, 0) != child) return 110;
        unsigned char signal = 0;
        unsigned char code;
        if (WIFEXITED(status)) {
            code = WEXITSTATUS(status);
        } else if (WIFSIGNALED(status)) {
            signal = WTERMSIG(status);
            code = 128 + signal;
        } else {
            return 110;
        }
        if (code == 0 && count == 3 && values[0][0] == 'l' && values[0][1] == 'i' &&
            values[0][2] == 'n' && values[0][3] == 'k' && !values[0][4]) {
            struct stat a, b;
            if (stat(values[1], &a) || stat(values[2], &b) || a.st_nlink < 2 ||
                a.st_dev != b.st_dev || a.st_ino != b.st_ino || a.st_nlink != b.st_nlink) return 113;
        }
        if (write(results, &code, 1) != 1 || write(signals, &signal, 1) != 1) return 111;
    }
    return 112;
}

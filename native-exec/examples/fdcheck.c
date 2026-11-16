#include <errno.h>
#include <fcntl.h>
#include <unistd.h>

int main(void) {
    const char *name = "gone.tmp";
    char data[2];
    int old = open(name, O_CREAT | O_EXCL | O_RDWR, 0600);
    if (old < 0 || write(old, "ab", 2) != 2)
        return 1;
    int alias = dup(old);
    if (alias < 0 || unlink(name) || open(name, O_RDONLY) != -1 || errno != ENOENT)
        return 2;
    int replacement = open(name, O_CREAT | O_EXCL | O_RDWR, 0600);
    if (replacement < 0 || write(replacement, "x", 1) != 1)
        return 3;
    if (lseek(alias, 0, SEEK_SET) != 0 || read(old, data, 1) != 1 || data[0] != 'a')
        return 4;
    if (close(old) || read(alias, data, 2) != 1 || data[0] != 'b')
        return 5;
    if (lseek(replacement, 0, SEEK_SET) != 0 || read(replacement, data, 2) != 1 || data[0] != 'x')
        return 6;
    if (close(alias) || close(replacement) || unlink(name))
        return 7;
    int ends[2];
    if (pipe(ends) || dup2(ends[1], 9) != 9 || close(ends[1]))
        return 8;
    if (write(9, "p", 1) != 1 || close(9) || read(ends[0], data, 1) != 1 || data[0] != 'p')
        return 9;
    if (read(ends[0], data, 1) != 0 || close(ends[0]))
        return 10;
    const char ok[] = "unlink, shared offset, pipe EOF OK\n";
    return write(1, ok, sizeof ok - 1) != sizeof ok - 1;
}

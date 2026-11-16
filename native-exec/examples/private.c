#include <unistd.h>

static volatile int owner;
int main(int argc, char **argv) {
    if (argc != 2 || owner != 0)
        return 1;
    int pid = owner = getpid();
    if (pid <= 0)
        return 2;
    if (argv[1][0] == 's') {
        if (write(1, &pid, sizeof pid) != sizeof pid)
            return 3;
    } else {
        int sender;
        if (read(0, &sender, sizeof sender) != sizeof sender || sender == pid)
            return 4;
        if (read(0, &sender, 1) != 0 || owner != pid)
            return 5;
        const char ok[] = "private BSS and distinct PIDs OK\n";
        if (write(1, ok, sizeof ok - 1) != sizeof ok - 1)
            return 6;
    }
    return owner != pid;
}

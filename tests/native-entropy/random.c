#define _GNU_SOURCE
#include <errno.h>
#include <string.h>
#include <sys/random.h>
#include <unistd.h>
int main(int argc, char **argv)
{
    if (argc != 2) return 1;
    unsigned char first[256], second[256];
    if (getrandom(NULL, 0, 0) != 0) return 2;
    if (getrandom(first, 1, 0x20) != -1 || errno != EINVAL) return 3;
    if (getrandom((void *)1, sizeof first, 0) != -1 || errno != EFAULT) return 4;
    if (!strcmp(argv[1], "absent")) {
        if (getrandom(first, sizeof first, GRND_NONBLOCK) != -1 || errno != ENODEV) return 5;
        return write(1, "ENODEV\n", 7) != 7;
    }
    if (getrandom(second, 1, 0) != 1 || getrandom(first, sizeof first, 0) != sizeof first ||
        getentropy(second, sizeof second)) return 6;
    if (memcmp(first, second, sizeof first) == 0) return 7;
    if (getentropy(first, sizeof first + 1) != -1 || errno != EIO) return 8;
    return write(1, first, sizeof first) != sizeof first || write(1, second, sizeof second) != sizeof second;
}

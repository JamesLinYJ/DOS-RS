#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 2 || argv[1][0] != '-' || argv[1][1] != 'c' || argv[1][2])
        return 2;
    char block[3072], digits[21];
    uint64_t total = 0;
    ssize_t n;
    while ((n = read(STDIN_FILENO, block, sizeof block)) > 0)
        total += n;
    if (n < 0)
        return 1;
    size_t i = sizeof digits;
    digits[--i] = '\n';
    do {
        digits[--i] = '0' + total % 10;
        total /= 10;
    } while (total);
    return write(STDOUT_FILENO, digits + i, sizeof digits - i) != (ssize_t)(sizeof digits - i);
}

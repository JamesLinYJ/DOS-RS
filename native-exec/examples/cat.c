#include <fcntl.h>
#include <unistd.h>

int main(int argc, char **argv) {
    char block[8192];
    int fd = argc == 1 ? STDIN_FILENO : open(argv[1], O_RDONLY);
    if (argc > 2 || fd < 0)
        return 1;
    ssize_t count;
    while ((count = read(fd, block, sizeof block)) > 0) {
        ssize_t sent = 0;
        while (sent < count) {
            ssize_t n = write(STDOUT_FILENO, block + sent, count - sent);
            if (n <= 0)
                return 1;
            sent += n;
        }
    }
    return count < 0 || close(fd) < 0;
}

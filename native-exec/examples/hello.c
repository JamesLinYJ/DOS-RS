#include <unistd.h>
int main(int argc, char **argv) {
    if (argc != 2 || argv[1][0] != '7' || argv[2] != 0 || getpid() <= 0)
        return 1;
    static const char text[] = "hello from native C\n";
    return write(STDOUT_FILENO, text, sizeof text - 1) == sizeof text - 1 ? 0 : 2;
}

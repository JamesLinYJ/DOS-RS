#include <unistd.h>

static volatile unsigned char bss[1100 * 1024];

int main(void) {
    if (bss[sizeof bss - 1] != 0)
        return 1;
    bss[0] = 42;
    const char text[] = "large image loaded\n";
    return write(1, text, sizeof text - 1) != sizeof text - 1;
}

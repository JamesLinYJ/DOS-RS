#include <unistd.h>
int main(void) {
    const char started[] = "CPU loop started\n";
    if (write(1, started, sizeof started - 1) != sizeof started - 1)
        return 1;
    for (;;) {
    }
}

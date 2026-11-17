#include <stdint.h>

/* The test loader maps its first anonymous page here before starting the AP. */
int main(void) {
    volatile unsigned char *ready = (void *)(uintptr_t)0x61000000u;
    *ready = 0x5a;
    for (;;) {}
}

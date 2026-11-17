#include <errno.h>
#include <unistd.h>

// Link the same binary into both processes at identical high VAs.
static int owner, sender;
static char unexecutable[] = { (char)0xc3 }; // RET would succeed if NX were missing.
static const char ok[] = "high VA exec, private data and syscall buffers OK\n";

int main(int argc, char **argv) {
    if (argc != 2 || owner != 0 || (unsigned long)main < (1UL << 32)
        || (unsigned long)&owner < (1UL << 46))
        return 1;
    switch (argv[1][0]) {
    case 'w':
        *(volatile char *)ok = 'x';  // Read-only user leaf must fault.
        return 2;
    case 'e':
        ((void (*)(void))unexecutable)();
        return 3;
    case 'h':
        // This text address belongs only to the separately linked highva.elf.
        return *(volatile unsigned char *)0x7f0000001000UL;
    case 's':
    case 'r':
        break;
    default:
        return 4;
    }
    if (write(1, (void *)(1UL << 47), 1) != -1 || errno != EFAULT)
        return 5;
    owner = getpid();
    if (owner <= 0)
        return 6;
    if (argv[1][0] == 's')
        return write(1, &owner, sizeof owner) != sizeof owner;
    if (read(0, &sender, sizeof sender) != sizeof sender || sender == owner
        || owner != getpid() || read(0, &sender, 1) != 0)
        return 7;
    return write(1, ok, sizeof ok - 1) != sizeof ok - 1;
}

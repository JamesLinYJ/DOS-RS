#include <stdint.h>
#include <unistd.h>
#include <errno.h>
#include <dos_memory.h>

static _Alignas(4096) volatile unsigned char data[5*1024*1024];

int main(void) {
    if (dos_unmap_pages((void *)data, 1) != -1 || errno != EINVAL) return 1;
    for (unsigned int i=0; i<sizeof data; i+=4096) {
        if (data[i]) return 2;
        data[i]=(unsigned char)(1+i/4096);
    }
    unsigned char *anon=dos_map_pages(2);
    if (!anon || (uintptr_t)anon<(uintptr_t)(data+sizeof data)) return 3;
    if (*anon || data[sizeof data-4096]!=(unsigned char)(sizeof data/4096)) return 4;
    if (dos_unmap_pages(anon,2)) return 5;
    const char ok[]="5 MiB ELF and disjoint anonymous mappings OK\n";
    return write(1,ok,sizeof ok-1)!=(sizeof ok-1);
}

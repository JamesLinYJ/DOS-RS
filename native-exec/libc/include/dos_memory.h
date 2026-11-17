#ifndef DOSRS_MEMORY_H
#define DOSRS_MEMORY_H
#include <stddef.h>
#define DOS_PAGE_SIZE 4096u
struct dos_meminfo {
    size_t total_pages;
    size_t free_pages;
    size_t process_pages;
};
/* Units are pages. A failed allocation returns NULL and sets errno. */
void *dos_map_pages(size_t pages);
int dos_unmap_pages(void *base, size_t pages);
int dos_meminfo(struct dos_meminfo *info);
#endif

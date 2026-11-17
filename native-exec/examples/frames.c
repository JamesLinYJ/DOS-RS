/* Validate user mappings against both the process and firmware frame counts. */
#include <dos_memory.h>
#include <dos_thread.h>
#include <errno.h>
#include <fcntl.h>
#include <stdatomic.h>
#include <stdint.h>
#include <unistd.h>

static unsigned char worker_stack[16384] __attribute__((aligned(16)));
static _Atomic uint32_t visited, released;
static volatile unsigned char *watched;

static int read_retired_page(void *unused) {
    (void)unused;
    if (watched[0] != 65) _exit(30);
    atomic_store(&visited, 1);
    while (!atomic_load(&released)) {
        if (dos_futex_wait((uint32_t *)&released, 0) < 0 && errno != EAGAIN) _exit(31);
    }
    volatile unsigned char value = watched[0];
    (void)value;
    _exit(32);
}

static int retired_page(void) {
    watched = dos_map_pages(1);
    if (!watched) return 33;
    watched[0] = 65;
    int tid = dos_thread_create(read_retired_page, NULL, worker_stack, sizeof worker_stack);
    if (tid < 0) return 34;
    while (!atomic_load(&visited)) {}
    if (dos_unmap_pages((void *)watched, 1)) return 35;
    atomic_store(&released, 1);
    dos_futex_wake((uint32_t *)&released, 1);
    dos_thread_join(tid, NULL);
    return 36;
}

static int write_record(const char *file, uint32_t *record, size_t length) {
    int fd = open(file, O_CREAT | O_TRUNC | O_WRONLY, 0600);
    if (fd < 0 || write(fd, record, length) != (ssize_t)length) return -1;
    return close(fd);
}

int main(int argc, char **argv) {
    if (argc != 2) return 1;
    if (argv[1][0] == 's') return retired_page();
    struct dos_meminfo initial, current;
    if (dos_meminfo(&initial) || initial.free_pages < 513) return 2;
    uint32_t record[] = {initial.total_pages, initial.free_pages, initial.process_pages, 0, 0};
    if (dos_map_pages(0) || errno != EINVAL) return 3;
    if (dos_map_pages(initial.free_pages + 1) || errno != ENOMEM) return 4;
    if (dos_meminfo(&current) || current.free_pages != initial.free_pages) return 5;
    volatile unsigned char *p = dos_map_pages(initial.free_pages);
    if (!p) return 6;
    if (dos_meminfo(&current) || current.free_pages != 0 ||
        current.process_pages != initial.process_pages + initial.free_pages) return 7;
    for (size_t i = 0; i < initial.free_pages; ++i) {
        if (p[i * DOS_PAGE_SIZE] != 0) return 8;
        p[i * DOS_PAGE_SIZE] = (unsigned char)(i + 1);
    }
    if (dos_map_pages(1) || errno != ENOMEM) return 9;
    for (size_t i = 0; i < initial.free_pages; ++i)
        if (p[i * DOS_PAGE_SIZE] != (unsigned char)(i + 1)) return 10;
    record[3] = current.process_pages;
    if (dos_unmap_pages((void *)(p + DOS_PAGE_SIZE), 1)) return 11;
    if (dos_unmap_pages((void *)(p + DOS_PAGE_SIZE), 1) != -1 || errno != EINVAL) return 12;
    unsigned char *reused = dos_map_pages(1);
    if (reused != p + DOS_PAGE_SIZE || *reused != 0) return 13;
    if (dos_meminfo(&current) || current.free_pages != 0) return 14;
    record[4] = current.free_pages;
    if (write_record(argv[1][0] == 'a' ? "again.bin" : "first.bin", record, sizeof record)) return 15;
    static const char ok[] = "all physical frames used, ENOMEM and zeroed reuse OK\n";
    return write(1, ok, sizeof ok - 1) != sizeof ok - 1;
}

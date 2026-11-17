#include <cpuid.h>
#include <dos_thread.h>
#include <errno.h>
#include <fcntl.h>
#include <stdatomic.h>
#include <unistd.h>

#define SAMPLES 512
#define ITERATIONS 65536
#define STACK_BYTES 16384

/* Fixed-width records shared with the host test on both x86 ABIs. */
struct sample {
    uint64_t start, end;
    uint32_t first_cpu, last_cpu;
};
struct worker_result {
    uint32_t tid, pid, checksum, error;
    double floating_sum;
    struct sample samples[SAMPLES];
};
_Static_assert(sizeof(struct sample) == 24, "sample layout");
_Static_assert(sizeof(struct worker_result) == 24 + SAMPLES * 24, "worker layout");

static _Alignas(16) unsigned char stacks[2][STACK_BYTES];
static struct worker_result results[2];
static int *error_slots[2];
static uint32_t park[2], gate, arrived;
static volatile uint32_t private_word;
static volatile unsigned divisor;

static void output(int fd, const void *data, size_t size) {
    const unsigned char *p = data;
    while (size) {
        ssize_t n = write(fd, p, size);
        if (n <= 0) _exit(70);
        p += n;
        size -= n;
    }
}

static void stamp(uint64_t *time, uint32_t *cpu) {
    unsigned a, b, c, d;
    atomic_signal_fence(memory_order_seq_cst);
    __cpuid(1, a, b, c, d);
    *time = __builtin_ia32_rdtsc();
    *cpu = b >> 24;
    atomic_signal_fence(memory_order_seq_cst);
}

static int worker(void *argument) {
    uintptr_t i = (uintptr_t)argument;
    struct worker_result *r = &results[i];
    r->tid = dos_gettid();
    r->pid = getpid();
    if (i == 0) {
        if (close(-1) != -1 || errno != EBADF) _exit(21);
    } else if (dos_thread_join(r->tid, 0) != -1 || errno != EDEADLK) {
        _exit(22);
    }
    error_slots[i] = &errno;
    /* Each park word stays zero. Only an actual wake can release this wait. */
    if (dos_futex_wait(&park[i], 0) != 0) _exit(23);
    r->error = errno;
    while (!__atomic_load_n(&gate, __ATOMIC_ACQUIRE)) {
        if (dos_futex_wait(&gate, 0) < 0 && errno != EAGAIN) _exit(24);
    }
    /* Let both workers reach user mode before measuring. With one CPU this
     * rendezvous also requires the timer to preempt the first worker. */
    __atomic_add_fetch(&arrived, 1, __ATOMIC_ACQ_REL);
    while (__atomic_load_n(&arrived, __ATOMIC_ACQUIRE) != 2) { }
    uint32_t value = 1 + i;
    volatile double increment = i == 0 ? 0.5 : 1.75;
    double total = 0;
    for (unsigned s = 0; s < SAMPLES; ++s) {
        struct sample *sample = &r->samples[s];
        stamp(&sample->start, &sample->first_cpu);
        for (unsigned n = 0; n < ITERATIONS; ++n) {
            value ^= value << 13;
            value ^= value >> 17;
            value ^= value << 5;
            total += increment;
        }
        /* Publish the result before the ending hardware sample. */
        r->checksum = value;
        r->floating_sum = total;
        stamp(&sample->end, &sample->last_cpu);
    }
    return 40 + i;
}

static int parallel(void) {
    int pid = getpid(), parent = dos_gettid(), tids[2];
    if (parent < 0 || pid <= 0) return 10;
    for (uintptr_t i = 0; i < 2; ++i) {
        tids[i] = dos_thread_create(worker, (void *)i, stacks[i], sizeof stacks[i]);
        if (tids[i] <= 0) return 11;
    }
    for (unsigned i = 0; i < 2; ++i) {
        int woke;
        do {
            woke = dos_futex_wake(&park[i], 1);
            if (woke < 0) return 12;
        } while (!woke);
    }
    __atomic_store_n(&gate, 1, __ATOMIC_RELEASE);
    if (dos_futex_wake(&gate, 2) < 0) return 13;
    for (unsigned i = 0; i < 2; ++i) {
        int code;
        if (dos_thread_join(tids[i], &code) || code != 40 + (int)i) return 14;
        if (results[i].tid != (uint32_t)tids[i] || results[i].pid != (uint32_t)pid) return 15;
        if (results[i].error != (i ? EDEADLK : EBADF)) return 16;
    }
    if (tids[0] == tids[1] || tids[0] == parent || tids[1] == parent ||
        error_slots[0] == error_slots[1] || error_slots[0] == &errno || error_slots[1] == &errno)
        return 17;
    const uint32_t header[] = {0x31524854, pid, parent, SAMPLES, ITERATIONS, 2};
    output(1, header, sizeof header);
    output(1, results, sizeof results);
    return 0;
}

struct owner { uintptr_t address, pid; };
static int private_page(int sender) {
    if (private_word) return 31;
    struct owner info = {(uintptr_t)&private_word, getpid()};
    if (sender) {
        private_word = 0x12345678;
        output(1, &info, sizeof info);
    } else {
        struct owner other;
        if (read(0, &other, sizeof other) != sizeof other || other.pid == info.pid ||
            other.address != info.address || private_word != 0) return 32;
        private_word = 0xdeadbeef;
        static const char ok[] = "same virtual address, distinct process pages\n";
        output(1, ok, sizeof ok - 1);
    }
    return 0;
}

static int witness(void) {
    static const char ready[] = "witness waiting for input\n";
    output(1, ready, sizeof ready - 1);
    unsigned char data[32];
    size_t n = 0;
    do {
        ssize_t got = read(0, data + n, sizeof data - n);
        if (got <= 0) return 33;
        n += got;
    } while (n < sizeof data && data[n - 1] != '\n');
    int fd = open("live.txt", O_WRONLY | O_CREAT | O_TRUNC);
    if (fd < 0) return 34;
    output(fd, data, n);
    return close(fd) != 0;
}

static int faulting_worker(void *unused) {
    (void)unused;
    int woke;
    do {
        woke = dos_futex_wake(&park[0], 1);
        if (woke < 0) _exit(35);
    } while (!woke);
    /* The runtime divisor forces a real ring-3 arithmetic exception. */
    return 123u / divisor;
}

int main(int argc, char **argv) {
    if (argc == 1) return parallel();
    if (argc != 2) return 36;
    switch (argv[1][0]) {
        case 's': return private_page(1);
        case 'r': return private_page(0);
        case 'w': return witness();
        case 'f':
            if (dos_thread_create(faulting_worker, 0, stacks[0], sizeof stacks[0]) <= 0) return 37;
            if (dos_futex_wait(&park[0], 0) < 0) return 38;
            /* Successful process teardown also removes this sleeping main thread. */
            dos_futex_wait(&park[1], 0);
            return 39;
        default: return 38;
    }
}

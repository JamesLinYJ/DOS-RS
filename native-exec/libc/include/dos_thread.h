#ifndef DOSRS_THREAD_H
#define DOSRS_THREAD_H
#include <stddef.h>
#include <stdint.h>

/* The caller retains the writable, 16-byte-aligned stack until join.
 * A worker returns a nonnegative exit status or calls dos_thread_exit. */
int dos_thread_create(int (*function)(void *), void *argument, void *stack, size_t size);
int dos_thread_join(int tid, int *status);
_Noreturn void dos_thread_exit(int status);
int dos_gettid(void);
int dos_futex_wait(uint32_t *address, uint32_t expected);
int dos_futex_wake(uint32_t *address, unsigned int count);
#endif

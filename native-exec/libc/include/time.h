#ifndef DOSRS_TIME_H
#define DOSRS_TIME_H
#include <stdint.h>
struct timespec {
    int64_t tv_sec, tv_nsec;
};
#endif

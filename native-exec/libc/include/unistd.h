#ifndef DOSRS_UNISTD_H
#define DOSRS_UNISTD_H
#include <stddef.h>
#include <stdint.h>
typedef intptr_t ssize_t;
typedef int64_t off_t;
#define STDIN_FILENO 0
#define STDOUT_FILENO 1
#define STDERR_FILENO 2
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
ssize_t read(int, void *, size_t);
ssize_t write(int, const void *, size_t);
int close(int);
int unlink(const char *);
int dup(int);
int dup2(int, int);
int pipe(int[2]);
int getpid(void);
unsigned int getuid(void);
unsigned int getgid(void);
off_t lseek(int, off_t, int);
_Noreturn void _exit(int status);
#endif

#ifndef DOSRS_SYS_WAIT_H
#define DOSRS_SYS_WAIT_H
#include <unistd.h>
#define WNOHANG 1
#define WIFEXITED(s) (((s) & 0x7f) == 0)
#define WEXITSTATUS(s) (((s) >> 8) & 255)
#define WIFSIGNALED(s) (((s) & 0x7f) != 0 && ((s) & 0x7f) != 0x7f)
#define WTERMSIG(s) ((s) & 0x7f)
pid_t waitpid(pid_t, int *, int);
pid_t wait(int *);
#endif

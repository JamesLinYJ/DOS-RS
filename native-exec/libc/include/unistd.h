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
int chdir(const char *);
char *getcwd(char *, size_t);
int rmdir(const char *);
int link(const char *, const char *);
int symlink(const char *, const char *);
ssize_t readlink(const char *, char *, size_t);
int rename(const char *,const char *);
int dup(int);
int dup2(int, int);
int pipe(int[2]);
typedef int pid_t;
pid_t getpid(void);
pid_t fork(void);
int execve(const char *, char *const [], char *const []);
pid_t getppid(void);
pid_t getpgid(pid_t);
int setpgid(pid_t, pid_t);
pid_t getsid(pid_t);
pid_t setsid(void);
unsigned int getuid(void);
int setuid(unsigned int);
int setgid(unsigned int);
int chown(const char *,unsigned int,unsigned int);
unsigned int getgid(void);
off_t lseek(int, off_t, int);
_Noreturn void _exit(int status);
#endif

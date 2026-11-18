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
int pipe2(int[2],int);
int dup3(int,int,int);
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
int unlinkat(int,const char *,int);
int renameat(int,const char *,int,const char *);
int linkat(int,const char *,int,const char *,int);
int symlinkat(const char *,int,const char *);
ssize_t readlinkat(int,const char *,char *,size_t);
int ftruncate(int,off_t);
int truncate(const char *,off_t);
int fsync(int);
int fchown(int,unsigned int,unsigned int);
#define F_OK 0
#define X_OK 1
#define W_OK 2
#define R_OK 4
int access(const char *,int);
int fchdir(int);
int faccessat(int,const char *,int,int);
int fchownat(int,const char *,unsigned int,unsigned int,int);
int lchown(const char *,unsigned int,unsigned int);
ssize_t pread(int,void *,size_t,off_t);
ssize_t pwrite(int,const void *,size_t,off_t);
_Noreturn void _exit(int status);
#endif

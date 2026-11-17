#include <stdint.h>
#include <errno.h>
#include <sys/wait.h>
#include <unistd.h>

extern long sys_sigaction(int, const void*, void*), sys_kill(int,int);
extern long sys_sigmask(int,const uint64_t*,uint64_t*), sys_sigpending(uint64_t*);
extern long sys_sigsuspend(const uint64_t*);
extern _Noreturn void sys_sigreturn(void);
struct action {uint64_t handler, restorer, mask; uint32_t flags, reserved;};
static volatile int handled;
static void receive(int signal) { handled = signal; }
static _Noreturn void resume(void) {sys_sigreturn();}
#define TEST(x) do { if (!(x)) {write(2,"signal failure " #x "\n",sizeof("signal failure " #x "\n")-1); return 1;} } while(0)
int main(void) {
    struct action a={(uintptr_t)receive,(uintptr_t)resume,0,0,0}, replaced={0};
    uint64_t bit=1ull<<9, empty=0,pending=0, previous=99;
    TEST(sys_sigaction(10,&a,&replaced)==0 && replaced.handler==0);
    TEST(sys_sigmask(0,&bit,&previous)==0 && previous==0);
    TEST(sys_kill(getpid(),10)==0 && !handled);
    TEST(sys_sigpending(&pending)==0 && (pending&bit));
    TEST(sys_sigsuspend(&empty)==-4 && handled==10);
    TEST(sys_sigmask(2,&empty,&previous)==0 && previous==bit);
    handled=0;
    TEST(sys_kill(getpid(),10)==0 && handled==10);
    int fds[2]; TEST(pipe(fds)==0);
    int child=fork(), status=0; TEST(child>=0);
    if(!child) {
        char byte; close(fds[1]);
        read(fds[0],&byte,1);
        return 99;
    }
    close(fds[0]);
    TEST(sys_kill(child,19)==0);
    TEST(waitpid(child,&status,2)==child && (status&255)==127 && (status>>8)==19);
    TEST(sys_kill(child,18)==0);
    TEST(waitpid(child,&status,8)==child && status==0xffff);
    TEST(sys_kill(child,15)==0);
    TEST(waitpid(child,&status,0)==child && WIFSIGNALED(status) && WTERMSIG(status)==15);
    close(fds[1]);
    write(1,"signal delivery stop continue wait OK\n",38);
    return 0;
}

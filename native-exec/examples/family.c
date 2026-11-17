#include <errno.h>
#include <fcntl.h>
#include <sys/wait.h>
#include <unistd.h>

static int private_value = 15;
static int same(const char *a, const char *b) {
    while (*a && *a == *b) { a++; b++; }
    return *a == *b;
}
static int check(int condition, int line) {
    if (!condition) {
        char message[] = "family: failed at line 000\n";
        message[23] = '0' + line / 100; message[24] = '0' + line / 10 % 10;
        message[25] = '0' + line % 10;
        write(2, message, sizeof(message)-1);
    }
    return condition;
}
#define EXPECT(c) do { if (!check((c), __LINE__)) return 1; } while (0)

int main(int argc, char **argv) {
    if (argc == 3 && same(argv[1], "replace")) {
        EXPECT((unsigned char)argv[2][0] == 0xfe && argv[2][1] == 0);
        EXPECT(same(argv[argc+1], "PATH=/bin") && same(argv[argc+2], "FAMILY_ENV=child"));
        EXPECT(private_value == 15 && getppid() > 0);
        EXPECT(write(1, "exec", 4) == 4);
        return 37;
    }
    int out = open("family.out", O_CREAT | O_TRUNC | O_RDWR, 0600), gates[2], status = -1;
    EXPECT(out >= 3 && pipe(gates) == 0);
    EXPECT(write(out, "parent-", 7) == 7);
    int parent = getpid(), id = fork();
    EXPECT(id >= 0);
    if (id == 0) {
        char ch; private_value = 67;
        EXPECT(getppid() == parent && close(gates[1]) == 0);
        EXPECT(read(gates[0], &ch, 1) == 1 && ch == 'K' && close(gates[0]) == 0);
        EXPECT(getpgid(0) == getpid() && getsid(0) == getsid(parent));
        EXPECT(dup2(out, 1) == 1 && close(out) == 0);
        char raw[] = {(char)0xfe, 0};
        char *args[] = {"family", "replace", raw, 0};
        char *env[] = {"PATH=/bin", "FAMILY_ENV=child", 0};
        EXPECT(execve("absent.elf", args, env) == -1 && errno == ENOENT && private_value == 67);
        execve("family.elf", args, env);
        return 110;
    }
    EXPECT(id != parent && private_value == 15 && close(gates[0]) == 0);
    EXPECT(waitpid(id, &status, WNOHANG) == 0 && status == -1);
    EXPECT(setpgid(id, id) == 0 && getpgid(id) == id);
    EXPECT(write(gates[1], "K", 1) == 1 && close(gates[1]) == 0);
    EXPECT(waitpid(-id, &status, 0) == id && WIFEXITED(status) && WEXITSTATUS(status) == 37);
    EXPECT(waitpid(id, 0, 0) == -1 && errno == 10 && private_value == 15);
    EXPECT(write(out, "-done", 5) == 5 && close(out) == 0);
    write(1, "fork exec wait OK\n", 18);
    return 0;
}

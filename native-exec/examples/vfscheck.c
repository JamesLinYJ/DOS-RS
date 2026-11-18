#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <sys/stat.h>
#include <unistd.h>

extern long sys_getdents(int, void *, size_t);
static int same(const char *a, const char *b, size_t n) {
    for (size_t i = 0; i < n; ++i) if (a[i] != b[i]) return 0;
    return 1;
}
static int touch(const char *path, const char *bytes, size_t n) {
    int fd = open(path, O_CREAT | O_WRONLY | O_EXCL, 0666);
    if (fd < 0) return -1;
    int ok = write(fd, bytes, n) == (ssize_t)n;
    return close(fd) == 0 && ok ? 0 : -1;
}
#define CHECK(n, expression) do { if (!(expression)) { const char m[]="VFS failure " #n "\n"; write(2,m,sizeof m-1); return n; } } while (0)

int main(int argc, char **argv) {
    char data[128]; struct stat old, newer;
    if (argc > 1 && argv[1][0] == 'c') {
        CHECK(2, stat("/work/result", &old) == 0 && old.st_nlink == 2 && (old.st_mode & 0777) == 0640);
        int fd = open("/work/view", O_RDONLY);
        CHECK(3, fd >= 0 && read(fd, data, 7) == 7 && same(data, "newdata", 7));
        return write(1, "cold root OK\n", 13) != 13;
    }
    write(1, "namespace\n", 10);
    CHECK(4, umask(0027) == 0022);
    CHECK(5, mkdir("/work", 0777) == 0 && chdir("/work") == 0);
    CHECK(6, getcwd(data, sizeof data) && same(data, "/work", 6));
    CHECK(7, touch("result", "old!", 4) == 0);
    int fd = open("result", O_RDWR);
    CHECK(8, fd >= 0 && fstat(fd, &old) == 0 && (old.st_mode & 0777) == 0640);
    CHECK(9, link("result","alias") == 0 && unlink("result") == 0 && unlink("alias") == 0);
    CHECK(10, fstat(fd,&newer)==0 && newer.st_nlink==0 && read(fd,data,4)==4 && same(data,"old!",4));
    CHECK(11, touch("result","newdata",7) == 0 && stat("result",&newer)==0 && newer.st_ino!=old.st_ino);
    write(1, "links\n", 6);
    CHECK(12, close(fd)==0 && link("result","alias")==0 && symlink("alias","view")==0);
    CHECK(13, lstat("view",&old)==0 && S_ISLNK(old.st_mode) && readlink("view",data,sizeof data)==5 && same(data,"alias",5));
    CHECK(14, stat("view",&old)==0 && old.st_ino==newer.st_ino && old.st_nlink==2);
    CHECK(15, symlink("loop","loop")==0 && stat("loop",&old)<0 && errno==ELOOP && unlink("loop")==0);
    CHECK(16, mkdir("dir",0755)==0 && chdir("dir")==0 && rename("/work/dir","/work/moved")==0);
    CHECK(17, getcwd(data,sizeof data) && same(data,"/work/moved",12) && chdir("..")==0 && rmdir("moved")==0);
    write(1, "rename\n", 7);
    CHECK(18, touch("dst","held",4)==0 && touch("src","swap",4)==0);
    fd=open("dst",O_RDONLY);
    CHECK(19, fd>=0 && rename("src","dst")==0 && read(fd,data,4)==4 && same(data,"held",4));
    CHECK(20, fstat(fd,&old)==0 && old.st_nlink==0 && close(fd)==0);
    fd=open(".",O_RDONLY|O_DIRECTORY);
    write(1, "directory\n", 10);
    CHECK(21, fd>=0); int found=0;
    for (;;) {
        long n=sys_getdents(fd,data,sizeof data);
        CHECK(22,n>=0);
        if (!n) break;
        if (n==16 && same(data+9,"result",7)) found=1;
    }
    CHECK(23,found && close(fd)==0);
    write(1, "permissions\n", 12);
    CHECK(24, touch("C:/DOS.TXT","shared",6)==0);
    CHECK(30,link("C:/DOS.TXT","foreign")<0 && errno==EXDEV);
    CHECK(31,chmod("result",06740)==0 && chown("result",123,456)==0 && stat("result",&old)==0 && (old.st_mode&06000)==0);
    CHECK(25, chown("result",123,456)==0 && chmod("result",0640)==0 && chmod(".",0755)==0);
    CHECK(26, setgid(456)==0 && setuid(999)==0);
    fd=open("result",O_RDONLY);
    CHECK(27, fd>=0 && close(fd)==0 && open("result",O_WRONLY)<0 && errno==EACCES);
    CHECK(28, chmod("result",0777)<0 && errno==EPERM && setuid(0)<0 && errno==EPERM);
    CHECK(29, mkdir("/forbidden",0755)<0 && errno==EACCES);
    return write(1,"Btrfs root VFS OK\n",18)!=18;
}

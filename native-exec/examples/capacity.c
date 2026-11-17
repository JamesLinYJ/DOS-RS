#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <unistd.h>

#define CHECK(n, expr)                                                                             \
    do {                                                                                           \
        if (!(expr)) {                                                                             \
            const char s[] = "capacity failure " #n "\n";                                          \
            write(2, s, sizeof s - 1);                                                             \
            return n;                                                                              \
        }                                                                                          \
    } while (0)
#define MIB (1024 * 1024)
#define SEED (32 * 1024)
#define CHUNK (256 * 1024)
#define WRITES 36
static char block[CHUNK];

int main(int argc, char **argv) {
    struct statvfs root, fat, handle, again;
    struct stat st;
    int ro = argc > 1 && argv[1][0] == 'r';
    int cold = argc > 1 && argv[1][0] == 'c';
    int fd = open("/large.bin", ro ? O_RDONLY : O_RDWR);
    int dos = open("C:/CAPACITY.ELF", O_RDONLY);
    CHECK(1,
          fd >= 0 && dos >= 0 && statvfs("/large.bin", &root) == 0 && fstatvfs(fd, &handle) == 0);
    CHECK(2, root.f_fsid == handle.f_fsid && root.f_bsize == 4096 && root.f_frsize == 4096 &&
                 root.f_blocks == 128 * MIB / 4096 && root.f_namemax == 255);
    CHECK(3, fstatvfs(dos, &handle) == 0 && statvfs("/dos/c/", &fat) == 0 &&
                 fat.f_fsid == handle.f_fsid && fat.f_fsid != root.f_fsid && fat.f_namemax == 12 &&
                 fat.f_blocks * fat.f_frsize <= 4 * MIB);
    CHECK(4, statvfs("/missing", &again) < 0 && errno == ENOENT && fstatvfs(-1, &again) < 0 &&
                 errno == EBADF);
    if (ro) {
        CHECK(5, (fat.f_flag & ST_RDONLY) && !fat.f_bavail && (root.f_flag & ST_RDONLY) &&
                     !root.f_bavail);
        CHECK(6, open("/large.bin", O_RDWR) < 0 && errno == EROFS &&
                     open("C:/CAPACITY.ELF", O_RDWR) < 0 && errno == EROFS);
        const char s[] = "read-only FAT and Btrfs capacity OK\n";
        write(1, s, sizeof s - 1);
        return 0;
    }
    CHECK(7, !(root.f_flag & ST_RDONLY) && root.f_bavail > 8 * MIB / 4096 &&
                 root.f_bavail <= root.f_bfree && root.f_bfree <= root.f_blocks);
    CHECK(8, !(fat.f_flag & ST_RDONLY) && fat.f_bavail == fat.f_bfree && fat.f_bavail > 0);
    CHECK(9, fstat(fd, &st) == 0);
    if (cold) {
        CHECK(10, st.st_size == SEED + WRITES * CHUNK && st.st_blocks * 512 >= st.st_size &&
                      pread(fd, block, CHUNK, SEED + (WRITES - 1) * CHUNK) == CHUNK);
        for (unsigned i = 0; i < sizeof block; i++)
            CHECK(11, (unsigned char)block[i] == (unsigned char)(i * 7 + (WRITES - 1) * 31));
        const char s[] = "cold block group file OK\n";
        write(1, s, sizeof s - 1);
        return 0;
    }
    CHECK(12, st.st_size == SEED && lseek(fd, 0, SEEK_END) == SEED);
    for (unsigned j = 0; j < WRITES; j++) {
        for (unsigned i = 0; i < sizeof block; i++)
            block[i] = (unsigned char)(i * 7 + j * 31);
        CHECK(13, write(fd, block, sizeof block) == sizeof block);
    }
    CHECK(14, fsync(fd) == 0 && fstat(fd, &st) == 0 && st.st_size == SEED + WRITES * CHUNK &&
                  st.st_blocks * 512 >= st.st_size);
    CHECK(15, fstatvfs(fd, &again) == 0 && again.f_bavail < root.f_bavail &&
                  again.f_blocks == root.f_blocks);
    CHECK(16, close(dos) == 0 && close(fd) == 0);
    const char s[] = "allocated data across original block group OK\n";
    write(1, s, sizeof s - 1);
    return 0;
}

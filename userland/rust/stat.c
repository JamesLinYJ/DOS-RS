/* struct stat has a different C layout on i686 and x86_64. Keep libc's own
 * declarations at this boundary instead of reproducing internal padding in Rust. */
#include <stdint.h>
#include <sys/stat.h>

struct file_info { uint64_t dev, ino; int64_t size; uint32_t mode, rdev; };
_Static_assert(sizeof(struct file_info) == 32, "file_info across DOS-RS ABIs");

int dosrs_file_info(const char *name, int fd, struct file_info *out) {
    struct stat st;
    int r = name ? stat(name, &st) : fstat(fd, &st);
    if (r != 0) return r;
    *out = (struct file_info){st.st_dev, st.st_ino, st.st_size, st.st_mode, (uint32_t)st.st_rdev};
    return 0;
}

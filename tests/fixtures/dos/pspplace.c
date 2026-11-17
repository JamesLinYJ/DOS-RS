/* DOS-RS test client. Smaller C's getenv implementation supplies the real-mode
   PSP query and far reads for this 386 real-mode tiny-model program. */
#include <getenv.c>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

void probe(void)
{
    unsigned psp = DosGetPspSeg();
    unsigned env = peek(psp, 0x2c);
    char report[] = "PSP=0000 OWNER/ENV/IO OK\r\n";
    char filename[] = "PSPPLACE.TMP";
    char payload[] = "UMB file";
    char readback[8];
    char hex[] = "0123456789ABCDEF";
    int fd, i;

    fd = __open(filename, O_RDWR | O_CREAT | O_TRUNC, S_IRUSR | S_IWUSR);
    if (fd < 0 || __write(fd, payload, 8) != 8 || __close(fd))
        goto failed;
    fd = __open(filename, O_RDONLY);
    if (fd < 0 || __read(fd, readback, 8) != 8 || __close(fd))
        goto failed;
    for (i = 0; i < 8; ++i)
        if (readback[i] != payload[i])
            goto failed;
    if (peek(psp - 1, 1) != psp || !env || peek(env - 1, 1) != psp)
        goto failed;
    for (i = 7; i >= 4; --i) {
        report[i] = hex[psp & 15];
        psp >>= 4;
    }
    __write(1, report, sizeof(report) - 1);
    return;
failed:
    __write(1, "PSP TEST FAILED\r\n", 17);
}

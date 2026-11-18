/* DOS-RS enters _start as a native C function on both architectures. */
extern int main(int, char **, char **);
extern int __libc_start_main(int (*)(int, char **, char **), int, char **, void (*)(void),
                             void (*)(void), void (*)(void));
_Noreturn void _start(int argc, char **argv)
{
    __libc_start_main(main, argc, argv, 0, 0, 0);
    __builtin_unreachable();
}

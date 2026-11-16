#include <unistd.h>
extern int main(int argc, char **argv);
_Noreturn void _start(int argc, char **argv) { _exit(main(argc, argv)); }

#include <glob.h>
#include <stddef.h>

int dosrs_shell_glob(const char *pattern, void *context, int (*append)(void *, const char *))
{
    glob_t result = {0};
    int status = glob(pattern, 0, NULL, &result);
    if (status == 0) {
        for (size_t i = 0; i < result.gl_pathc; i++) {
            if (append(context, result.gl_pathv[i]) < 0) {
                status = GLOB_ABORTED;
                break;
            }
        }
    }
    globfree(&result);
    return status == 0 ? 0 : status == GLOB_NOMATCH ? 1 : -1;
}

#include <pwd.h>
#include <unistd.h>

const char *dosrs_shell_home(const char *login)
{
    struct passwd *entry = login ? getpwnam(login) : getpwuid(getuid());
    return entry ? entry->pw_dir : NULL;
}

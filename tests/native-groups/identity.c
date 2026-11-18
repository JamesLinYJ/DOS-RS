#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <limits.h>
#include <pthread.h>
#include <pwd.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

#define CHECK(e)                                                               \
  do {                                                                         \
    if (!(e)) {                                                                \
      dprintf(2, "identity line %d errno %d\n", __LINE__, errno);              \
      _exit(90);                                                               \
    }                                                                          \
  } while (0)
static int channel[2];
static gid_t other[] = {81, 70002};
static int have_groups(void) {
  gid_t values[4] = {0};
  return getgroups(0, NULL) == 2 && getgroups(4, values) == 2 &&
         values[0] == other[0] && values[1] == other[1];
}
static void *observe_groups(void *unused) {
  char c;
  CHECK(read(channel[0], &c, 1) == 1 && have_groups());
  return NULL;
}
static void collect(FILE *log, const char *name) {
  int fd = open(name, O_RDONLY);
  CHECK(fd >= 0);
  unsigned char buf[256];
  ssize_t n;
  while ((n = read(fd, buf, sizeof buf)) > 0)
    CHECK(fwrite(buf, 1, n, log) == (size_t)n);
  CHECK(n == 0 && close(fd) == 0);
  CHECK(fputc(0, log) == 0);
}
static void command(FILE *log, int user, char *const argv[]) {
  char executable[64];
  CHECK(snprintf(executable, sizeof executable, "/bin/%s", argv[0]) <
        (int)sizeof executable);
  int out = open("stdout", O_CREAT | O_WRONLY | O_TRUNC, 0644);
  int err = open("stderr", O_CREAT | O_WRONLY | O_TRUNC, 0644);
  CHECK(out >= 0 && err >= 0);
  pid_t child = fork();
  CHECK(child >= 0);
  if (!child) {
    CHECK(dup2(out, 1) >= 0 && dup2(err, 2) >= 0);
    close(out);
    close(err);
    if (user)
      CHECK(setgid(70000) == 0 && setuid(70001) == 0);
    char *env[] = {"LC_ALL=C", "LANG=C", "TZ=UTC", "PATH=/bin", NULL};
    execve(executable, argv, env);
    _exit(127);
  }
  close(out);
  close(err);
  int status;
  CHECK(waitpid(child, &status, 0) == child && WIFEXITED(status));
  fprintf(log, "%d ", user);
  for (int i = 0; argv[i]; i++)
    fprintf(log, "%s ", argv[i]);
  fprintf(log, "%d\n", WEXITSTATUS(status));
  collect(log, "stdout");
  collect(log, "stderr");
}
static void user_permissions(void) {
  CHECK(getuid() == 70001 && geteuid() == 70001 && getgid() == 70000);
  CHECK(have_groups());
  CHECK(kill(getppid(), 0) < 0 && errno == EPERM);
  int fd = open("groupfile", O_RDWR);
  CHECK(fd >= 0 && write(fd, "G", 1) == 1 && close(fd) == 0);
  CHECK(open("restricted", O_RDONLY) < 0 && errno == EACCES);
  CHECK(setgroups(0, NULL) < 0 && errno == EPERM);
  CHECK(chown("own", (uid_t)-1, 81) == 0);
  CHECK(chmod("own", 02750) == 0);
  struct stat st;
  CHECK(stat("own", &st) == 0 && st.st_gid == 81 &&
        (st.st_mode & 07777) == 02750);
  CHECK(chown("own", (uid_t)-1, 70003) < 0 && errno == EPERM);
}
int main(int argc, char **argv) {
  CHECK(chdir("/work") == 0);
  if (argc == 2) {
    user_permissions();
    return 0;
  }
  struct passwd pw, *person;
  struct group gr, *group;
  char buf[512];
  CHECK(getpwnam_r("operator", &pw, buf, sizeof buf, &person) == 0 && person &&
        pw.pw_uid == 70001);
  CHECK(getgrgid_r(70002, &gr, buf, sizeof buf, &group) == 0 && group &&
        !strcmp(gr.gr_name, "archive"));
  CHECK(getpwnam_r("absent", &pw, buf, sizeof buf, &person) == 0 && !person);
  CHECK(initgroups("operator", 70000) == 0);
  gid_t listed[4];
  CHECK(getgroups(4, listed) == 3 && listed[0] == 81 && listed[1] == 70000 &&
        listed[2] == 70002);
  CHECK(setgroups(0, NULL) == 0 && getgroups(0, NULL) == 0);
  CHECK(pipe(channel) == 0);
  pthread_t worker;
  CHECK(pthread_create(&worker, NULL, observe_groups, NULL) == 0);
  const gid_t unordered[] = {70002, 81};
  CHECK(setgroups(2, unordered) == 0);
  CHECK(write(channel[1], "g", 1) == 1 && pthread_join(worker, NULL) == 0);
  close(channel[0]);
  close(channel[1]);
  CHECK(getgroups(1, listed) < 0 && errno == EINVAL && have_groups());
  const gid_t invalid = (gid_t)-1;
  CHECK(setgroups(1, &invalid) < 0 && errno == EINVAL && have_groups());
  FILE *log = fopen("report", "wb");
  CHECK(log);
  const char *files[] = {"groupfile", "own", "restricted", "change", NULL};
  for (int i = 0; files[i]; i++) {
    int fd = open(files[i], O_CREAT | O_WRONLY, 0660);
    CHECK(fd >= 0 && close(fd) == 0 && chmod(files[i], 0660) == 0);
  }
  CHECK(chown("groupfile", 0, 70002) == 0 && chown("own", 70001, 70000) == 0);
  CHECK(chown("restricted", 0, 70003) == 0);
  pid_t child = fork();
  CHECK(child >= 0);
  if (!child) {
    CHECK(setgid(70000) == 0 && setuid(70001) == 0);
    execl("/probe.elf", "probe.elf", "child", (char *)0);
    _exit(127);
  }
  int status;
  CHECK(waitpid(child, &status, 0) == child && WIFEXITED(status) &&
        WEXITSTATUS(status) == 0);
  CHECK(getuid() == 0 && have_groups());
#define RUN(u, ...) command(log, u, (char *[]){__VA_ARGS__, NULL})
  RUN(0, "id");
  RUN(1, "id");
  RUN(1, "id", "-u");
  RUN(1, "id", "-gn");
  RUN(1, "id", "-Gn");
  RUN(1, "id", "-Gz");
  RUN(0, "id", "operator");
  RUN(0, "groups", "operator");
  RUN(1, "groups");
  RUN(1, "whoami");
  RUN(0, "whoami");
  RUN(0, "id", "missing-user");
  RUN(0, "chown", "operator:archive", "change");
  RUN(1, "chgrp", "readers", "change");
  RUN(1, "chgrp", "restricted", "change");
  RUN(1, "chown", "root", "change");
  struct stat st;
  CHECK(stat("change", &st) == 0 && st.st_uid == 70001 && st.st_gid == 81);
  CHECK(fclose(log) == 0);
  puts("identity suite complete");
  return 0;
}

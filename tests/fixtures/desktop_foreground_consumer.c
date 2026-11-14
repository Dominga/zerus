/* An owned foreground leader with a caller reparented outside its ancestry.
 * The orphan execs Python before it invokes hgs, so even a copy named codex is
 * not an ancestor image. Only the leader retains its initial agent evidence.
 */
#ifdef __APPLE__
#define _DARWIN_C_SOURCE 1
#else
#define _GNU_SOURCE 1
#endif
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 7) return 90;
    int tty = open(argv[1], O_RDWR | O_NOCTTY);
    if (tty < 0 || ioctl(tty, TIOCSCTTY, 0) < 0) return 91;
    signal(SIGTTOU, SIG_IGN);
    if (tcsetpgrp(tty, getpgrp()) < 0 || getpid() != getpgrp()) return 92;
    pid_t intermediary = fork();
    if (intermediary < 0) return 93;
    if (intermediary == 0) {
        pid_t parent = getpid();
        pid_t caller = fork();
        if (caller < 0) _exit(94);
        if (caller > 0) _exit(0);
        time_t until = time(NULL) + 10;
        while (getppid() == parent && time(NULL) < until) usleep(5000);
        if (getppid() == parent) _exit(95);
        unsetenv("HGS_SESSION"); unsetenv("HGS_SESSION_ID");
        unsetenv("HGS_RUN_ID"); unsetenv("HGS_AGENT");
        execl(argv[2], argv[2], argv[3], "caller", argv[4], argv[5], (char *)0);
        _exit(96);
    }
    int status;
    if (waitpid(intermediary, &status, 0) != intermediary || status != 0) return 97;
    if (strcmp(argv[6], "restricted") == 0) {
        /* A real platform shell, including the marker it inherited before exec. */
        execl("/bin/zsh", "zsh", "-c",
              "[[ -n $HGS_RUN_ID ]] && print inherited >\"$1/inherited\"; "
              "end=$((SECONDS+25)); "
              "while [[ ! -e $1/done ]] && ((SECONDS < end)); do /bin/sleep 0.02; done; "
              "[[ -e $1/done ]]",
              "foreground-fixture", argv[5], (char *)0);
        return 98;
    }
    char done[4096];
    if (snprintf(done, sizeof(done), "%s/done", argv[5]) >= (int)sizeof(done)) return 99;
    time_t until = time(NULL) + 25;
    while (access(done, F_OK) != 0 && time(NULL) < until) usleep(5000);
    return access(done, F_OK) == 0 ? 0 : 100;
}

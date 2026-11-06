/* Private canonical frontend for frontend-request integration, no GUI or SDK.
 * The native parent owns the canonical socket and the exact unreaped CLI child.
 * Python supplies closed cached bytes only after observing that child's birth.
 */
/* Darwin libproc headers require BSD types even with strict C11 enabled. */
#ifdef __APPLE__
#define _DARWIN_C_SOURCE 1
#else
#define _GNU_SOURCE 1
#endif
#include <sys/types.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>
#ifdef __APPLE__
#include <libproc.h>
#else
#include <sys/mman.h>
#include <time.h>
#endif

static pid_t child = -1;
static void stop(int sig) {
    if (child > 0) {
        kill(child, SIGTERM);
        while (waitpid(child, NULL, 0) < 0 && errno == EINTR) {}
    }
    _exit(128 + sig);
}
static void check(int ok) {
    if (!ok) { perror("private frontend fixture"); stop(SIGABRT); }
}
static void write_all(int fd, const char *bytes, size_t length) {
    while (length) {
        ssize_t used = write(fd, bytes, length);
        if (used < 0 && errno == EINTR) continue;
        check(used > 0);
        bytes += used; length -= (size_t)used;
    }
}
static char *line(void) {
    const size_t cap = 160 * 1024; /* bounded private program envelope */
    char *bytes = calloc(cap + 1, 1);
    check(bytes != NULL && fgets(bytes, (int)cap + 1, stdin) != NULL);
    size_t length = strlen(bytes);
    check(length > 0 && length <= cap && bytes[length - 1] == '\n');
    return bytes;
}
static void birth(pid_t pid, char *output, size_t length) {
#ifdef __APPLE__
    struct proc_bsdinfo info;
    check(proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, sizeof(info)) == (int)sizeof(info));
    snprintf(output, length, "%llu:%llu", (unsigned long long)info.pbi_start_tvsec,
             (unsigned long long)info.pbi_start_tvusec);
#else
    char path[64], bytes[4096];
    snprintf(path, sizeof(path), "/proc/%d/stat", pid);
    FILE *file = fopen(path, "r");
    check(file != NULL && fgets(bytes, sizeof(bytes), file) != NULL);
    fclose(file);
    char *after = strrchr(bytes, ')'); check(after != NULL);
    char *save = NULL, *field = strtok_r(after + 2, " ", &save);
    for (int number = 3; number < 22; number++) {
        check(field != NULL); field = strtok_r(NULL, " ", &save);
    }
    check(field != NULL); snprintf(output, length, "%s", field);
#endif
}
#ifdef __linux__
/* Native adversarial relay: SAME binary/uid/direct parent/argv, a kernel-sealed
 * image instead of the installed inode. No product FD or image assertion. */
static int sealed_image(const char *path, const char *mode) {
    if (!strncmp(mode, "held:", 5)) {
        char *end = NULL;
        long held = strtol(mode + 5, &end, 10);
        check(end && !*end && held >= 3 && held < 4096);
        int required = F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL;
        check((fcntl((int)held, F_GET_SEALS) & required) == required);
        int retained = fcntl((int)held, F_DUPFD_CLOEXEC, 3);
        check(retained >= 0);
        return retained; /* SAME fixture-selected original object, no recopy */
    }
    int input = open(path, O_RDONLY | O_CLOEXEC); check(input >= 0);
    struct stat info; check(fstat(input, &info) == 0 && S_ISREG(info.st_mode));
    check(info.st_size > 0 && info.st_size <= (256L << 20));
    int image = memfd_create("zerus-helper-image", MFD_CLOEXEC | MFD_ALLOW_SEALING | 0x10);
    check(image >= 0);
    struct timespec start, now; check(clock_gettime(CLOCK_MONOTONIC, &start) == 0);
    char buffer[65536]; off_t total = 0;
    for (;;) {
        ssize_t used = read(input, buffer, sizeof(buffer));
        if (used < 0 && errno == EINTR) continue;
        check(used >= 0); if (!used) break;
        total += used; check(total <= info.st_size);
        write_all(image, buffer, (size_t)used);
        check(clock_gettime(CLOCK_MONOTONIC, &now) == 0 && now.tv_sec - start.tv_sec < 5);
    }
    check(total == info.st_size); close(input);
    int seals = F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL;
    if (!strcmp(mode, "missing-write")) seals &= ~F_SEAL_WRITE;
    else if (!strcmp(mode, "missing-grow")) seals &= ~F_SEAL_GROW;
    else if (!strcmp(mode, "missing-shrink")) seals &= ~F_SEAL_SHRINK;
    else if (!strcmp(mode, "missing-seal")) seals &= ~F_SEAL_SEAL;
    else if (!strcmp(mode, "future-write")) seals = F_SEAL_FUTURE_WRITE;
    else check(!strcmp(mode, "sealed") || !strcmp(mode, "wrong-parent"));
    check(fcntl(image, F_ADD_SEALS, seals) == 0);
    return image;
}
#endif
int main(int argc, char **argv) {
    check(argc == 3 || argc == 4 || argc == 5);
    const char *program_command = argc == 5 ? argv[4] : NULL;
    check(program_command == NULL || !strcmp(program_command, "program-stop") || !strcmp(program_command, "program-cleanup"));
    int image = -1;
    int rendezvous = argc == 4 && !strcmp(argv[3], "rendezvous");
#ifdef __linux__
    if (argc >= 4 && !rendezvous) image = sealed_image(argv[2], argv[3]);
#else
    check(argc == 3 || rendezvous);
#endif
    signal(SIGPIPE, SIG_IGN); signal(SIGTERM, stop); signal(SIGALRM, stop); alarm(35);
    int listener = socket(AF_UNIX, SOCK_STREAM, 0); check(listener >= 0);
    check(fcntl(listener, F_SETFD, FD_CLOEXEC) == 0);
    struct sockaddr_un address; memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
#ifdef __APPLE__
    address.sun_len = sizeof(address);
#endif
    check(strlen(argv[1]) < sizeof(address.sun_path));
    strcpy(address.sun_path, argv[1]);
    check(bind(listener, (struct sockaddr *)&address, sizeof(address)) == 0);
    check(chmod(argv[1], 0600) == 0 && listen(listener, 8) == 0);
    int input[2], output[2]; check(pipe(input) == 0 && pipe(output) == 0);
    int identity_pipe[2]; check(pipe(identity_pipe) == 0);
    child = fork(); check(child >= 0);
    if (child == 0) {
        close(identity_pipe[0]);
        if (argc == 4 && !strcmp(argv[3], "wrong-parent")) {
            child = fork(); check(child >= 0);
            if (child > 0) {
                write_all(identity_pipe[1], (const char *)&child, sizeof(child));
                close(identity_pipe[1]);
                close(input[0]); close(input[1]); close(output[0]); close(output[1]); close(listener);
                int result; check(waitpid(child, &result, 0) == child); child = -1;
                check(WIFEXITED(result)); _exit(WEXITSTATUS(result));
            }
        }
        close(identity_pipe[1]);
        close(input[1]); close(output[0]);
        check(dup2(input[0], STDIN_FILENO) >= 0 && dup2(output[1], STDOUT_FILENO) >= 0);
        close(input[0]); close(output[1]);
        /* A marked native parent stays visible even if its relay is clean. */
        unsetenv("HGS_RUN_ID");
        char program[64];
        const char *selected = argv[2];
        if (image >= 0) {
            snprintf(program, sizeof(program), "/proc/self/fd/%d", image);
            selected = program;
        }
        if (program_command)
            execl(selected, argv[2], "worktrees", program_command, "--json", (char *)NULL);
        else if (rendezvous)
            execl(selected, argv[2], "--desktop-helper-rendezvous-v1", "desktop", "frontend-request", "--json", (char *)NULL);
        else
            execl(selected, argv[2], "desktop", "frontend-request", "--json", (char *)NULL);
        _exit(127);
    }
    close(input[0]); close(output[1]);
    if (image >= 0) close(image);
    close(identity_pipe[1]);
    pid_t relay = child;
    if (argc == 4 && !strcmp(argv[3], "wrong-parent")) {
        check(read(identity_pipe[0], &relay, sizeof(relay)) == (ssize_t)sizeof(relay));
    }
    close(identity_pipe[0]);
    char identity[128]; birth(relay, identity, sizeof(identity));
    printf("{\"pid\":%d,\"birth\":\"%s\"}\n", relay, identity); fflush(stdout);
    char *envelope = line(), *reply = line(), *query = line();
    if (rendezvous) write_all(input[1], "HGS-ORIGINAL-CHILD-V1\n", sizeof("HGS-ORIGINAL-CHILD-V1\n") - 1);
    write_all(input[1], envelope, strlen(envelope)); close(input[1]); free(envelope);
    size_t total = 0; int proofs = 0;
    for (;;) {
        struct pollfd ready[2] = {{listener, POLLIN, 0}, {output[0], POLLIN, 0}};
        int result = poll(ready, 2, 30000);
        if (result < 0 && errno == EINTR) continue;
        check(result > 0);
        if (ready[0].revents & POLLIN) {
            int client = accept(listener, NULL, NULL); check(client >= 0);
            struct pollfd incoming = {client, POLLIN, 0};
            if (poll(&incoming, 1, 1000) == 1) {
                char request[65]; ssize_t used = read(client, request, sizeof(request));
                if (used == (ssize_t)strlen(query) && memcmp(request, query, (size_t)used) == 0) {
                    write_all(client, reply, strlen(reply)); proofs++;
                }
            }
            close(client);
        }
        if (ready[1].revents & (POLLIN | POLLHUP)) {
            char bytes[4096]; ssize_t used = read(output[0], bytes, sizeof(bytes));
            if (used < 0 && errno == EINTR) continue;
            check(used >= 0);
            if (used == 0) break;
            total += (size_t)used; check(total <= 65536);
            write_all(STDOUT_FILENO, bytes, (size_t)used);
        }
    }
    close(output[0]); close(listener); check(unlink(argv[1]) == 0);
    int status; check(waitpid(child, &status, 0) == child); child = -1;
    fprintf(stderr, "private frontend proofs=%d original_reaped=1\n", proofs);
    free(reply); free(query);
    check(WIFEXITED(status)); return WEXITSTATUS(status);
}

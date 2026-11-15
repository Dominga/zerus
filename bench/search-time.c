#define _GNU_SOURCE
#include <errno.h>
#include <limits.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

/* Pin our own child until its foreground I/O is captured, before reaping it. */
int main(int argc, char **argv) {
    if (argc < 3) return 2;
    pid_t child = fork();
    if (child < 0) return 2;
    if (child == 0) {
        execv(argv[2], &argv[2]);
        perror("benchmark exec");
        _exit(127);
    }
    unsigned long peak = 0;
    char path[128], line[256];
    for (;;) {
        siginfo_t event = {0};
        if (waitid(P_PID, (id_t)child, &event, WEXITED | WNOHANG | WNOWAIT) < 0) {
            if (errno == EINTR) continue;
            return 2;
        }
        if (event.si_pid == child) break;
        snprintf(path, sizeof(path), "/proc/%ld/status", (long)child);
        FILE *status = fopen(path, "re");
        if (status != NULL) {
            while (fgets(line, sizeof(line), status)) {
                unsigned long value;
                if (sscanf(line, "VmHWM: %lu", &value) == 1 && value > peak) peak = value;
            }
            fclose(status);
        }
        struct timespec pause = {0, 1000000};
        nanosleep(&pause, NULL);
    }
    unsigned long long reads = ULLONG_MAX, disk = ULLONG_MAX;
    /* Per-thread counters exclude I/O folded in from reaped background helpers. */
    snprintf(path, sizeof(path), "/proc/%ld/task/%ld/io", (long)child, (long)child);
    FILE *input = fopen(path, "re");
    if (input != NULL) {
        while (fgets(line, sizeof(line), input)) {
            unsigned long long value;
            if (sscanf(line, "rchar: %llu", &value) == 1) reads = value;
            if (sscanf(line, "read_bytes: %llu", &value) == 1) disk = value;
        }
        fclose(input);
    }
    int status;
    struct rusage usage;
    while (wait4(child, &status, 0, &usage) < 0) {
        if (errno != EINTR) return 2;
    }
    FILE *output = fopen(argv[1], "wx");
    if (output == NULL) return 2;
    if (fprintf(output, "%lu %llu %llu %u\n", peak ? peak : (unsigned long)usage.ru_maxrss,
                reads, disk, peak != 0) < 0) {
        fclose(output);
        return 2;
    }
    if (fclose(output) != 0) return 2;
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}

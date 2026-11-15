#define _GNU_SOURCE
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>

/* Measure only the CLI child, excluding the Python fixture generator's heap. */
int main(int argc, char **argv) {
    if (argc < 3) return 2;
    pid_t child = fork();
    if (child < 0) return 2;
    if (child == 0) {
        execv(argv[2], &argv[2]);
        perror("benchmark exec");
        _exit(127);
    }
    int status;
    struct rusage usage;
    while (wait4(child, &status, 0, &usage) < 0) {
        if (errno != EINTR) return 2;
    }
    FILE *output = fopen(argv[1], "wx");
    if (output == NULL) return 2;
    if (fprintf(output, "%ld\n", usage.ru_maxrss) < 0) {
        fclose(output);
        return 2;
    }
    if (fclose(output) != 0) return 2;
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}

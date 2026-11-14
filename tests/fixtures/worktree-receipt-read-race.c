/* Private Linux fixture: hold the first read of one selected receipt while
 * the fixture writer changes that same inode. Only this private child is held. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

ssize_t read(int fd, void *buffer, size_t length) {
    ssize_t (*original)(int, void *, size_t) = dlsym(RTLD_NEXT, "read");
    const char *target = getenv("ZERUS_POLL_RECEIPT");
    const char *ready = getenv("ZERUS_POLL_READ_READY");
    const char *done = getenv("ZERUS_POLL_WRITE_DONE");
    char link[64], path[PATH_MAX];
    if (target && ready && done) {
        snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
        ssize_t n = readlink(link, path, sizeof(path) - 1);
        if (n >= 0) {
            path[n] = '\0';
            if (strcmp(path, target) == 0) {
                int gate = open(ready, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
                if (gate >= 0) {
                    close(gate);
                    /* A missed barrier is a fixture failure, never a receipt refusal. */
                    struct timespec tick = {0, 1000000};
                    int rounds = 3000;
                    while (access(done, F_OK) != 0 && rounds-- > 0) nanosleep(&tick, NULL);
                    if (access(done, F_OK) != 0) { errno = EIO; return -1; }
                }
            }
        }
    }
    return original(fd, buffer, length);
}

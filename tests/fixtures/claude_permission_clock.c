/* Fixture-only clock for the hgs answer process. Ordinary child commands and
 * all other threads retain the kernel clock. Only the chooser's polling waits
 * advance this clock, so a busy host cannot accidentally age the fake guard.
 * Each advance waits for the owned fake TUI to acknowledge its new frame. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

struct clock_data { _Atomic uint64_t now, acknowledged, owner; };
static struct clock_data *clock_data;
static struct timespec anchor;
static pid_t owner;
static int events;

static int active(void) {
    return clock_data && getpid() == owner && syscall(SYS_gettid) == owner;
}

__attribute__((constructor)) static void initialize(void) {
    const char *path = getenv("TEST_CLAUDE_CLOCK");
    const char *parent = getenv("TEST_CLAUDE_CLOCK_OWNER");
    if (!path || (parent && strtol(parent, NULL, 10) != getpid())) return;
    int fd = open(path, O_RDWR | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
    struct stat st;
    const char *pipe = getenv("TEST_CLAUDE_CLOCK_EVENTS");
    if (fd < 0 || !pipe || fstat(fd, &st) || !S_ISREG(st.st_mode)
        || st.st_size != (off_t)sizeof(struct clock_data)) _exit(120);
    events = atoi(pipe);
    if (fstat(events, &st) || !S_ISFIFO(st.st_mode)
        || fcntl(events, F_SETFD, FD_CLOEXEC)) _exit(121);
    clock_data = mmap(NULL, sizeof(*clock_data), PROT_READ | PROT_WRITE,
                      MAP_SHARED, fd, 0);
    close(fd);
    if (clock_data == MAP_FAILED) _exit(122);
    if (syscall(SYS_clock_gettime, CLOCK_MONOTONIC, &anchor)) _exit(123);
    owner = getpid();
    char text[32];
    snprintf(text, sizeof(text), "%ld", (long)owner);
    if (setenv("TEST_CLAUDE_CLOCK_OWNER", text, 1)) _exit(124);
    atomic_store(&clock_data->owner, owner);
}

int clock_gettime(clockid_t kind, struct timespec *value) {
    if (!active() || kind != CLOCK_MONOTONIC)
        return syscall(SYS_clock_gettime, kind, value);
    uint64_t ns = anchor.tv_nsec + atomic_load(&clock_data->now);
    value->tv_sec = anchor.tv_sec + ns / 1000000000;
    value->tv_nsec = ns % 1000000000;
    return 0;
}

static int advance(const struct timespec *duration) {
    /* The chooser polls for 40 ms; this 150 ms model has a 30 ms tail. */
    if (!active() || duration->tv_sec != 0
        || (duration->tv_nsec != 40000000 && duration->tv_nsec != 30000000)) return 0;
    uint64_t now = atomic_fetch_add(&clock_data->now, duration->tv_nsec) + duration->tv_nsec;
    if (write(events, "t", 1) != 1) _exit(125);
    struct timespec from, at, pause = {0, 1000000};
    syscall(SYS_clock_gettime, CLOCK_MONOTONIC, &from);
    while (atomic_load(&clock_data->acknowledged) != now) {
        syscall(SYS_clock_gettime, CLOCK_MONOTONIC, &at);
        if (at.tv_sec - from.tv_sec >= 5) _exit(126);
        syscall(SYS_nanosleep, &pause, NULL);
    }
    return 1;
}

int nanosleep(const struct timespec *duration, struct timespec *remaining) {
    if (advance(duration)) return 0;
    return syscall(SYS_nanosleep, duration, remaining);
}

int clock_nanosleep(clockid_t kind, int flags, const struct timespec *duration,
                    struct timespec *remaining) {
    if (!flags && kind == CLOCK_MONOTONIC && advance(duration)) return 0;
    int result = syscall(SYS_clock_nanosleep, kind, flags, duration, remaining);
    return result < 0 ? errno : result;
}

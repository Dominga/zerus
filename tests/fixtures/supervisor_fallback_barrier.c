#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
ssize_t write(int fd, const void *buf, size_t count) {
    static ssize_t (*original)(int,const void*,size_t);
    if (!original) original=dlsym(RTLD_NEXT,"write");
    ssize_t result=original(fd,buf,count);
    const char *root=getenv("G1_FALLBACK_BARRIER");
    if (root && fd==2 && memmem(buf,count,"nothing to resume",17)) {
        char ready[4096],release[4096];
        snprintf(ready,sizeof ready,"%s/decision-ready",root);
        snprintf(release,sizeof release,"%s/decision-release",root);
        int marker=open(ready,O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC,0600);
        if (marker>=0) close(marker);
        struct timespec start,now;clock_gettime(CLOCK_MONOTONIC,&start);
        while (access(release,F_OK)!=0) {
            clock_gettime(CLOCK_MONOTONIC,&now);
            if (now.tv_sec-start.tv_sec>15) _exit(93);
            usleep(1000);
        }
    }
    return result;
}

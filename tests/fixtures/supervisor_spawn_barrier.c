#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <time.h>
#include <unistd.h>
static int armed;
static void barrier(void) {
    const char *root=getenv("G1_FALLBACK_BARRIER");
    if (!root) return;
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
ssize_t write(int fd,const void *buf,size_t count) {
    static ssize_t (*real)(int,const void*,size_t);
    if (!real) real=dlsym(RTLD_NEXT,"write");
    ssize_t result=real(fd,buf,count);
    if (getenv("G1_FALLBACK_BARRIER") && fd==2 && memmem(buf,count,"nothing to resume",17)) armed=1;
    return result;
}
int flock(int fd,int operation) {
    static int (*real)(int,int);
    if (!real) real=dlsym(RTLD_NEXT,"flock");
    int result=real(fd,operation);
    const char *state=getenv("HGS_STATE_DIR");
    if (result==0 && operation==LOCK_UN && armed && state) {
        char link[128],path[4096],expected[4096];
        snprintf(link,sizeof link,"/proc/self/fd/%d",fd);
        ssize_t n=readlink(link,path,sizeof path-1);
        if (n>=0) {
            path[n]=0;snprintf(expected,sizeof expected,"%s/.lock",state);
            if (!strcmp(path,expected)) { armed=0;barrier(); }
        }
    }
    return result;
}

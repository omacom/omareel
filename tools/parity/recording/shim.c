// A logging shim: records its argv (argv[0] included) and the role of the process that started
// it (the parent's first omareel argument), then execs the target named in <dir>/targets/<name>
// with the same argv. Built once, copied under each program's name.
#include <errno.h>
#include <fcntl.h>
#include <libgen.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <unistd.h>

int main(int argc, char **argv)
{
    char self[PATH_MAX];
    ssize_t n = readlink("/proc/self/exe", self, sizeof self - 1);
    if (n < 0) return 127;
    self[n] = 0;
    char dir[PATH_MAX], base[PATH_MAX];
    strcpy(dir, self); strcpy(base, self);
    const char *name = basename(base);
    const char *shimdir = dirname(dir);
    char parent[4096] = {0};
    char path[PATH_MAX];
    snprintf(path, sizeof path, "/proc/%d/cmdline", (int)getppid());
    int fd = open(path, O_RDONLY);
    ssize_t len = fd >= 0 ? read(fd, parent, sizeof parent - 1) : 0;
    if (fd >= 0) close(fd);
    // The role: the parent's argv[1] (record, __record-daemon, __record-bar, ...) for omareel,
    // else the parent's program name.
    const char *role = parent;
    size_t first = strlen(parent);
    if ((ssize_t)first + 1 < len) role = parent + first + 1;
    if (strstr(parent, "omareel") == NULL) role = parent;
    const char *log = getenv("OMAREEL_PARITY_LOG");
    if (log && *log) {
        int out = open(log, O_WRONLY | O_CREAT | O_APPEND, 0644);
        if (out >= 0) {
            flock(out, LOCK_EX);
            char buffer[65536];
            size_t used = (size_t)snprintf(buffer, sizeof buffer, "%s\037%s", name, role);
            for (int i = 0; i < argc && used < sizeof buffer; ++i)
                used += (size_t)snprintf(buffer + used, sizeof buffer - used, "\037%s", argv[i]);
            if (used < sizeof buffer) buffer[used++] = '\036';
            (void)!write(out, buffer, used);
            close(out);
        }
    }
    snprintf(path, sizeof path, "%s/targets/%s", shimdir, name);
    char target[PATH_MAX] = {0};
    FILE *file = fopen(path, "r");
    if (!file || !fgets(target, sizeof target, file)) { fprintf(stderr, "shim: no target for %s\n", name); return 127; }
    fclose(file);
    target[strcspn(target, "\n")] = 0;
    execv(target, argv);
    fprintf(stderr, "shim: exec %s: %s\n", target, strerror(errno));
    return 127;
}

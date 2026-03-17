#pragma once

#ifdef _WIN32
/* Define timeval guard BEFORE any Windows headers include winsock to avoid redefinition */
#ifndef _TIMEVAL_DEFINED
#define _TIMEVAL_DEFINED
struct timeval { long tv_sec; long tv_usec; };
#endif
#include <windows.h>
#include <direct.h>
#include <io.h>
#include <sys/stat.h>
#include <sys/utime.h>
#include <time.h>
#include <stdlib.h>
#include <string.h>

/* S_ISDIR / S_ISREG */
#ifndef S_ISDIR
#define S_ISDIR(m) (((m) & _S_IFMT) == _S_IFDIR)
#endif
#ifndef S_ISREG
#define S_ISREG(m) (((m) & _S_IFMT) == _S_IFREG)
#endif

/* mkdir(path, mode) → _mkdir(path) */
#define mkdir(path, mode) _mkdir(path)

/* access / F_OK */
#define access _access
#ifndef F_OK
#define F_OK 0
#endif

/* strcasecmp / strncasecmp */
#define strcasecmp  _stricmp
#define strncasecmp _strnicmp

/* symlink — not supported on Windows; stub returns -1 */
static inline int symlink(const char *target, const char *linkpath) {
    (void)target; (void)linkpath; return -1;
}

/* setenv shim */
static inline int setenv(const char *name, const char *value, int overwrite) {
    (void)overwrite;
    return _putenv_s(name, value) == 0 ? 0 : -1;
}

/* mkdtemp shim — creates a unique temp directory */
static inline char *mkdtemp(char *tmpl) {
    size_t len = strlen(tmpl);
    if (len < 6) return NULL;
    char *x = tmpl + len - 6;
    DWORD tick = GetTickCount() ^ (DWORD)(uintptr_t)tmpl;
    for (int i = 0; i < 6; i++)
        x[i] = 'A' + (int)((tick >> (i * 5)) & 0x1F) % 26;
    if (_mkdir(tmpl) != 0) return NULL;
    return tmpl;
}

/* utimes shim using struct timeval from winsock2.h */
static inline int utimes(const char *path, const struct timeval times[2]) {
    struct _utimbuf ut;
    ut.actime  = (time_t)times[0].tv_sec;
    ut.modtime = (time_t)times[1].tv_sec;
    return _utime(path, &ut);
}

#else
#include <sys/time.h>
#include <unistd.h>
#endif

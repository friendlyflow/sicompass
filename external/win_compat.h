#pragma once

#ifdef _MSC_VER

/* Suppress POSIX deprecation warnings */
#ifndef _CRT_SECURE_NO_WARNINGS
#define _CRT_SECURE_NO_WARNINGS
#endif
#ifndef _CRT_NONSTDC_NO_WARNINGS
#define _CRT_NONSTDC_NO_WARNINGS
#endif

/* Make GCC __attribute__ a no-op (covers __attribute__((unused)) etc.) */
#define __attribute__(x)

/* Map POSIX popen/pclose to MSVC equivalents */
#define popen  _popen
#define pclose _pclose

/* Map POSIX strncasecmp to MSVC equivalent */
#define strncasecmp _strnicmp
#define strcasecmp  _stricmp

/* Dynamic loading via Windows API */
#include <windows.h>
#include <io.h>
#define dlopen(path, flags) ((void*)LoadLibraryA(path))
#define dlsym(handle, name) ((void*)GetProcAddress((HMODULE)(handle), (name)))
#define dlclose(handle)     FreeLibrary((HMODULE)(handle))

/* POSIX file/stat shims */
#include <sys/stat.h>
#include <direct.h>
#define mkdir(path, mode) _mkdir(path)
#ifndef S_ISDIR
#define S_ISDIR(m) (((m) & _S_IFMT) == _S_IFDIR)
#endif
#ifndef S_ISREG
#define S_ISREG(m) (((m) & _S_IFMT) == _S_IFREG)
#endif
#define lstat      stat
#define access     _access
#ifndef F_OK
#define F_OK 0
#endif

/* strtok_r → strtok_s on MSVC */
#define strtok_r   strtok_s

/* GCC constructor attribute equivalent for MSVC using .CRT$XCU section */
#define GCC_CONSTRUCTOR(func_name)                                              \
    static void func_name(void);                                               \
    __pragma(section(".CRT$XCU", read))                                        \
    __declspec(allocate(".CRT$XCU")) static void (__cdecl *func_name##_auto_)(void) = func_name; \
    static void func_name(void)

#else

#define GCC_CONSTRUCTOR(func_name) \
    __attribute__((constructor)) static void func_name(void)

#endif /* _MSC_VER */

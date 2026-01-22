#include "platform.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>

#if defined(_WIN32)
    #define PLATFORM_WINDOWS 1
    #include <windows.h>
    #include <shellapi.h>
#elif defined(__APPLE__)
    #define PLATFORM_MACOS 1
#else
    #define PLATFORM_LINUX 1
#endif

bool platformOpenWithDefault(const char *path) {
    if (!path) return false;

#if defined(PLATFORM_WINDOWS)
    // Windows: use ShellExecuteA
    HINSTANCE result = ShellExecuteA(NULL, "open", path, NULL, NULL, SW_SHOWNORMAL);
    return (intptr_t)result > 32;

#elif defined(PLATFORM_MACOS)
    // macOS: use open command
    char *command = malloc(strlen(path) + 32);
    if (!command) return false;

    snprintf(command, strlen(path) + 32, "open \"%s\" &", path);
    int result = system(command);
    free(command);
    return result == 0;

#else
    // Linux: use xdg-open
    char *command = malloc(strlen(path) + 32);
    if (!command) return false;

    snprintf(command, strlen(path) + 32, "xdg-open \"%s\" &", path);
    int result = system(command);
    free(command);
    return result == 0;
#endif
}

char* platformGetConfigHome(void) {
#if defined(PLATFORM_WINDOWS)
    // Windows: use APPDATA
    const char *appData = getenv("APPDATA");
    if (!appData || appData[0] == '\0') {
        return NULL;
    }

    size_t len = strlen(appData) + 2;
    char *result = malloc(len);
    if (!result) return NULL;

    snprintf(result, len, "%s\\", appData);
    return result;

#elif defined(PLATFORM_MACOS)
    // macOS: use ~/Library/Application Support/
    const char *home = getenv("HOME");
    if (!home || home[0] == '\0') {
        return NULL;
    }

    const char *suffix = "/Library/Application Support/";
    size_t len = strlen(home) + strlen(suffix) + 1;
    char *result = malloc(len);
    if (!result) return NULL;

    snprintf(result, len, "%s%s", home, suffix);
    return result;

#else
    // Linux: use XDG_CONFIG_HOME or ~/.config/
    const char *configHome = getenv("XDG_CONFIG_HOME");

    if (configHome && configHome[0] != '\0') {
        size_t len = strlen(configHome) + 2;
        char *result = malloc(len);
        if (!result) return NULL;

        snprintf(result, len, "%s/", configHome);
        return result;
    }

    const char *home = getenv("HOME");
    if (!home || home[0] == '\0') {
        return NULL;
    }

    const char *suffix = "/.config/";
    size_t len = strlen(home) + strlen(suffix) + 1;
    char *result = malloc(len);
    if (!result) return NULL;

    snprintf(result, len, "%s%s", home, suffix);
    return result;
#endif
}

char* platformGetHomeDir(void) {
#if defined(PLATFORM_WINDOWS)
    const char *home = getenv("USERPROFILE");
#else
    const char *home = getenv("HOME");
#endif

    if (!home || home[0] == '\0') {
        return NULL;
    }

    return strdup(home);
}

const char* platformGetPathSeparator(void) {
#if defined(PLATFORM_WINDOWS)
    return "\\";
#else
    return "/";
#endif
}

bool platformIsWindows(void) {
#if defined(PLATFORM_WINDOWS)
    return true;
#else
    return false;
#endif
}

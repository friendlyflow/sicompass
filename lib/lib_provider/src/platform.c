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
    #include <dirent.h>
    #include <sys/stat.h>
#else
    #define PLATFORM_LINUX 1
    #include <dirent.h>
    #include <sys/stat.h>
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

#if defined(PLATFORM_WINDOWS)
    return _strdup(home);
#else
    return strdup(home);
#endif
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

char** platformGetPathExecutables(int *outCount) {
    *outCount = 0;

    const char *pathEnv = getenv("PATH");
    if (!pathEnv || pathEnv[0] == '\0') return NULL;

    char *pathCopy = strdup(pathEnv);
    if (!pathCopy) return NULL;

    int capacity = 256;
    int count = 0;
    char **result = malloc(capacity * sizeof(char*));
    if (!result) {
        free(pathCopy);
        return NULL;
    }

#if defined(PLATFORM_WINDOWS)
    const char *delim = ";";
#else
    const char *delim = ":";
#endif

    char *dir = strtok(pathCopy, delim);
    while (dir != NULL) {
#if defined(PLATFORM_WINDOWS)
        char searchPath[4096];
        snprintf(searchPath, sizeof(searchPath), "%s\\*", dir);
        WIN32_FIND_DATAA findData;
        HANDLE hFind = FindFirstFileA(searchPath, &findData);
        if (hFind != INVALID_HANDLE_VALUE) {
            do {
                if (findData.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) continue;
                const char *name = findData.cFileName;
                size_t len = strlen(name);
                if (len > 4) {
                    const char *ext = name + len - 4;
                    if (_stricmp(ext, ".exe") != 0 &&
                        _stricmp(ext, ".bat") != 0 &&
                        _stricmp(ext, ".cmd") != 0) {
                        continue;
                    }
                } else {
                    continue;
                }

                bool duplicate = false;
                for (int i = 0; i < count; i++) {
                    if (_stricmp(result[i], name) == 0) {
                        duplicate = true;
                        break;
                    }
                }
                if (duplicate) continue;

                if (count >= capacity) {
                    capacity *= 2;
                    char **newResult = realloc(result, capacity * sizeof(char*));
                    if (!newResult) break;
                    result = newResult;
                }
                result[count++] = _strdup(name);
            } while (FindNextFileA(hFind, &findData) != 0);
            FindClose(hFind);
        }
#else
        DIR *d = opendir(dir);
        if (d) {
            struct dirent *entry;
            while ((entry = readdir(d)) != NULL) {
                if (entry->d_name[0] == '.') continue;

                char fullpath[4096];
                snprintf(fullpath, sizeof(fullpath), "%s/%s", dir, entry->d_name);

                struct stat st;
                if (stat(fullpath, &st) != 0) continue;
                if (S_ISDIR(st.st_mode)) continue;
                if (!(st.st_mode & S_IXUSR)) continue;

                bool duplicate = false;
                for (int i = 0; i < count; i++) {
                    if (strcmp(result[i], entry->d_name) == 0) {
                        duplicate = true;
                        break;
                    }
                }
                if (duplicate) continue;

                if (count >= capacity) {
                    capacity *= 2;
                    char **newResult = realloc(result, capacity * sizeof(char*));
                    if (!newResult) break;
                    result = newResult;
                }
                result[count++] = strdup(entry->d_name);
            }
            closedir(d);
        }
#endif
        dir = strtok(NULL, delim);
    }

    free(pathCopy);
    *outCount = count;
    return result;
}

void platformFreePathExecutables(char **executables, int count) {
    if (!executables) return;
    for (int i = 0; i < count; i++) {
        free(executables[i]);
    }
    free(executables);
}

bool platformOpenWith(const char *program, const char *filePath) {
    if (!program || !filePath) return false;

#if defined(PLATFORM_WINDOWS)
    HINSTANCE result = ShellExecuteA(NULL, "open", program, filePath, NULL, SW_SHOWNORMAL);
    return (intptr_t)result > 32;

#elif defined(PLATFORM_MACOS)
    size_t len = strlen(program) + strlen(filePath) + 64;
    char *command = malloc(len);
    if (!command) return false;
    snprintf(command, len, "open -a \"%s\" \"%s\" &", program, filePath);
    int result = system(command);
    free(command);
    return result == 0;

#else
    size_t len = strlen(program) + strlen(filePath) + 16;
    char *command = malloc(len);
    if (!command) return false;
    snprintf(command, len, "\"%s\" \"%s\" &", program, filePath);
    int result = system(command);
    free(command);
    return result == 0;
#endif
}

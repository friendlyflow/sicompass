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

PlatformApplication* platformGetApplications(int *outCount) {
    *outCount = 0;

    int capacity = 128;
    int count = 0;
    PlatformApplication *result = malloc(capacity * sizeof(PlatformApplication));
    if (!result) return NULL;

#if defined(PLATFORM_WINDOWS)
    // Windows: enumerate App Paths registry key
    HKEY hKey;
    if (RegOpenKeyExA(HKEY_LOCAL_MACHINE,
                      "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths",
                      0, KEY_READ, &hKey) != ERROR_SUCCESS) {
        free(result);
        return NULL;
    }

    char subKeyName[256];
    DWORD subKeyLen;
    for (DWORD idx = 0; ; idx++) {
        subKeyLen = sizeof(subKeyName);
        if (RegEnumKeyExA(hKey, idx, subKeyName, &subKeyLen,
                          NULL, NULL, NULL, NULL) != ERROR_SUCCESS) break;

        // Display name: strip .exe extension
        char displayName[256];
        strncpy(displayName, subKeyName, sizeof(displayName) - 1);
        displayName[sizeof(displayName) - 1] = '\0';
        size_t nameLen = strlen(displayName);
        if (nameLen > 4) {
            const char *ext = displayName + nameLen - 4;
            if (_stricmp(ext, ".exe") == 0) {
                displayName[nameLen - 4] = '\0';
            }
        }

        // Deduplicate by display name
        bool duplicate = false;
        for (int i = 0; i < count; i++) {
            if (_stricmp(result[i].name, displayName) == 0) {
                duplicate = true;
                break;
            }
        }
        if (duplicate) continue;

        if (count >= capacity) {
            capacity *= 2;
            PlatformApplication *newResult = realloc(result, capacity * sizeof(PlatformApplication));
            if (!newResult) break;
            result = newResult;
        }

        result[count].name = _strdup(displayName);
        result[count].exec = _strdup(subKeyName);
        count++;
    }

    RegCloseKey(hKey);

#elif defined(PLATFORM_MACOS)
    // macOS: scan /Applications and ~/Applications for .app bundles
    const char *appDirs[3];
    int numDirs = 0;

    appDirs[numDirs++] = "/Applications";

    char userApps[4096];
    const char *home = getenv("HOME");
    if (home && home[0] != '\0') {
        snprintf(userApps, sizeof(userApps), "%s/Applications", home);
        appDirs[numDirs++] = userApps;
    }

    for (int d = 0; d < numDirs; d++) {
        DIR *dir = opendir(appDirs[d]);
        if (!dir) continue;

        struct dirent *entry;
        while ((entry = readdir(dir)) != NULL) {
            if (entry->d_name[0] == '.') continue;

            size_t nameLen = strlen(entry->d_name);
            bool isApp = (nameLen > 4 && strcmp(entry->d_name + nameLen - 4, ".app") == 0);

            if (isApp) {
                // Display name: strip .app suffix
                char displayName[256];
                size_t dispLen = nameLen - 4;
                if (dispLen >= sizeof(displayName)) dispLen = sizeof(displayName) - 1;
                memcpy(displayName, entry->d_name, dispLen);
                displayName[dispLen] = '\0';

                // Deduplicate
                bool duplicate = false;
                for (int i = 0; i < count; i++) {
                    if (strcmp(result[i].name, displayName) == 0) {
                        duplicate = true;
                        break;
                    }
                }
                if (duplicate) continue;

                if (count >= capacity) {
                    capacity *= 2;
                    PlatformApplication *newResult = realloc(result, capacity * sizeof(PlatformApplication));
                    if (!newResult) { closedir(dir); goto done; }
                    result = newResult;
                }

                result[count].name = strdup(displayName);
                result[count].exec = strdup(displayName);
                count++;
            } else {
                // Check subdirectories (e.g., /Applications/Utilities/) one level deep
                char subPath[4096];
                snprintf(subPath, sizeof(subPath), "%s/%s", appDirs[d], entry->d_name);

                struct stat st;
                if (stat(subPath, &st) != 0 || !S_ISDIR(st.st_mode)) continue;

                DIR *subDir = opendir(subPath);
                if (!subDir) continue;

                struct dirent *subEntry;
                while ((subEntry = readdir(subDir)) != NULL) {
                    if (subEntry->d_name[0] == '.') continue;
                    size_t subLen = strlen(subEntry->d_name);
                    if (subLen <= 4 || strcmp(subEntry->d_name + subLen - 4, ".app") != 0) continue;

                    char displayName[256];
                    size_t dispLen = subLen - 4;
                    if (dispLen >= sizeof(displayName)) dispLen = sizeof(displayName) - 1;
                    memcpy(displayName, subEntry->d_name, dispLen);
                    displayName[dispLen] = '\0';

                    bool duplicate = false;
                    for (int i = 0; i < count; i++) {
                        if (strcmp(result[i].name, displayName) == 0) {
                            duplicate = true;
                            break;
                        }
                    }
                    if (duplicate) continue;

                    if (count >= capacity) {
                        capacity *= 2;
                        PlatformApplication *newResult = realloc(result, capacity * sizeof(PlatformApplication));
                        if (!newResult) { closedir(subDir); closedir(dir); goto done; }
                        result = newResult;
                    }

                    result[count].name = strdup(displayName);
                    result[count].exec = strdup(displayName);
                    count++;
                }
                closedir(subDir);
            }
        }
        closedir(dir);
    }

#else
    // Linux: parse .desktop files from XDG application directories
    const char *desktopDirs[3];
    int numDirs = 0;

    desktopDirs[numDirs++] = "/usr/share/applications";
    desktopDirs[numDirs++] = "/usr/local/share/applications";

    char userDesktop[4096];
    const char *home = getenv("HOME");
    if (home && home[0] != '\0') {
        snprintf(userDesktop, sizeof(userDesktop), "%s/.local/share/applications", home);
        desktopDirs[numDirs++] = userDesktop;
    }

    for (int d = 0; d < numDirs; d++) {
        DIR *dir = opendir(desktopDirs[d]);
        if (!dir) continue;

        struct dirent *entry;
        while ((entry = readdir(dir)) != NULL) {
            size_t nameLen = strlen(entry->d_name);
            if (nameLen <= 8 || strcmp(entry->d_name + nameLen - 8, ".desktop") != 0) continue;

            char filepath[4096];
            snprintf(filepath, sizeof(filepath), "%s/%s", desktopDirs[d], entry->d_name);

            FILE *f = fopen(filepath, "r");
            if (!f) continue;

            char appName[256] = {0};
            char appExec[4096] = {0};
            bool noDisplay = false;
            bool hidden = false;
            bool typeApp = false;
            bool inDesktopEntry = false;
            char line[4096];

            while (fgets(line, sizeof(line), f)) {
                // Strip trailing newline
                size_t len = strlen(line);
                while (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r'))
                    line[--len] = '\0';

                // Track sections
                if (line[0] == '[') {
                    inDesktopEntry = (strcmp(line, "[Desktop Entry]") == 0);
                    continue;
                }
                if (!inDesktopEntry) continue;

                if (strncmp(line, "Name=", 5) == 0 && appName[0] == '\0') {
                    strncpy(appName, line + 5, sizeof(appName) - 1);
                } else if (strncmp(line, "Exec=", 5) == 0 && appExec[0] == '\0') {
                    strncpy(appExec, line + 5, sizeof(appExec) - 1);
                } else if (strncmp(line, "NoDisplay=", 10) == 0) {
                    noDisplay = (strcmp(line + 10, "true") == 0);
                } else if (strncmp(line, "Hidden=", 7) == 0) {
                    hidden = (strcmp(line + 7, "true") == 0);
                } else if (strncmp(line, "Type=", 5) == 0) {
                    typeApp = (strcmp(line + 5, "Application") == 0);
                }
            }
            fclose(f);

            if (!typeApp || noDisplay || hidden || appName[0] == '\0' || appExec[0] == '\0')
                continue;

            // Strip field codes (%f, %F, %u, %U, %d, %D, %n, %N, %i, %c, %k, %v, %m) from exec
            char cleanExec[4096];
            int ci = 0;
            for (int i = 0; appExec[i] != '\0' && ci < (int)sizeof(cleanExec) - 1; i++) {
                if (appExec[i] == '%' && appExec[i + 1] != '\0') {
                    char code = appExec[i + 1];
                    if (code == 'f' || code == 'F' || code == 'u' || code == 'U' ||
                        code == 'd' || code == 'D' || code == 'n' || code == 'N' ||
                        code == 'i' || code == 'c' || code == 'k' || code == 'v' ||
                        code == 'm') {
                        i++; // Skip the code character
                        // Also skip trailing space
                        if (appExec[i + 1] == ' ') i++;
                        continue;
                    }
                }
                cleanExec[ci++] = appExec[i];
            }
            cleanExec[ci] = '\0';

            // Trim trailing whitespace
            while (ci > 0 && cleanExec[ci - 1] == ' ') cleanExec[--ci] = '\0';

            if (cleanExec[0] == '\0') continue;

            // Deduplicate by exec command
            bool duplicate = false;
            for (int i = 0; i < count; i++) {
                if (strcmp(result[i].exec, cleanExec) == 0) {
                    duplicate = true;
                    break;
                }
            }
            if (duplicate) continue;

            if (count >= capacity) {
                capacity *= 2;
                PlatformApplication *newResult = realloc(result, capacity * sizeof(PlatformApplication));
                if (!newResult) { closedir(dir); goto done; }
                result = newResult;
            }

            result[count].name = strdup(appName);
            result[count].exec = strdup(cleanExec);
            count++;
        }
        closedir(dir);
    }
#endif

done:
    *outCount = count;
    return result;
}

void platformFreeApplications(PlatformApplication *apps, int count) {
    if (!apps) return;
    for (int i = 0; i < count; i++) {
        free(apps[i].name);
        free(apps[i].exec);
    }
    free(apps);
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

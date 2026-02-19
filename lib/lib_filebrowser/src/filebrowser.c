#include "filebrowser.h"
#include <provider_tags.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#if defined(_WIN32)
    #include <windows.h>
    #include <sys/stat.h>
    #define stat _stat
    #define S_ISDIR(m) (((m) & _S_IFMT) == _S_IFDIR)
    #define S_IXUSR 0100
#else
    #include <dirent.h>
    #include <sys/stat.h>
    #include <unistd.h>
    #include <pwd.h>
    #include <grp.h>
#endif

#if !defined(_WIN32)
static void filebrowserFormatProperties(struct stat *st, char *buf, size_t bufSize) {
    // Permissions string (e.g. "drwxr-xr-x")
    char perm[11];
    perm[0] = S_ISDIR(st->st_mode) ? 'd' : (S_ISLNK(st->st_mode) ? 'l' : '-');
    perm[1] = (st->st_mode & S_IRUSR) ? 'r' : '-';
    perm[2] = (st->st_mode & S_IWUSR) ? 'w' : '-';
    perm[3] = (st->st_mode & S_IXUSR) ? 'x' : '-';
    perm[4] = (st->st_mode & S_IRGRP) ? 'r' : '-';
    perm[5] = (st->st_mode & S_IWGRP) ? 'w' : '-';
    perm[6] = (st->st_mode & S_IXGRP) ? 'x' : '-';
    perm[7] = (st->st_mode & S_IROTH) ? 'r' : '-';
    perm[8] = (st->st_mode & S_IWOTH) ? 'w' : '-';
    perm[9] = (st->st_mode & S_IXOTH) ? 'x' : '-';
    perm[10] = '\0';

    // Owner and group names (fall back to numeric ids)
    const char *owner = "?";
    const char *group = "?";
    struct passwd *pw = getpwuid(st->st_uid);
    struct group  *gr = getgrgid(st->st_gid);
    char ownerBuf[32], groupBuf[32];
    if (pw) {
        owner = pw->pw_name;
    } else {
        snprintf(ownerBuf, sizeof(ownerBuf), "%u", (unsigned)st->st_uid);
        owner = ownerBuf;
    }
    if (gr) {
        group = gr->gr_name;
    } else {
        snprintf(groupBuf, sizeof(groupBuf), "%u", (unsigned)st->st_gid);
        group = groupBuf;
    }

    // Date: "Mon DD HH:MM" for recent, "Mon DD  YYYY" for older
    char dateBuf[16];
    time_t now = time(NULL);
    struct tm *tm_info = localtime(&st->st_mtime);
    if (now - st->st_mtime < 6 * 30 * 24 * 3600) {
        strftime(dateBuf, sizeof(dateBuf), "%b %e %H:%M", tm_info);
    } else {
        strftime(dateBuf, sizeof(dateBuf), "%b %e  %Y", tm_info);
    }

    snprintf(buf, bufSize, "%s %2lu %-8s %-8s %5lld %s ",
             perm,
             (unsigned long)st->st_nlink,
             owner,
             group,
             (long long)st->st_size,
             dateBuf);
}
#endif

FfonElement** filebrowserListDirectory(const char *uri, bool commands, bool showProperties, int *out_count) {
    *out_count = 0;

    if (uri == NULL) {
        return NULL;
    }

#if defined(_WIN32)
    // Windows: path should start with drive letter or be a UNC path
    if (strlen(uri) < 2) {
        return NULL;
    }
#else
    // Unix: path should start with /
    if (uri[0] != '/') {
        return NULL;
    }
#endif

    int capacity = 16;
    int count = 0;
    FfonElement **elements = malloc(capacity * sizeof(FfonElement*));
    if (elements == NULL) {
        return NULL;
    }

#if defined(_WIN32)
    // Windows directory listing using FindFirstFile/FindNextFile
    char searchPath[4096];
    snprintf(searchPath, sizeof(searchPath), "%s\\*", uri);

    WIN32_FIND_DATAA findData;
    HANDLE hFind = FindFirstFileA(searchPath, &findData);
    if (hFind == INVALID_HANDLE_VALUE) {
        free(elements);
        return NULL;
    }

    do {
        const char *name = findData.cFileName;

        if (strcmp(name, ".") == 0 || strcmp(name, "..") == 0) {
            continue;
        }

        bool is_dir = (findData.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0;

        // Skip executables if not showing commands (simplified check for Windows)
        bool is_executable = false;
        if (!is_dir) {
            size_t len = strlen(name);
            if (len > 4) {
                const char *ext = name + len - 4;
                is_executable = (_stricmp(ext, ".exe") == 0 ||
                                _stricmp(ext, ".bat") == 0 ||
                                _stricmp(ext, ".cmd") == 0);
            }
        }

        if (!commands && is_executable) {
            continue;
        }

        if (count >= capacity) {
            capacity *= 2;
            FfonElement **new_elements = realloc(elements, capacity * sizeof(FfonElement*));
            if (new_elements == NULL) {
                for (int i = 0; i < count; i++) {
                    ffonElementDestroy(elements[i]);
                }
                free(elements);
                FindClose(hFind);
                return NULL;
            }
            elements = new_elements;
        }

        // Build properties prefix for Windows (size + date)
        char propBuf[64] = "";
        if (showProperties) {
            SYSTEMTIME st;
            FileTimeToSystemTime(&findData.ftLastWriteTime, &st);
            ULARGE_INTEGER size;
            size.LowPart  = findData.nFileSizeLow;
            size.HighPart = findData.nFileSizeHigh;
            snprintf(propBuf, sizeof(propBuf), "%8llu %04d-%02d-%02d %02d:%02d ",
                     (unsigned long long)size.QuadPart,
                     st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute);
        }

        FfonElement *elem;
        if (is_dir) {
            char key_with_tags[1024];
            snprintf(key_with_tags, sizeof(key_with_tags), "%s%s%s%s",
                     propBuf, INPUT_TAG_OPEN, name, INPUT_TAG_CLOSE);
            elem = ffonElementCreateObject(key_with_tags);
        } else {
            char name_with_tags[1024];
            snprintf(name_with_tags, sizeof(name_with_tags), "%s%s%s%s",
                     propBuf, INPUT_TAG_OPEN, name, INPUT_TAG_CLOSE);
            elem = ffonElementCreateString(name_with_tags);
        }

        if (elem == NULL) {
            continue;
        }

        elements[count++] = elem;
    } while (FindNextFileA(hFind, &findData) != 0);

    FindClose(hFind);

#else
    // POSIX directory listing
    DIR *dir = opendir(uri);
    if (dir == NULL) {
        free(elements);
        return NULL;
    }

    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }

        char fullpath[4096];
        snprintf(fullpath, sizeof(fullpath), "%s/%s", uri, entry->d_name);

        struct stat st;
        if (stat(fullpath, &st) != 0) {
            continue;
        }

        bool is_dir = S_ISDIR(st.st_mode);
        bool is_executable = !is_dir && (st.st_mode & S_IXUSR);

        if (!commands && is_executable) {
            continue;
        }

        if (count >= capacity) {
            capacity *= 2;
            FfonElement **new_elements = realloc(elements, capacity * sizeof(FfonElement*));
            if (new_elements == NULL) {
                for (int i = 0; i < count; i++) {
                    ffonElementDestroy(elements[i]);
                }
                free(elements);
                closedir(dir);
                return NULL;
            }
            elements = new_elements;
        }

        // Build properties prefix (ls -al style)
        char propBuf[128] = "";
        if (showProperties) {
            filebrowserFormatProperties(&st, propBuf, sizeof(propBuf));
        }

        FfonElement *elem;
        if (is_dir) {
            char key_with_tags[1024];
            snprintf(key_with_tags, sizeof(key_with_tags), "%s%s%s%s",
                     propBuf, INPUT_TAG_OPEN, entry->d_name, INPUT_TAG_CLOSE);
            elem = ffonElementCreateObject(key_with_tags);
        } else {
            char name_with_tags[1024];
            snprintf(name_with_tags, sizeof(name_with_tags), "%s%s%s%s",
                     propBuf, INPUT_TAG_OPEN, entry->d_name, INPUT_TAG_CLOSE);
            elem = ffonElementCreateString(name_with_tags);
        }

        if (elem == NULL) {
            continue;
        }

        elements[count++] = elem;
    }

    closedir(dir);
#endif

    *out_count = count;
    return elements;
}

bool filebrowserRename(const char *uri, const char *oldName, const char *newName) {
    if (!uri || !oldName || !newName) {
        return false;
    }

    // Build full paths
    char oldPath[4096];
    char newPath[4096];

    // Handle trailing slash for directories
    size_t oldLen = strlen(oldName);
    size_t newLen = strlen(newName);

    // Create copies without trailing slash for path construction
    char oldNameClean[512];
    char newNameClean[512];
    strncpy(oldNameClean, oldName, sizeof(oldNameClean) - 1);
    oldNameClean[sizeof(oldNameClean) - 1] = '\0';
    strncpy(newNameClean, newName, sizeof(newNameClean) - 1);
    newNameClean[sizeof(newNameClean) - 1] = '\0';

    // Remove trailing slash if present
    if (oldLen > 0 && (oldNameClean[oldLen - 1] == '/' || oldNameClean[oldLen - 1] == '\\')) {
        oldNameClean[oldLen - 1] = '\0';
    }
    if (newLen > 0 && (newNameClean[newLen - 1] == '/' || newNameClean[newLen - 1] == '\\')) {
        newNameClean[newLen - 1] = '\0';
    }

#if defined(_WIN32)
    snprintf(oldPath, sizeof(oldPath), "%s\\%s", uri, oldNameClean);
    snprintf(newPath, sizeof(newPath), "%s\\%s", uri, newNameClean);
#else
    snprintf(oldPath, sizeof(oldPath), "%s/%s", uri, oldNameClean);
    snprintf(newPath, sizeof(newPath), "%s/%s", uri, newNameClean);
#endif

    // Use rename() - works on both Windows and POSIX
    if (rename(oldPath, newPath) != 0) {
        perror("filebrowserRename");
        return false;
    }

    return true;
}

bool filebrowserCreateDirectory(const char *uri, const char *name) {
    if (!uri || !name || name[0] == '\0') return false;

    char fullpath[4096];
#if defined(_WIN32)
    snprintf(fullpath, sizeof(fullpath), "%s\\%s", uri, name);
    return CreateDirectoryA(fullpath, NULL) != 0;
#else
    snprintf(fullpath, sizeof(fullpath), "%s/%s", uri, name);
    return mkdir(fullpath, 0755) == 0;
#endif
}

bool filebrowserCreateFile(const char *uri, const char *name) {
    if (!uri || !name || name[0] == '\0') return false;

    char fullpath[4096];
#if defined(_WIN32)
    snprintf(fullpath, sizeof(fullpath), "%s\\%s", uri, name);
#else
    snprintf(fullpath, sizeof(fullpath), "%s/%s", uri, name);
#endif

    FILE *f = fopen(fullpath, "w");
    if (!f) return false;
    fclose(f);
    return true;
}


#include "filebrowser.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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
#endif

#define INPUT_TAG_OPEN "<input>"
#define INPUT_TAG_CLOSE "</input>"
#define INPUT_TAG_OPEN_LEN 7
#define INPUT_TAG_CLOSE_LEN 8

FfonElement** filebrowserListDirectory(const char *uri, bool commands, int *out_count) {
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

        FfonElement *elem;
        if (is_dir) {
            char key_with_tags[512];
            snprintf(key_with_tags, sizeof(key_with_tags), "%s%s%s",
                     INPUT_TAG_OPEN, name, INPUT_TAG_CLOSE);
            elem = ffonElementCreateObject(key_with_tags);
        } else {
            char name_with_tags[512];
            snprintf(name_with_tags, sizeof(name_with_tags), "%s%s%s",
                     INPUT_TAG_OPEN, name, INPUT_TAG_CLOSE);
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

        FfonElement *elem;
        if (is_dir) {
            char key_with_tags[512];
            snprintf(key_with_tags, sizeof(key_with_tags), "%s%s%s",
                     INPUT_TAG_OPEN, entry->d_name, INPUT_TAG_CLOSE);
            elem = ffonElementCreateObject(key_with_tags);
        } else {
            char name_with_tags[512];
            snprintf(name_with_tags, sizeof(name_with_tags), "%s%s%s",
                     INPUT_TAG_OPEN, entry->d_name, INPUT_TAG_CLOSE);
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

bool filebrowserHasInputTags(const char *text) {
    if (!text) return false;
    return strstr(text, INPUT_TAG_OPEN) != NULL && strstr(text, INPUT_TAG_CLOSE) != NULL;
}

char* filebrowserExtractInputContent(const char *text) {
    if (!text) return NULL;

    const char *start = strstr(text, INPUT_TAG_OPEN);
    if (!start) return NULL;

    start += INPUT_TAG_OPEN_LEN;

    const char *end = strstr(start, INPUT_TAG_CLOSE);
    if (!end) return NULL;

    size_t len = end - start;
    char *result = malloc(len + 1);
    if (!result) return NULL;

    strncpy(result, start, len);
    result[len] = '\0';

    return result;
}

char* filebrowserStripInputTags(const char *text) {
    if (!text) return NULL;

    // If no input tags, just return a copy
    if (!filebrowserHasInputTags(text)) {
#if defined(_WIN32)
        return _strdup(text);
#else
        return strdup(text);
#endif
    }

    // Find tag positions
    const char *openTag = strstr(text, INPUT_TAG_OPEN);
    const char *closeTag = strstr(text, INPUT_TAG_CLOSE);

    if (!openTag || !closeTag) {
#if defined(_WIN32)
        return _strdup(text);
#else
        return strdup(text);
#endif
    }

    // Calculate result length: original - open tag length - close tag length
    size_t textLen = strlen(text);
    size_t resultLen = textLen - INPUT_TAG_OPEN_LEN - INPUT_TAG_CLOSE_LEN;

    char *result = malloc(resultLen + 1);
    if (!result) return NULL;

    // Copy parts: before open tag + between tags + after close tag
    size_t pos = 0;

    // Copy before <input>
    size_t beforeLen = openTag - text;
    strncpy(result + pos, text, beforeLen);
    pos += beforeLen;

    // Copy between tags
    const char *contentStart = openTag + INPUT_TAG_OPEN_LEN;
    size_t contentLen = closeTag - contentStart;
    strncpy(result + pos, contentStart, contentLen);
    pos += contentLen;

    // Copy after </input>
    const char *afterClose = closeTag + INPUT_TAG_CLOSE_LEN;
    strcpy(result + pos, afterClose);

    return result;
}

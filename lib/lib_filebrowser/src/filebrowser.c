#include "filebrowser.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>
#include <sys/stat.h>
#include <unistd.h>

FfonElement** filebrowserListDirectory(const char *uri, bool commands, int *out_count) {
    *out_count = 0;

    if (uri == NULL || uri[0] != '/') {
        return NULL;
    }

    DIR *dir = opendir(uri);
    if (dir == NULL) {
        return NULL;
    }

    int capacity = 16;
    int count = 0;
    FfonElement **elements = malloc(capacity * sizeof(FfonElement*));
    if (elements == NULL) {
        closedir(dir);
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
            char key_with_slash[257];
            snprintf(key_with_slash, sizeof(key_with_slash), "%s/", entry->d_name);
            elem = ffonElementCreateObject(key_with_slash);
        } else {
            elem = ffonElementCreateString(entry->d_name);
        }

        if (elem == NULL) {
            continue;
        }

        elements[count++] = elem;
    }

    closedir(dir);
    *out_count = count;
    return elements;
}

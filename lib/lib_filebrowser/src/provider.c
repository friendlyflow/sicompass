#include <filebrowser.h>
#include <filebrowser_provider.h>
#include <provider_tags.h>
#include <platform.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

// Fetch children at current path
static FfonElement** fbFetch(const char *path, int *outCount) {
    return filebrowserListDirectory(path, false, outCount);
}

// Commit a rename operation
static bool fbCommit(const char *path, const char *oldName, const char *newName) {
    return filebrowserRename(path, oldName, newName);
}

// Create a directory
static bool fbCreateDirectory(const char *path, const char *name) {
    return filebrowserCreateDirectory(path, name);
}

// Create a file
static bool fbCreateFile(const char *path, const char *name) {
    return filebrowserCreateFile(path, name);
}

// Stored file path between handleCommand and executeCommand for "open with"
static char fb_openWithPath[4096];

static const char *fb_commands[] = {
    "create directory",
    "create file",
    "open file with"
};

static const char** fbGetCommands(int *outCount) {
    *outCount = 3;
    return fb_commands;
}

static FfonElement* fbHandleCommand(const char *path, const char *command,
                                     const char *elementKey, int elementType,
                                     char *errorMsg, int errorMsgSize) {
    if (strcmp(command, "create directory") == 0) {
        FfonElement *elem = ffonElementCreateObject("<input></input>");
        ffonObjectAddElement(elem->data.object, ffonElementCreateString("<input></input>"));
        return elem;
    }
    if (strcmp(command, "create file") == 0) {
        return ffonElementCreateString("<input></input>");
    }
    if (strcmp(command, "open file with") == 0) {
        if (elementType != FFON_STRING) {
            snprintf(errorMsg, errorMsgSize, "open with: select a file, not a directory");
            return NULL;
        }
        char *filename = providerTagExtractContent(elementKey);
        if (!filename) {
            snprintf(errorMsg, errorMsgSize, "open with: could not extract filename");
            return NULL;
        }
        const char *sep = platformGetPathSeparator();
        snprintf(fb_openWithPath, sizeof(fb_openWithPath), "%s%s%s", path, sep, filename);
        free(filename);
        return NULL;
    }
    return NULL;
}

static ProviderListItem* fbGetCommandListItems(const char *path __attribute__((unused)), const char *command, int *outCount) {
    if (strcmp(command, "open file with") != 0) {
        *outCount = 0;
        return NULL;
    }
    int appCount = 0;
    PlatformApplication *apps = platformGetApplications(&appCount);
    if (!apps || appCount == 0) {
        platformFreeApplications(apps, appCount);
        *outCount = 0;
        return NULL;
    }
    ProviderListItem *items = malloc(appCount * sizeof(ProviderListItem));
    if (!items) {
        platformFreeApplications(apps, appCount);
        *outCount = 0;
        return NULL;
    }
    for (int i = 0; i < appCount; i++) {
        items[i].label = strdup(apps[i].name);
        items[i].data = strdup(apps[i].exec);
    }
    platformFreeApplications(apps, appCount);
    *outCount = appCount;
    return items;
}

static bool fbExecuteCommand(const char *path __attribute__((unused)), const char *command, const char *selection) {
    if (strcmp(command, "open file with") == 0) {
        return platformOpenWith(selection, fb_openWithPath);
    }
    return false;
}

// Provider singleton
static Provider *g_provider = NULL;

Provider* filebrowserGetProvider(void) {
    if (!g_provider) {
        static ProviderOps ops = {
            .name = "filebrowser",
            .displayName = "file browser",
            .fetch = fbFetch,
            .commit = fbCommit,
            .createDirectory = fbCreateDirectory,
            .createFile = fbCreateFile,
            .getCommands = fbGetCommands,
            .handleCommand = fbHandleCommand,
            .getCommandListItems = fbGetCommandListItems,
            .executeCommand = fbExecuteCommand,
        };
        g_provider = providerCreate(&ops);
    }
    return g_provider;
}

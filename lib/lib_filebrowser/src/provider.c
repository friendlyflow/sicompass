#include <filebrowser.h>
#include <filebrowser_provider.h>
#include <provider_tags.h>
#include <platform.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <dirent.h>
#include <sys/stat.h>

// Toggle for show/hide properties command
static bool g_showProperties = false;

// Current sort mode (session-only, initialised from settings on startup)
static FilebrowserSortMode g_sortMode = FILEBROWSER_SORT_ALPHA;

// Fetch children at current path
static FfonElement** fbFetch(const char *path, int *outCount) {
    return filebrowserListDirectory(path, false, g_showProperties, g_sortMode, outCount);
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

// Delete a file or directory
static bool fbDeleteItem(const char *path, const char *name) {
    return filebrowserDelete(path, name);
}

// Stored file path between handleCommand and executeCommand for "open with"
static char fb_openWithPath[4096];

static const char *fb_commands[] = {
    "create directory",
    "create file",
    "open file with",
    "show/hide properties",
    "sort alphanumerically",
    "sort chronologically"
};

static const char** fbGetCommands(int *outCount) {
    *outCount = 6;
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
    if (strcmp(command, "show/hide properties") == 0) {
        g_showProperties = !g_showProperties;
        return NULL;
    }
    if (strcmp(command, "sort alphanumerically") == 0) {
        g_sortMode = FILEBROWSER_SORT_ALPHA;
        return NULL;
    }
    if (strcmp(command, "sort chronologically") == 0) {
        g_sortMode = FILEBROWSER_SORT_CHRONO;
        return NULL;
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

// Deep search implementation

#define FILEBROWSER_MAX_DEEP_ITEMS 50000

typedef struct {
    SearchResultItem *items;
    int count;
    int cap;
} DeepCtx;

static bool deepCtxAppend(DeepCtx *ctx, char *label, char *breadcrumb, char *navPath) {
    if (ctx->count >= ctx->cap) {
        int newCap = ctx->cap == 0 ? 256 : ctx->cap * 2;
        SearchResultItem *newItems = realloc(ctx->items, newCap * sizeof(SearchResultItem));
        if (!newItems) {
            free(label); free(breadcrumb); free(navPath);
            return false;
        }
        ctx->items = newItems;
        ctx->cap = newCap;
    }
    ctx->items[ctx->count].label = label;
    ctx->items[ctx->count].breadcrumb = breadcrumb;
    ctx->items[ctx->count].navPath = navPath;
    ctx->count++;
    return true;
}

// BFS queue node for deep search
typedef struct BfsNode {
    char *dirPath;
    char *breadcrumb;
    struct BfsNode *next;
} BfsNode;

static SearchResultItem* fbCollectDeepSearchItems(const char *rootPath, int *outCount) {
    DeepCtx ctx = {NULL, 0, 0};

    // Seed the BFS queue with the root directory
    BfsNode *head = malloc(sizeof(BfsNode));
    if (!head) { *outCount = 0; return NULL; }
    head->dirPath = strdup(rootPath);
    head->breadcrumb = strdup("");
    head->next = NULL;
    BfsNode *tail = head;

    while (head && ctx.count < FILEBROWSER_MAX_DEEP_ITEMS) {
        // Pop from front of queue
        BfsNode *node = head;
        head = head->next;
        if (!head) tail = NULL;

        DIR *dir = opendir(node->dirPath);
        if (!dir) { free(node->dirPath); free(node->breadcrumb); free(node); continue; }

        struct dirent *entry;
        while ((entry = readdir(dir)) != NULL && ctx.count < FILEBROWSER_MAX_DEEP_ITEMS) {
            if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) continue;

            char fullPath[4096];
            const char *sep = (node->dirPath[strlen(node->dirPath) - 1] == '/') ? "" : "/";
            snprintf(fullPath, sizeof(fullPath), "%s%s%s", node->dirPath, sep, entry->d_name);

            // Use lstat to avoid following symlinks (circular symlinks cause infinite loops)
            struct stat st;
            bool isDir = (lstat(fullPath, &st) == 0 && S_ISDIR(st.st_mode));

            char *label = malloc(strlen(entry->d_name) + 3); // "prefix name\0"
            if (!label) continue;
            snprintf(label, strlen(entry->d_name) + 3, "%s %s", isDir ? "+" : "-", entry->d_name);

            char *bc = strdup(node->breadcrumb);
            char *navPath = strdup(fullPath);
            if (!bc || !navPath) { free(label); free(bc); free(navPath); continue; }

            deepCtxAppend(&ctx, label, bc, navPath);

            // Enqueue subdirectories for later processing (BFS: process siblings before children)
            if (isDir) {
                BfsNode *child = malloc(sizeof(BfsNode));
                if (child) {
                    char newBreadcrumb[4096];
                    if (node->breadcrumb[0] != '\0')
                        snprintf(newBreadcrumb, sizeof(newBreadcrumb), "%s%s > ", node->breadcrumb, entry->d_name);
                    else
                        snprintf(newBreadcrumb, sizeof(newBreadcrumb), "%s > ", entry->d_name);
                    child->dirPath = strdup(fullPath);
                    child->breadcrumb = strdup(newBreadcrumb);
                    child->next = NULL;
                    if (tail) tail->next = child; else head = child;
                    tail = child;
                }
            }
        }
        closedir(dir);
        free(node->dirPath); free(node->breadcrumb); free(node);
    }

    // Free any remaining queue entries if cap was hit
    while (head) {
        BfsNode *node = head; head = head->next;
        free(node->dirPath); free(node->breadcrumb); free(node);
    }

    *outCount = ctx.count;
    return ctx.items;
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
            .deleteItem = fbDeleteItem,
            .getCommands = fbGetCommands,
            .handleCommand = fbHandleCommand,
            .getCommandListItems = fbGetCommandListItems,
            .executeCommand = fbExecuteCommand,
            .collectDeepSearchItems = fbCollectDeepSearchItems,
        };
        g_provider = providerCreate(&ops);
    }
    return g_provider;
}

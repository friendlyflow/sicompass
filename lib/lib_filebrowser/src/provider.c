#include <filebrowser.h>
#include <filebrowser_provider.h>

// Fetch children at current path
static FfonElement** fbFetch(const char *path, int *outCount) {
    return filebrowserListDirectory(path, false, outCount);
}

// Commit a rename operation
static bool fbCommit(const char *path, const char *oldName, const char *newName) {
    return filebrowserRename(path, oldName, newName);
}

// Provider singleton
static Provider *g_provider = NULL;

Provider* filebrowserGetProvider(void) {
    if (!g_provider) {
        static ProviderOps ops = {
            .name = "filebrowser",
            .displayName = "file browser",
            .tagPrefix = "<input>",
            .fetch = fbFetch,
            .commit = fbCommit,
        };
        g_provider = providerCreate(&ops);
    }
    return g_provider;
}

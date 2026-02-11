#include "provider.h"
#include "view.h"
#include <provider_tags.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_PROVIDERS 16

// Provider registry
static Provider *g_providers[MAX_PROVIDERS];
static int g_providerCount = 0;

// Register a provider
void providerRegister(Provider *provider) {
    if (!provider || g_providerCount >= MAX_PROVIDERS) {
        return;
    }
    g_providers[g_providerCount++] = provider;
}

// Get provider by name
Provider* providerFindByName(const char *name) {
    if (!name) return NULL;

    for (int i = 0; i < g_providerCount; i++) {
        if (g_providers[i]->name && strcmp(g_providers[i]->name, name) == 0) {
            return g_providers[i];
        }
    }
    return NULL;
}

// Initialize all providers
void providerInitAll(void) {
    for (int i = 0; i < g_providerCount; i++) {
        if (g_providers[i]->init) {
            g_providers[i]->init(g_providers[i]);
        }
    }
}

// Cleanup all providers
void providerCleanupAll(void) {
    for (int i = 0; i < g_providerCount; i++) {
        if (g_providers[i]->cleanup) {
            g_providers[i]->cleanup(g_providers[i]);
        }
    }
}

// Get active provider from navigation context
Provider* providerGetActive(AppRenderer *appRenderer) {
    if (!appRenderer || appRenderer->currentId.depth < 1) return NULL;
    int rootIndex = appRenderer->currentId.ids[0];
    if (rootIndex < 0 || rootIndex >= appRenderer->ffonCount) return NULL;
    if (!appRenderer->providers) return NULL;
    return appRenderer->providers[rootIndex];
}

// Get current path from active provider
const char* providerGetCurrentPath(AppRenderer *appRenderer) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->getCurrentPath) return NULL;
    return provider->getCurrentPath(provider);
}

// Commit edit operation
bool providerCommitEdit(AppRenderer *appRenderer, const char *oldContent, const char *newContent) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->commitEdit) return false;
    return provider->commitEdit(provider, oldContent, newContent);
}

// Create a directory
bool providerCreateDirectory(AppRenderer *appRenderer, const char *name) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->createDirectory) return false;
    return provider->createDirectory(provider, name);
}

// Create a file
bool providerCreateFile(AppRenderer *appRenderer, const char *name) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->createFile) return false;
    return provider->createFile(provider, name);
}

// Get commands from active provider
const char** providerGetCommands(AppRenderer *appRenderer, int *outCount) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->getCommands) { *outCount = 0; return NULL; }
    return provider->getCommands(provider, outCount);
}

// Handle a provider command
FfonElement* providerHandleCommand(AppRenderer *appRenderer, const char *command,
                                    const char *elementKey, int elementType,
                                    char *errorMsg, int errorMsgSize) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->handleCommand) return NULL;
    return provider->handleCommand(provider, command, elementKey, elementType,
                                    errorMsg, errorMsgSize);
}

// Get list items for a command's secondary selection
ProviderListItem* providerGetCommandListItems(AppRenderer *appRenderer, const char *command, int *outCount) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->getCommandListItems) { *outCount = 0; return NULL; }
    return provider->getCommandListItems(provider, command, outCount);
}

// Execute a command with selection
bool providerExecuteCommand(AppRenderer *appRenderer, const char *command, const char *selection) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->executeCommand) return false;
    return provider->executeCommand(provider, command, selection);
}

// Navigate right into an object
bool providerNavigateRight(AppRenderer *appRenderer) {
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                     &appRenderer->currentId, &count);
    if (!arr || count == 0) {
        return false;
    }

    int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (idx < 0 || idx >= count) {
        return false;
    }

    FfonElement *elem = arr[idx];
    if (elem->type != FFON_OBJECT) {
        return false;
    }

    FfonObject *obj = elem->data.object;
    const char *key = obj->key;

    // Get active provider and extract content from tagged key
    Provider *provider = providerGetActive(appRenderer);
    char *strippedKey = providerTagExtractContent(key);

    // If object already has children, just navigate into it
    if (obj->count > 0) {
        if (provider && provider->pushPath && strippedKey) {
            provider->pushPath(provider, strippedKey);
        }
        free(strippedKey);
        idArrayPush(&appRenderer->currentId, 0);
        return true;
    }

    // Fetch children from provider
    if (!provider || !provider->fetch) {
        free(strippedKey);
        return false;
    }

    // Update path before fetching
    if (provider->pushPath && strippedKey) {
        provider->pushPath(provider, strippedKey);
        printf("providerNavigateRight: key='%s', path='%s'\n",
               strippedKey, provider->getCurrentPath ? provider->getCurrentPath(provider) : "?");
    }
    free(strippedKey);

    // Fetch children
    int childCount = 0;
    FfonElement **children = provider->fetch(provider, &childCount);
    printf("providerNavigateRight: fetched %d children\n", childCount);

    if (!children || childCount == 0) {
        // Empty directory: add placeholder child so user can create files
        if (children) free(children);
        ffonObjectAddElement(obj, ffonElementCreateString(INPUT_TAG_OPEN INPUT_TAG_CLOSE));
    } else {
        for (int i = 0; i < childCount; i++) {
            ffonObjectAddElement(obj, children[i]);
        }
        free(children);
    }

    idArrayPush(&appRenderer->currentId, 0);
    return true;
}

// Navigate left out of an object
bool providerNavigateLeft(AppRenderer *appRenderer) {
    if (appRenderer->currentId.depth <= 1) {
        return false;
    }

    // Pop path on the active provider
    Provider *provider = providerGetActive(appRenderer);
    if (provider && provider->popPath) {
        provider->popPath(provider);
    }

    idArrayPop(&appRenderer->currentId);
    return true;
}

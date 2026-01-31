#include "provider.h"
#include "view.h"
#include <filebrowser.h>
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

// Find provider that can handle an element
Provider* providerFindForElement(const char *elementKey) {
    if (!elementKey) return NULL;

    for (int i = 0; i < g_providerCount; i++) {
        if (g_providers[i]->canHandle &&
            g_providers[i]->canHandle(g_providers[i], elementKey)) {
            return g_providers[i];
        }
    }
    return NULL;
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

// Get current path for an element's provider
const char* providerGetCurrentPath(const char *elementKey) {
    Provider *provider = providerFindForElement(elementKey);
    if (!provider || !provider->getCurrentPath) return NULL;
    return provider->getCurrentPath(provider);
}

// Get editable content from element
char* providerGetEditableContent(const char *elementKey) {
    Provider *provider = providerFindForElement(elementKey);
    if (!provider || !provider->getEditableContent) return NULL;
    return provider->getEditableContent(provider, elementKey);
}

// Commit edit operation
bool providerCommitEdit(const char *elementKey, const char *oldContent, const char *newContent) {
    Provider *provider = providerFindForElement(elementKey);
    if (!provider || !provider->commitEdit) return false;
    return provider->commitEdit(provider, oldContent, newContent);
}

// Format updated key after edit
char* providerFormatUpdatedKey(const char *elementKey, const char *newContent) {
    Provider *provider = providerFindForElement(elementKey);
    if (!provider || !provider->formatUpdatedKey) return NULL;
    return provider->formatUpdatedKey(provider, newContent);
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

    // Find provider for this element
    Provider *provider = providerFindForElement(key);
    char *strippedKey = NULL;

    if (provider && provider->getEditableContent) {
        strippedKey = provider->getEditableContent(provider, key);
    }

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
        if (provider->popPath) provider->popPath(provider);
        if (children) free(children);
        return false;
    }

    for (int i = 0; i < childCount; i++) {
        ffonObjectAddElement(obj, children[i]);
    }
    free(children);

    idArrayPush(&appRenderer->currentId, 0);
    return true;
}

// Navigate left out of an object
bool providerNavigateLeft(AppRenderer *appRenderer) {
    if (appRenderer->currentId.depth <= 1) {
        return false;
    }

    // Get parent element
    IdArray parentId;
    idArrayCopy(&parentId, &appRenderer->currentId);
    idArrayPop(&parentId);

    int parentCount;
    FfonElement **parentArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                           &parentId, &parentCount);
    if (parentArr && parentCount > 0) {
        int parentIdx = parentId.ids[parentId.depth - 1];
        if (parentIdx >= 0 && parentIdx < parentCount) {
            FfonElement *parentElem = parentArr[parentIdx];
            if (parentElem->type == FFON_OBJECT) {
                const char *key = parentElem->data.object->key;
                Provider *provider = providerFindForElement(key);
                if (provider && provider->popPath) {
                    provider->popPath(provider);
                }
            }
        }
    }

    idArrayPop(&appRenderer->currentId);
    return true;
}

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

int providerGetRegisteredCount(void) {
    return g_providerCount;
}

Provider* providerGetRegisteredAt(int i) {
    if (i < 0 || i >= g_providerCount) return NULL;
    return g_providers[i];
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

// Fetch URL content via curl and parse into FFON elements
static FfonElement** fetchUrlToElements(const char *url, int *outCount) {
    *outCount = 0;

    char command[4096 + 64];
    snprintf(command, sizeof(command), "curl -sfL \"%s\"", url);

    FILE *pipe = popen(command, "r");
    if (!pipe) {
        fprintf(stderr, "fetchUrlToElements: failed to run curl\n");
        return NULL;
    }

    size_t capacity = 4096;
    size_t size = 0;
    char *buffer = malloc(capacity);
    if (!buffer) {
        pclose(pipe);
        return NULL;
    }

    size_t bytesRead;
    while ((bytesRead = fread(buffer + size, 1, capacity - size - 1, pipe)) > 0) {
        size += bytesRead;
        if (size + 1 >= capacity) {
            capacity *= 2;
            char *newBuf = realloc(buffer, capacity);
            if (!newBuf) {
                free(buffer);
                pclose(pipe);
                return NULL;
            }
            buffer = newBuf;
        }
    }
    buffer[size] = '\0';
    pclose(pipe);

    if (size == 0) {
        free(buffer);
        return NULL;
    }

    // Check if URL ends with .ffon for binary format
    size_t urlLen = strlen(url);
    if (urlLen > 5 && strcmp(url + urlLen - 5, ".ffon") == 0) {
        FfonElement **elements = ffonDeserializeBinary((uint8_t*)buffer, size, outCount);
        free(buffer);
        return elements;
    }

    // Default: parse as JSON
    json_object *root = json_tokener_parse(buffer);
    free(buffer);

    if (!root) {
        fprintf(stderr, "fetchUrlToElements: failed to parse JSON from: %s\n", url);
        return NULL;
    }

    if (!json_object_is_type(root, json_type_array)) {
        fprintf(stderr, "fetchUrlToElements: expected JSON array from: %s\n", url);
        json_object_put(root);
        return NULL;
    }

    int arrayLen = json_object_array_length(root);
    if (arrayLen == 0) {
        json_object_put(root);
        return NULL;
    }

    FfonElement **elements = malloc(sizeof(FfonElement*) * arrayLen);
    if (!elements) {
        json_object_put(root);
        return NULL;
    }

    int count = 0;
    for (int i = 0; i < arrayLen; i++) {
        json_object *item = json_object_array_get_idx(root, i);
        FfonElement *elem = parseJsonValue(item);
        if (elem) {
            elements[count++] = elem;
        }
    }

    json_object_put(root);
    *outCount = count;
    return elements;
}

// Resolve a link URL (local file or HTTP) into FFON elements
static FfonElement** resolveLinkToElements(const char *url, int *outCount) {
    *outCount = 0;
    if (!url) return NULL;

    // HTTP/HTTPS: fetch via curl
    if (strncmp(url, "http://", 7) == 0 || strncmp(url, "https://", 8) == 0) {
        return fetchUrlToElements(url, outCount);
    }

    // Local file: determine format by extension
    size_t urlLen = strlen(url);
    if (urlLen > 5 && strcmp(url + urlLen - 5, ".ffon") == 0) {
        return loadFfonFileToElements(url, outCount);
    }

    // Default: JSON file
    return loadJsonFileToElements(url, outCount);
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

    // Handle <link> tags: resolve URL and load content as children
    if (providerTagHasLink(key)) {
        if (obj->count > 0) {
            // Already loaded: just navigate in
            idArrayPush(&appRenderer->currentId, 0);
            return true;
        }

        char *url = providerTagExtractLinkContent(key);
        if (!url) return false;

        int childCount = 0;
        FfonElement **children = resolveLinkToElements(url, &childCount);
        free(url);

        if (!children || childCount == 0) {
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

    // Get active provider and extract content from tagged key
    Provider *provider = providerGetActive(appRenderer);
    char *strippedKey = providerTagExtractContent(key);

    // Validate radio group constraints before navigating in
    if (providerTagHasRadio(key) && obj->count > 0) {
        const char *radioError = NULL;
        int checkedCount = 0;
        for (int i = 0; i < obj->count; i++) {
            if (obj->elements[i]->type == FFON_OBJECT) {
                radioError = "Radio group children must be strings, not objects";
                break;
            }
            if (obj->elements[i]->type == FFON_STRING &&
                providerTagHasChecked(obj->elements[i]->data.string)) {
                checkedCount++;
            }
        }
        if (!radioError && checkedCount > 1) {
            radioError = "Radio group must have at most one checked item";
        }
        if (radioError) {
            setErrorMessage(appRenderer, radioError);
            free(strippedKey);
            return false;
        }
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
        // Empty directory: add placeholder child so user can create files
        if (children) free(children);
        ffonObjectAddElement(obj, ffonElementCreateString(INPUT_TAG_OPEN INPUT_TAG_CLOSE));
    } else {
        for (int i = 0; i < childCount; i++) {
            ffonObjectAddElement(obj, children[i]);
        }
        free(children);
    }

    // Validate radio group constraints after fetching
    if (providerTagHasRadio(key)) {
        const char *radioError = NULL;
        int checkedCount = 0;
        for (int i = 0; i < obj->count; i++) {
            if (obj->elements[i]->type == FFON_OBJECT) {
                radioError = "Radio group children must be strings, not objects";
                break;
            }
            if (obj->elements[i]->type == FFON_STRING &&
                providerTagHasChecked(obj->elements[i]->data.string)) {
                checkedCount++;
            }
        }
        if (!radioError && checkedCount > 1) {
            radioError = "Radio group must have at most one checked item";
        }
        if (radioError) {
            // Undo pushPath if it was called
            if (provider && provider->popPath) {
                provider->popPath(provider);
            }
            setErrorMessage(appRenderer, radioError);
            return false;
        }
    }

    idArrayPush(&appRenderer->currentId, 0);
    return true;
}

// Refresh the current directory listing by clearing cached children and re-fetching
void providerRefreshCurrentDirectory(AppRenderer *appRenderer) {
    if (appRenderer->currentId.depth < 2) return;

    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->fetch) return;

    // Find the parent FFON_OBJECT that contains the current directory listing
    IdArray parentId;
    idArrayCopy(&parentId, &appRenderer->currentId);
    idArrayPop(&parentId);

    int parentCount;
    FfonElement **parentArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                          &parentId, &parentCount);
    if (!parentArr) return;

    int parentIdx = parentId.ids[parentId.depth - 1];
    if (parentIdx < 0 || parentIdx >= parentCount) return;

    FfonElement *parentElem = parentArr[parentIdx];
    if (parentElem->type != FFON_OBJECT) return;

    FfonObject *obj = parentElem->data.object;

    // Destroy the old cached children and reset count (keep the elements array allocated)
    for (int i = 0; i < obj->count; i++) {
        ffonElementDestroy(obj->elements[i]);
        obj->elements[i] = NULL;
    }
    obj->count = 0;

    // Re-fetch using the provider's current path (already set correctly)
    int childCount = 0;
    FfonElement **children = provider->fetch(provider, &childCount);

    if (!children || childCount == 0) {
        if (children) free(children);
        ffonObjectAddElement(obj, ffonElementCreateString(INPUT_TAG_OPEN INPUT_TAG_CLOSE));
    } else {
        for (int i = 0; i < childCount; i++) {
            ffonObjectAddElement(obj, children[i]);
        }
        free(children);
    }

    // Clamp cursor to valid range
    int newCount = obj->count;
    int *cursorIdx = &appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (*cursorIdx >= newCount) {
        *cursorIdx = newCount > 0 ? newCount - 1 : 0;
    }
}

// Navigate left out of an object
bool providerNavigateLeft(AppRenderer *appRenderer) {
    if (appRenderer->currentId.depth <= 1) {
        return false;
    }

    // Check if the parent element (the one we're leaving) has a <link> tag
    // If so, skip popPath since we didn't pushPath when entering a link
    bool parentIsLink = false;
    if (appRenderer->currentId.depth >= 2) {
        IdArray parentId;
        idArrayCopy(&parentId, &appRenderer->currentId);
        idArrayPop(&parentId);
        int parentCount;
        FfonElement **parentArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                               &parentId, &parentCount);
        if (parentArr) {
            int parentIdx = parentId.ids[parentId.depth - 1];
            if (parentIdx >= 0 && parentIdx < parentCount &&
                parentArr[parentIdx]->type == FFON_OBJECT &&
                providerTagHasLink(parentArr[parentIdx]->data.object->key)) {
                parentIsLink = true;
            }
        }
    }

    // Pop path on the active provider (skip for link parents)
    if (!parentIsLink) {
        Provider *provider = providerGetActive(appRenderer);
        if (provider && provider->popPath) {
            provider->popPath(provider);
        }
    }

    idArrayPop(&appRenderer->currentId);
    return true;
}

// Navigate to absoluteDir by building the full FFON path from root, then find targetFilename.
// Sets appRenderer->currentId to point at targetFilename.
// Returns the index of targetFilename in the final listing, or -1 on failure.
int providerNavigateToPath(AppRenderer *appRenderer, int rootIdx,
                           const char *absoluteDir, const char *targetFilename) {
    if (rootIdx < 0 || rootIdx >= appRenderer->ffonCount) return -1;

    Provider *provider = appRenderer->providers[rootIdx];
    if (!provider || !provider->setCurrentPath || !provider->fetch) return -1;

    // Reset provider path to "/"
    provider->setCurrentPath(provider, "/");

    // Clear and reload root FFON from "/"
    FfonElement *rootElem = appRenderer->ffon[rootIdx];
    if (!rootElem || rootElem->type != FFON_OBJECT) return -1;
    FfonObject *rootObj = rootElem->data.object;
    for (int i = 0; i < rootObj->count; i++) {
        ffonElementDestroy(rootObj->elements[i]);
        rootObj->elements[i] = NULL;
    }
    rootObj->count = 0;
    int cc = 0;
    FfonElement **ch = provider->fetch(provider, &cc);
    for (int i = 0; i < cc; i++) ffonObjectAddElement(rootObj, ch[i]);
    free(ch);

    // Start at depth=2 (inside root, viewing "/" listing)
    appRenderer->currentId.depth = 2;
    appRenderer->currentId.ids[0] = rootIdx;
    appRenderer->currentId.ids[1] = 0;

    // Navigate through each path component of absoluteDir (skip leading "/")
    char pathBuf[4096];
    strncpy(pathBuf, absoluteDir, sizeof(pathBuf) - 1);
    pathBuf[sizeof(pathBuf) - 1] = '\0';
    char *saveptr = NULL;
    char *start = pathBuf + (pathBuf[0] == '/' ? 1 : 0);
    char *component = strtok_r(start, "/", &saveptr);
    while (component && *component != '\0') {
        int levelCount;
        FfonElement **levelArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                             &appRenderer->currentId, &levelCount);
        if (!levelArr) return -1;

        int idx = -1;
        for (int i = 0; i < levelCount; i++) {
            FfonElement *e = levelArr[i];
            const char *raw = (e->type == FFON_STRING) ? e->data.string : e->data.object->key;
            char *name = providerTagExtractContent(raw);
            if (name && strcmp(name, component) == 0) { free(name); idx = i; break; }
            free(name);
        }
        if (idx < 0) return -1;

        appRenderer->currentId.ids[appRenderer->currentId.depth - 1] = idx;
        if (!providerNavigateRight(appRenderer)) return -1;

        component = strtok_r(NULL, "/", &saveptr);
    }

    // Now inside absoluteDir — find targetFilename
    int finalCount;
    FfonElement **finalArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                          &appRenderer->currentId, &finalCount);
    if (!finalArr) return -1;
    for (int i = 0; i < finalCount; i++) {
        FfonElement *e = finalArr[i];
        const char *raw = (e->type == FFON_STRING) ? e->data.string : e->data.object->key;
        char *name = providerTagExtractContent(raw);
        bool match = name && strcmp(name, targetFilename) == 0;
        free(name);
        if (match) {
            appRenderer->currentId.ids[appRenderer->currentId.depth - 1] = i;
            return i;
        }
    }
    return -1;
}

// Notify the active provider that a radio item was selected.
// elementId: ID of the newly checked radio child element.
void providerNotifyRadioChanged(AppRenderer *appRenderer, IdArray *elementId) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->onRadioChange) return;
    if (!elementId || elementId->depth < 2) return;

    // Get the selected child element
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, elementId, &count);
    if (!arr) return;
    int idx = elementId->ids[elementId->depth - 1];
    if (idx < 0 || idx >= count) return;
    FfonElement *elem = arr[idx];
    if (elem->type != FFON_STRING) return;

    // Get the parent radio group element
    IdArray parentId;
    idArrayCopy(&parentId, elementId);
    idArrayPop(&parentId);
    int parentCount;
    FfonElement **parentArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                          &parentId, &parentCount);
    if (!parentArr) return;
    int parentIdx = parentId.ids[parentId.depth - 1];
    if (parentIdx < 0 || parentIdx >= parentCount) return;
    FfonElement *parentElem = parentArr[parentIdx];
    if (parentElem->type != FFON_OBJECT) return;

    char *groupKey = providerTagStripDisplay(parentElem->data.object->key);
    char *selectedValue = providerTagStripDisplay(elem->data.string);

    if (groupKey && selectedValue) {
        provider->onRadioChange(provider, groupKey, selectedValue);
    }

    free(groupKey);
    free(selectedValue);
}

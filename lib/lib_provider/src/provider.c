#include <provider_interface.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

// Internal state for generic providers
typedef struct GenericProviderState {
    char currentPath[4096];
    const ProviderOps *ops;
} GenericProviderState;

// Generic canHandle: check if elementKey starts with tagPrefix
static bool genericCanHandle(Provider *self, const char *elementKey) {
    if (!elementKey || !self->tagPrefix) return false;
    return strncmp(elementKey, self->tagPrefix, strlen(self->tagPrefix)) == 0;
}

// Generic fetch: call ops->fetch with current path
static FfonElement** genericFetch(Provider *self, int *outCount) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    if (!state->ops->fetch) {
        *outCount = 0;
        return NULL;
    }
    return state->ops->fetch(state->currentPath, outCount);
}

// Generic getEditableContent: extract content between tags
static char* genericGetEditableContent(Provider *self, const char *elementKey) {
    if (!elementKey || !self->tagPrefix) return NULL;

    size_t prefixLen = strlen(self->tagPrefix);
    if (strncmp(elementKey, self->tagPrefix, prefixLen) != 0) return NULL;

    const char *start = elementKey + prefixLen;

    // Build closing tag from prefix (e.g., "<input>" -> "</input>")
    char closeTag[64];
    snprintf(closeTag, sizeof(closeTag), "</%s", self->tagPrefix + 1);

    const char *end = strstr(start, closeTag);
    if (!end) {
        // No closing tag, return everything after prefix
        return strdup(start);
    }

    size_t len = end - start;
    char *result = malloc(len + 1);
    if (result) {
        memcpy(result, start, len);
        result[len] = '\0';
    }
    return result;
}

// Generic commitEdit: call ops->commit with current path
static bool genericCommitEdit(Provider *self, const char *oldContent, const char *newContent) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    if (!state->ops->commit) return false;
    return state->ops->commit(state->currentPath, oldContent, newContent);
}

// Generic createDirectory: call ops->createDirectory with current path
static bool genericCreateDirectory(Provider *self, const char *name) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    if (!state->ops->createDirectory) return false;
    return state->ops->createDirectory(state->currentPath, name);
}

// Generic createFile: call ops->createFile with current path
static bool genericCreateFile(Provider *self, const char *name) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    if (!state->ops->createFile) return false;
    return state->ops->createFile(state->currentPath, name);
}

// Generic formatUpdatedKey: wrap content in tags
static char* genericFormatUpdatedKey(Provider *self, const char *newContent) {
    if (!newContent || !self->tagPrefix) return NULL;

    // Build closing tag
    char closeTag[64];
    snprintf(closeTag, sizeof(closeTag), "</%s", self->tagPrefix + 1);

    size_t len = strlen(self->tagPrefix) + strlen(newContent) + strlen(closeTag) + 1;
    char *result = malloc(len);
    if (result) {
        snprintf(result, len, "%s%s%s", self->tagPrefix, newContent, closeTag);
    }
    return result;
}

// Generic init: set path to "/"
static void genericInit(Provider *self) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    strcpy(state->currentPath, "/");
}

// Generic pushPath: append segment to path
static void genericPushPath(Provider *self, const char *segment) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    int pathLen = strlen(state->currentPath);
    int segLen = strlen(segment);

    // Remove trailing slash from segment if present
    if (segLen > 0 && segment[segLen - 1] == '/') {
        segLen--;
    }

    // Ensure we have a slash before appending
    if (pathLen > 0 && state->currentPath[pathLen - 1] != '/') {
        if (pathLen + 1 < (int)sizeof(state->currentPath)) {
            state->currentPath[pathLen++] = '/';
            state->currentPath[pathLen] = '\0';
        }
    }

    // Append segment
    if (pathLen + segLen < (int)sizeof(state->currentPath)) {
        strncat(state->currentPath, segment, segLen);
    }
}

// Generic popPath: remove last path segment
static void genericPopPath(Provider *self) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    int len = strlen(state->currentPath);
    if (len <= 1) return;

    // Remove trailing slash if present
    if (state->currentPath[len - 1] == '/') {
        state->currentPath[--len] = '\0';
    }

    // Find and truncate at last slash
    char *lastSlash = strrchr(state->currentPath, '/');
    if (lastSlash && lastSlash != state->currentPath) {
        *lastSlash = '\0';
    } else if (lastSlash == state->currentPath) {
        state->currentPath[1] = '\0';
    }
}

// Generic getCurrentPath
static const char* genericGetCurrentPath(Provider *self) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    return state->currentPath;
}

Provider* providerCreate(const ProviderOps *ops) {
    if (!ops || !ops->name || !ops->tagPrefix) return NULL;

    Provider *provider = calloc(1, sizeof(Provider));
    if (!provider) return NULL;

    GenericProviderState *state = calloc(1, sizeof(GenericProviderState));
    if (!state) {
        free(provider);
        return NULL;
    }

    state->ops = ops;
    strcpy(state->currentPath, "/");

    provider->name = ops->name;
    provider->tagPrefix = ops->tagPrefix;
    provider->state = state;

    // Wire up generic implementations
    provider->canHandle = genericCanHandle;
    provider->fetch = genericFetch;
    provider->getEditableContent = genericGetEditableContent;
    provider->commitEdit = ops->commit ? genericCommitEdit : NULL;
    provider->formatUpdatedKey = genericFormatUpdatedKey;
    provider->init = genericInit;
    provider->cleanup = NULL;
    provider->pushPath = genericPushPath;
    provider->popPath = genericPopPath;
    provider->getCurrentPath = genericGetCurrentPath;
    provider->createDirectory = ops->createDirectory ? genericCreateDirectory : NULL;
    provider->createFile = ops->createFile ? genericCreateFile : NULL;
    provider->loadConfig = NULL;
    provider->saveConfig = NULL;

    return provider;
}

void providerDestroy(Provider *provider) {
    if (!provider) return;
    free(provider->state);
    free(provider);
}

FfonElement* providerGetInitialElement(Provider *provider) {
    if (!provider || !provider->fetch) return NULL;

    GenericProviderState *state = (GenericProviderState*)provider->state;
    if (!state || !state->ops) return NULL;

    // Use displayName if set, otherwise fall back to name
    const char *displayName = state->ops->displayName ? state->ops->displayName : state->ops->name;

    // Fetch initial children
    int count = 0;
    FfonElement **children = provider->fetch(provider, &count);
    if (!children || count == 0) {
        if (children) free(children);
        return NULL;
    }

    // Create root element with displayName
    FfonElement *root = ffonElementCreateObject(displayName);
    if (!root) {
        for (int i = 0; i < count; i++) {
            ffonElementDestroy(children[i]);
        }
        free(children);
        return NULL;
    }

    // Add children to root
    FfonObject *obj = root->data.object;
    for (int i = 0; i < count; i++) {
        ffonObjectAddElement(obj, children[i]);
    }
    free(children);

    return root;
}

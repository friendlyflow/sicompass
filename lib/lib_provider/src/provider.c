#include <provider_interface.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <json-c/json.h>

// Internal state for generic providers
typedef struct GenericProviderState {
    char currentPath[4096];
    const ProviderOps *ops;
} GenericProviderState;

// Generic fetch: call ops->fetch with current path
static FfonElement** genericFetch(Provider *self, int *outCount) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    if (!state->ops->fetch) {
        *outCount = 0;
        return NULL;
    }
    return state->ops->fetch(state->currentPath, outCount);
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

// Generic getCommands: call ops->getCommands
static const char** genericGetCommands(Provider *self, int *outCount) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    if (!state->ops->getCommands) { *outCount = 0; return NULL; }
    return state->ops->getCommands(outCount);
}

// Generic handleCommand: call ops->handleCommand with current path
static FfonElement* genericHandleCommand(Provider *self, const char *command,
                                          const char *elementKey, int elementType,
                                          char *errorMsg, int errorMsgSize) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    if (!state->ops->handleCommand) return NULL;
    return state->ops->handleCommand(state->currentPath, command, elementKey, elementType,
                                      errorMsg, errorMsgSize);
}

// Generic getCommandListItems: call ops->getCommandListItems with current path
static ProviderListItem* genericGetCommandListItems(Provider *self, const char *command, int *outCount) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    if (!state->ops->getCommandListItems) { *outCount = 0; return NULL; }
    return state->ops->getCommandListItems(state->currentPath, command, outCount);
}

// Generic executeCommand: call ops->executeCommand with current path
static bool genericExecuteCommand(Provider *self, const char *command, const char *selection) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    if (!state->ops->executeCommand) return false;
    return state->ops->executeCommand(state->currentPath, command, selection);
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

// Generic setCurrentPath: overwrite currentPath directly (for teleport navigation)
static void genericSetCurrentPath(Provider *self, const char *absolutePath) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    strncpy(state->currentPath, absolutePath, sizeof(state->currentPath) - 1);
    state->currentPath[sizeof(state->currentPath) - 1] = '\0';
}

// Generic collectDeepSearchItems: delegate to ops->collectDeepSearchItems with currentPath
static SearchResultItem* genericCollectDeepSearchItems(Provider *self, int *outCount) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    if (!state->ops->collectDeepSearchItems) { *outCount = 0; return NULL; }
    return state->ops->collectDeepSearchItems(state->currentPath, outCount);
}

Provider* providerCreate(const ProviderOps *ops) {
    if (!ops || !ops->name) return NULL;

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
    provider->state = state;

    // Wire up generic implementations
    provider->fetch = genericFetch;
    provider->commitEdit = ops->commit ? genericCommitEdit : NULL;
    provider->init = genericInit;
    provider->cleanup = NULL;
    provider->pushPath = genericPushPath;
    provider->popPath = genericPopPath;
    provider->getCurrentPath = genericGetCurrentPath;
    provider->createDirectory = ops->createDirectory ? genericCreateDirectory : NULL;
    provider->createFile = ops->createFile ? genericCreateFile : NULL;
    provider->getCommands = ops->getCommands ? genericGetCommands : NULL;
    provider->handleCommand = ops->handleCommand ? genericHandleCommand : NULL;
    provider->getCommandListItems = ops->getCommandListItems ? genericGetCommandListItems : NULL;
    provider->executeCommand = ops->executeCommand ? genericExecuteCommand : NULL;
    provider->setCurrentPath = genericSetCurrentPath;
    provider->collectDeepSearchItems = ops->collectDeepSearchItems ? genericCollectDeepSearchItems : NULL;
    provider->loadConfig = NULL;
    provider->saveConfig = NULL;

    return provider;
}

void providerDestroy(Provider *provider) {
    if (!provider) return;
    free(provider->state);
    free(provider);
}

void providerFreeCommandListItems(ProviderListItem *items, int count) {
    if (!items) return;
    for (int i = 0; i < count; i++) {
        free(items[i].label);
        free(items[i].data);
    }
    free(items);
}

FfonElement* providerGetInitialElement(Provider *provider) {
    if (!provider || !provider->fetch) return NULL;

    // Resolve display name: prefer ProviderOps displayName/name, fall back to provider->name
    const char *displayName = NULL;
    GenericProviderState *state = (GenericProviderState*)provider->state;
    if (state && state->ops) {
        displayName = state->ops->displayName ? state->ops->displayName : state->ops->name;
    }
    if (!displayName) {
        displayName = provider->name;
    }
    if (!displayName) return NULL;

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

// ============================================
// Script provider (runs external scripts via bun)
// ============================================

// State for script-backed providers.
// Layout-compatible with GenericProviderState (currentPath and ops at same offsets)
// so generic path management functions work unchanged.
typedef struct ScriptProviderState {
    char currentPath[4096];
    const ProviderOps *ops;
    char scriptPath[4096];
} ScriptProviderState;

// Run script and parse JSON output into FFON elements
static FfonElement** scriptFetch(Provider *self, int *outCount) {
    *outCount = 0;
    ScriptProviderState *state = (ScriptProviderState*)self->state;

    // Build command: bun run <scriptPath> <currentPath>
    char command[8192 + 64];
    snprintf(command, sizeof(command), "bun run \"%s\" \"%s\"",
             state->scriptPath, state->currentPath);

    FILE *pipe = popen(command, "r");
    if (!pipe) {
        fprintf(stderr, "scriptProvider: failed to run: %s\n", command);
        return NULL;
    }

    // Read all output
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

    // Parse JSON
    json_object *root = json_tokener_parse(buffer);
    free(buffer);

    if (!root) {
        fprintf(stderr, "scriptProvider: failed to parse JSON from: %s\n", state->scriptPath);
        return NULL;
    }

    if (!json_object_is_type(root, json_type_array)) {
        fprintf(stderr, "scriptProvider: expected JSON array from: %s\n", state->scriptPath);
        json_object_put(root);
        return NULL;
    }

    // Convert JSON array to FFON elements
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

Provider* scriptProviderCreate(const char *name, const char *displayName,
                               const char *scriptPath) {
    if (!name || !scriptPath) return NULL;

    Provider *provider = calloc(1, sizeof(Provider));
    if (!provider) return NULL;

    ScriptProviderState *state = calloc(1, sizeof(ScriptProviderState));
    if (!state) {
        free(provider);
        return NULL;
    }

    // Create a static-lifetime ProviderOps for providerGetInitialElement compatibility
    ProviderOps *ops = calloc(1, sizeof(ProviderOps));
    if (!ops) {
        free(state);
        free(provider);
        return NULL;
    }
    ops->name = name;
    ops->displayName = displayName;
    ops->fetch = NULL;
    ops->commit = NULL;
    ops->createDirectory = NULL;
    ops->createFile = NULL;

    strcpy(state->currentPath, "/");
    strncpy(state->scriptPath, scriptPath, sizeof(state->scriptPath) - 1);
    state->scriptPath[sizeof(state->scriptPath) - 1] = '\0';
    state->ops = ops;

    provider->name = name;
    provider->state = state;

    // Wire up: custom fetch, reuse generic path management
    provider->fetch = scriptFetch;
    provider->commitEdit = NULL;
    provider->init = genericInit;
    provider->cleanup = NULL;
    provider->pushPath = genericPushPath;
    provider->popPath = genericPopPath;
    provider->getCurrentPath = genericGetCurrentPath;
    provider->createDirectory = NULL;
    provider->createFile = NULL;
    provider->loadConfig = NULL;
    provider->saveConfig = NULL;

    return provider;
}

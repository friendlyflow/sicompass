#include <win_compat.h>
#include <provider_interface.h>
#include <provider_tags.h>
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

// Generic deleteItem: call ops->deleteItem with current path
static bool genericDeleteItem(Provider *self, const char *name) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    if (!state->ops->deleteItem) return false;
    return state->ops->deleteItem(state->currentPath, name);
}

// Generic copyItem: delegate explicit src/dest paths to ops->copyItem
static bool genericCopyItem(Provider *self, const char *srcDir, const char *srcName,
                             const char *destDir, const char *destName) {
    GenericProviderState *state = (GenericProviderState*)self->state;
    if (!state->ops->copyItem) return false;
    return state->ops->copyItem(srcDir, srcName, destDir, destName);
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
    provider->deleteItem = ops->deleteItem ? genericDeleteItem : NULL;
    provider->copyItem = ops->copyItem ? genericCopyItem : NULL;
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

// Factory registry

typedef struct {
    const char *name;
    ProviderCreateFn fn;
} FactoryEntry;

static FactoryEntry g_factories[16];
static int g_factory_count = 0;

void providerFactoryRegister(const char *name, ProviderCreateFn fn) {
    if (g_factory_count < 16) {
        g_factories[g_factory_count].name = name;
        g_factories[g_factory_count].fn   = fn;
        g_factory_count++;
    }
}

Provider* providerFactoryCreate(const char *name) {
    for (int i = 0; i < g_factory_count; i++) {
        if (strcmp(g_factories[i].name, name) == 0)
            return g_factories[i].fn();
    }
    return NULL;
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
    char dashboardImagePath[4096];
    FfonElement *metaElement;  // parsed from "meta" key in script output; NULL if absent
} ScriptProviderState;

// On Windows, bun is installed to %USERPROFILE%\.bun\bin\bun.exe but the
// current process may not have it in PATH (PATH registry changes only take
// effect in new login sessions). Inject the bun bin directory into the
// process PATH so popen("bun run ...") finds it in child processes.
static const char* getBunExecutable(void) {
#ifdef _WIN32
    static int s_done = 0;
    if (!s_done) {
        s_done = 1;
        const char *userProfile = getenv("USERPROFILE");
        if (userProfile) {
            char bunExe[2048];
            snprintf(bunExe, sizeof(bunExe), "%s\\.bun\\bin\\bun.exe", userProfile);
            FILE *f = fopen(bunExe, "rb");
            if (f) {
                fclose(f);
                char bunDir[2048];
                snprintf(bunDir, sizeof(bunDir), "%s\\.bun\\bin", userProfile);
                const char *existing = getenv("PATH");
                char newPath[8192];
                snprintf(newPath, sizeof(newPath), "%s;%s",
                         bunDir, existing ? existing : "");
                _putenv_s("PATH", newPath);
            }
        }
    }
    return "bun";
#else
    return "bun";
#endif
}

// Shell-escape a string by wrapping in single quotes.
// Caller must free the returned string.
static char* shellEscape(const char *str) {
    if (!str) return strdup("''");
    // Count single quotes in input
    int quotes = 0;
    for (const char *p = str; *p; p++) {
        if (*p == '\'') quotes++;
    }
    // Escaped form: 'str' with each ' replaced by '\''
    size_t len = strlen(str) + 2 + quotes * 3 + 1;
    char *out = malloc(len);
    if (!out) return NULL;
    char *w = out;
    *w++ = '\'';
    for (const char *p = str; *p; p++) {
        if (*p == '\'') {
            *w++ = '\''; *w++ = '\\'; *w++ = '\''; *w++ = '\'';
        } else {
            *w++ = *p;
        }
    }
    *w++ = '\'';
    *w = '\0';
    return out;
}

// Run a script subcommand and return parsed JSON, or NULL on failure.
// Caller must json_object_put() the result.
static json_object* scriptRunSubcommand(ScriptProviderState *state, const char *subcmd,
                                         const char **args, int argCount) {
    // Build command with shell-escaped arguments
    char command[16384];
    char *escaped = shellEscape(state->scriptPath);
    int offset = snprintf(command, sizeof(command), "%s run %s %s", getBunExecutable(), escaped, subcmd);
    free(escaped);

    for (int i = 0; i < argCount && offset < (int)sizeof(command) - 1; i++) {
        escaped = shellEscape(args[i]);
        offset += snprintf(command + offset, sizeof(command) - offset, " %s", escaped);
        free(escaped);
    }

    FILE *pipe = popen(command, "r");
    if (!pipe) return NULL;

    size_t capacity = 4096, size = 0;
    char *buffer = malloc(capacity);
    if (!buffer) { pclose(pipe); return NULL; }

    size_t bytesRead;
    while ((bytesRead = fread(buffer + size, 1, capacity - size - 1, pipe)) > 0) {
        size += bytesRead;
        if (size + 1 >= capacity) {
            capacity *= 2;
            char *newBuf = realloc(buffer, capacity);
            if (!newBuf) { free(buffer); pclose(pipe); return NULL; }
            buffer = newBuf;
        }
    }
    buffer[size] = '\0';
    int status = pclose(pipe);
    if (status != 0 || size == 0) { free(buffer); return NULL; }

    json_object *result = json_tokener_parse(buffer);
    free(buffer);
    return result;
}

// Check if a JSON response indicates success (has "ok":true and no "error")
static bool scriptResponseOk(json_object *resp) {
    if (!resp) return false;
    json_object *errObj = NULL;
    if (json_object_object_get_ex(resp, "error", &errObj)) return false;
    json_object *okObj = NULL;
    if (json_object_object_get_ex(resp, "ok", &okObj)) {
        return json_object_get_boolean(okObj);
    }
    return false;
}

// Run script and parse JSON output into FFON elements
static FfonElement** scriptFetch(Provider *self, int *outCount) {
    *outCount = 0;
    ScriptProviderState *state = (ScriptProviderState*)self->state;

    // Build command: bun run <scriptPath> <currentPath>
    char command[8192 + 64];
    snprintf(command, sizeof(command), "%s run \"%s\" \"%s\"",
             getBunExecutable(), state->scriptPath, state->currentPath);

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

    // Accept JSON array (backward compat) or object with "children" array + optional metadata
    json_object *childrenArr = NULL;
    if (json_object_is_type(root, json_type_array)) {
        childrenArr = root;
    } else if (json_object_is_type(root, json_type_object)) {
        json_object *cObj = NULL;
        if (json_object_object_get_ex(root, "children", &cObj) &&
            json_object_is_type(cObj, json_type_array)) {
            childrenArr = cObj;
        }
        // Extract optional dashboardImage metadata
        json_object *imgObj = NULL;
        if (json_object_object_get_ex(root, "dashboardImage", &imgObj) &&
            json_object_is_type(imgObj, json_type_string)) {
            const char *imgPath = json_object_get_string(imgObj);
            strncpy(state->dashboardImagePath, imgPath, sizeof(state->dashboardImagePath) - 1);
            state->dashboardImagePath[sizeof(state->dashboardImagePath) - 1] = '\0';
            self->dashboardImagePath = state->dashboardImagePath;
        }
        // Extract optional meta object (single-key object whose value is an array of strings)
        json_object *metaObj = NULL;
        if (json_object_object_get_ex(root, "meta", &metaObj)) {
            state->metaElement = parseJsonValue(metaObj);
        }
    }

    if (!childrenArr) {
        fprintf(stderr, "scriptProvider: expected JSON array or object with 'children' from: %s\n", state->scriptPath);
        json_object_put(root);
        return NULL;
    }

    // Convert JSON array to FFON elements
    int arrayLen = json_object_array_length(childrenArr);
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
        json_object *item = json_object_array_get_idx(childrenArr, i);
        FfonElement *elem = parseJsonValue(item);
        if (elem) {
            elements[count++] = elem;
        }
    }

    json_object_put(root);

    // Prepend meta element if provided by the script
    if (state->metaElement && count > 0) {
        FfonElement **withMeta = malloc(sizeof(FfonElement*) * (count + 1));
        if (withMeta) {
            withMeta[0] = state->metaElement;
            for (int i = 0; i < count; i++) withMeta[i + 1] = elements[i];
            free(elements);
            state->metaElement = NULL;
            *outCount = count + 1;
            return withMeta;
        }
    }
    if (state->metaElement) {
        ffonElementDestroy(state->metaElement);
        state->metaElement = NULL;
    }

    *outCount = count;
    return elements;
}

// Script commit: bun run <script> commit <path> <old> <new>
static bool scriptCommitEdit(Provider *self, const char *oldContent, const char *newContent) {
    ScriptProviderState *state = (ScriptProviderState*)self->state;
    const char *args[] = { state->currentPath, oldContent, newContent };
    json_object *resp = scriptRunSubcommand(state, "commit", args, 3);
    bool ok = scriptResponseOk(resp);
    if (resp) json_object_put(resp);
    return ok;
}

// Script createDirectory: bun run <script> createDirectory <path> <name>
static bool scriptCreateDirectory(Provider *self, const char *name) {
    ScriptProviderState *state = (ScriptProviderState*)self->state;
    const char *args[] = { state->currentPath, name };
    json_object *resp = scriptRunSubcommand(state, "createDirectory", args, 2);
    bool ok = scriptResponseOk(resp);
    if (resp) json_object_put(resp);
    return ok;
}

// Script createFile: bun run <script> createFile <path> <name>
static bool scriptCreateFile(Provider *self, const char *name) {
    ScriptProviderState *state = (ScriptProviderState*)self->state;
    const char *args[] = { state->currentPath, name };
    json_object *resp = scriptRunSubcommand(state, "createFile", args, 2);
    bool ok = scriptResponseOk(resp);
    if (resp) json_object_put(resp);
    return ok;
}

// Script deleteItem: bun run <script> deleteItem <path> <name>
static bool scriptDeleteItem(Provider *self, const char *name) {
    ScriptProviderState *state = (ScriptProviderState*)self->state;
    const char *args[] = { state->currentPath, name };
    json_object *resp = scriptRunSubcommand(state, "deleteItem", args, 2);
    bool ok = scriptResponseOk(resp);
    if (resp) json_object_put(resp);
    return ok;
}

// Script copyItem: bun run <script> copyItem <srcDir> <srcName> <destDir> <destName>
static bool scriptCopyItem(Provider *self, const char *srcDir, const char *srcName,
                            const char *destDir, const char *destName) {
    ScriptProviderState *state = (ScriptProviderState*)self->state;
    const char *args[] = { srcDir, srcName, destDir, destName };
    json_object *resp = scriptRunSubcommand(state, "copyItem", args, 4);
    bool ok = scriptResponseOk(resp);
    if (resp) json_object_put(resp);
    return ok;
}

// Script getCommands: bun run <script> commands
static const char** scriptGetCommands(Provider *self, int *outCount) {
    ScriptProviderState *state = (ScriptProviderState*)self->state;
    json_object *resp = scriptRunSubcommand(state, "commands", NULL, 0);
    *outCount = 0;
    if (!resp || !json_object_is_type(resp, json_type_array)) {
        if (resp) json_object_put(resp);
        return NULL;
    }
    int len = json_object_array_length(resp);
    if (len == 0) { json_object_put(resp); return NULL; }

    const char **cmds = malloc(len * sizeof(const char*));
    for (int i = 0; i < len; i++) {
        cmds[i] = strdup(json_object_get_string(json_object_array_get_idx(resp, i)));
    }
    *outCount = len;
    json_object_put(resp);
    return cmds;
}

// Script handleCommand: bun run <script> handleCommand <path> <command> <key> <type>
static FfonElement* scriptHandleCommand(Provider *self, const char *command,
                                         const char *elementKey, int elementType,
                                         char *errorMsg, int errorMsgSize) {
    ScriptProviderState *state = (ScriptProviderState*)self->state;
    char typeStr[16];
    snprintf(typeStr, sizeof(typeStr), "%d", elementType);
    const char *args[] = { state->currentPath, command, elementKey ? elementKey : "", typeStr };
    json_object *resp = scriptRunSubcommand(state, "handleCommand", args, 4);
    if (!resp) return NULL;

    json_object *errObj = NULL;
    if (json_object_object_get_ex(resp, "error", &errObj)) {
        if (errorMsg && errorMsgSize > 0) {
            strncpy(errorMsg, json_object_get_string(errObj), errorMsgSize - 1);
            errorMsg[errorMsgSize - 1] = '\0';
        }
        json_object_put(resp);
        return NULL;
    }

    FfonElement *elem = parseJsonValue(resp);
    json_object_put(resp);
    return elem;
}

// Script getCommandListItems: bun run <script> commandListItems <path> <command>
static ProviderListItem* scriptGetCommandListItems(Provider *self, const char *command, int *outCount) {
    ScriptProviderState *state = (ScriptProviderState*)self->state;
    const char *args[] = { state->currentPath, command };
    json_object *resp = scriptRunSubcommand(state, "commandListItems", args, 2);
    *outCount = 0;
    if (!resp || !json_object_is_type(resp, json_type_array)) {
        if (resp) json_object_put(resp);
        return NULL;
    }
    int len = json_object_array_length(resp);
    if (len == 0) { json_object_put(resp); return NULL; }

    ProviderListItem *items = malloc(len * sizeof(ProviderListItem));
    for (int i = 0; i < len; i++) {
        json_object *item = json_object_array_get_idx(resp, i);
        json_object *labelObj = NULL, *dataObj = NULL;
        json_object_object_get_ex(item, "label", &labelObj);
        json_object_object_get_ex(item, "data", &dataObj);
        items[i].label = labelObj ? strdup(json_object_get_string(labelObj)) : strdup("");
        items[i].data = dataObj ? strdup(json_object_get_string(dataObj)) : strdup("");
    }
    *outCount = len;
    json_object_put(resp);
    return items;
}

// Script executeCommand: bun run <script> executeCommand <path> <command> <selection>
static bool scriptExecuteCommand(Provider *self, const char *command, const char *selection) {
    ScriptProviderState *state = (ScriptProviderState*)self->state;
    const char *args[] = { state->currentPath, command, selection ? selection : "" };
    json_object *resp = scriptRunSubcommand(state, "executeCommand", args, 3);
    bool ok = scriptResponseOk(resp);
    if (resp) json_object_put(resp);
    return ok;
}

// Script collectDeepSearchItems: bun run <script> deepSearch <path>
static SearchResultItem* scriptCollectDeepSearchItems(Provider *self, int *outCount) {
    ScriptProviderState *state = (ScriptProviderState*)self->state;
    const char *args[] = { state->currentPath };
    json_object *resp = scriptRunSubcommand(state, "deepSearch", args, 1);
    *outCount = 0;
    if (!resp || !json_object_is_type(resp, json_type_array)) {
        if (resp) json_object_put(resp);
        return NULL;
    }
    int len = json_object_array_length(resp);
    if (len == 0) { json_object_put(resp); return NULL; }

    SearchResultItem *items = malloc(len * sizeof(SearchResultItem));
    for (int i = 0; i < len; i++) {
        json_object *item = json_object_array_get_idx(resp, i);
        json_object *labelObj = NULL, *bcObj = NULL, *navObj = NULL;
        json_object_object_get_ex(item, "label", &labelObj);
        json_object_object_get_ex(item, "breadcrumb", &bcObj);
        json_object_object_get_ex(item, "navPath", &navObj);
        items[i].label = labelObj ? strdup(json_object_get_string(labelObj)) : strdup("");
        items[i].breadcrumb = bcObj ? strdup(json_object_get_string(bcObj)) : strdup("");
        items[i].navPath = navObj ? strdup(json_object_get_string(navObj)) : strdup("");
    }
    *outCount = len;
    json_object_put(resp);
    return items;
}

// Fetch children from the script provider for a given path.
// Returns the number of children added to the object, or 0 on failure.
static int scriptFetchChildren(ScriptProviderState *state, const char *path, FfonElement *objElem) {
    char command[16384];
    char *escaped = shellEscape(state->scriptPath);
    char *escapedPath = shellEscape(path);
    snprintf(command, sizeof(command), "bun run %s %s", escaped, escapedPath);
    free(escaped);
    free(escapedPath);

    FILE *pipe = popen(command, "r");
    if (!pipe) return 0;

    size_t capacity = 4096, size = 0;
    char *buffer = malloc(capacity);
    if (!buffer) { pclose(pipe); return 0; }

    size_t bytesRead;
    while ((bytesRead = fread(buffer + size, 1, capacity - size - 1, pipe)) > 0) {
        size += bytesRead;
        if (size + 1 >= capacity) {
            capacity *= 2;
            char *newBuf = realloc(buffer, capacity);
            if (!newBuf) { free(buffer); pclose(pipe); return 0; }
            buffer = newBuf;
        }
    }
    buffer[size] = '\0';
    int status = pclose(pipe);
    if (status != 0 || size == 0) { free(buffer); return 0; }

    json_object *root = json_tokener_parse(buffer);
    free(buffer);
    if (!root) return 0;

    json_object *childrenArr = NULL;
    if (json_object_is_type(root, json_type_array)) {
        childrenArr = root;
    } else if (json_object_is_type(root, json_type_object)) {
        json_object *cObj = NULL;
        if (json_object_object_get_ex(root, "children", &cObj) &&
            json_object_is_type(cObj, json_type_array)) {
            childrenArr = cObj;
        }
    }

    int added = 0;
    if (childrenArr) {
        int arrayLen = json_object_array_length(childrenArr);
        for (int i = 0; i < arrayLen; i++) {
            json_object *item = json_object_array_get_idx(childrenArr, i);
            FfonElement *child = parseJsonValue(item);
            if (child) {
                ffonObjectAddElement(objElem->data.object, child);
                added++;
            }
        }
    }

    json_object_put(root);
    return added;
}

static FfonElement* scriptCreateElement(Provider *self, const char *elementKey) {
    ScriptProviderState *state = (ScriptProviderState*)self->state;
    const char *key = elementKey;
    bool isOneOpt = strncmp(key, "one-opt:", 8) == 0;
    if (isOneOpt) {
        key = key + 8;
    }
    if (isOneOpt) {
        // Prefix with <one-opt></one-opt> tag so deletion can restore the button
        size_t taggedLen = ONE_OPT_TAG_LEN + strlen(key) + 1;
        char *tagged = malloc(taggedLen);
        if (!tagged) return NULL;
        snprintf(tagged, taggedLen, ONE_OPT_TAG "%s", key);
        FfonElement *elem;
        if (providerTagHasInput(key) || providerTagHasInputAll(key)) {
            elem = ffonElementCreateString(tagged);
        } else {
            elem = ffonElementCreateObject(tagged);
            // Fetch children from the script provider
            char childPath[4096];
            snprintf(childPath, sizeof(childPath), "%s%s%s",
                     state->currentPath,
                     (state->currentPath[strlen(state->currentPath) - 1] == '/') ? "" : "/",
                     key);
            scriptFetchChildren(state, childPath, elem);
        }
        free(tagged);
        return elem;
    }
    // Many-opt: prefix with <many-opt></many-opt> tag so deletion knows it's deletable
    size_t taggedLen = MANY_OPT_TAG_LEN + strlen(key) + 1;
    char *tagged = malloc(taggedLen);
    if (!tagged) return NULL;
    snprintf(tagged, taggedLen, MANY_OPT_TAG "%s", key);
    FfonElement *elem;
    if (providerTagHasInput(key) || providerTagHasInputAll(key)) {
        elem = ffonElementCreateString(tagged);
    } else {
        elem = ffonElementCreateObject(tagged);
        // Fetch children from the script provider
        char childPath[4096];
        snprintf(childPath, sizeof(childPath), "%s%s%s",
                 state->currentPath,
                 (state->currentPath[strlen(state->currentPath) - 1] == '/') ? "" : "/",
                 key);
        scriptFetchChildren(state, childPath, elem);
    }
    free(tagged);
    return elem;
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
    ops->name = strdup(name);
    ops->displayName = displayName ? strdup(displayName) : NULL;
    if (!ops->name || (displayName && !ops->displayName)) {
        free((char*)ops->name);
        free((char*)ops->displayName);
        free(ops);
        free(state);
        free(provider);
        return NULL;
    }
    ops->fetch = NULL;
    ops->commit = NULL;
    ops->createDirectory = NULL;
    ops->createFile = NULL;

    strcpy(state->currentPath, "/");
    strncpy(state->scriptPath, scriptPath, sizeof(state->scriptPath) - 1);
    state->scriptPath[sizeof(state->scriptPath) - 1] = '\0';
    state->ops = ops;

    provider->name = ops->name;
    provider->state = state;

    // Wire up: custom fetch, reuse generic path management
    provider->fetch = scriptFetch;
    provider->commitEdit = scriptCommitEdit;
    provider->init = genericInit;
    provider->cleanup = NULL;
    provider->pushPath = genericPushPath;
    provider->popPath = genericPopPath;
    provider->getCurrentPath = genericGetCurrentPath;
    provider->setCurrentPath = genericSetCurrentPath;
    provider->createDirectory = scriptCreateDirectory;
    provider->createFile = scriptCreateFile;
    provider->deleteItem = scriptDeleteItem;
    provider->copyItem = scriptCopyItem;
    provider->getCommands = scriptGetCommands;
    provider->handleCommand = scriptHandleCommand;
    provider->getCommandListItems = scriptGetCommandListItems;
    provider->executeCommand = scriptExecuteCommand;
    provider->collectDeepSearchItems = scriptCollectDeepSearchItems;
    provider->loadConfig = NULL;
    provider->saveConfig = NULL;
    provider->createElement = scriptCreateElement;

    return provider;
}

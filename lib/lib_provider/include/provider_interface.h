#pragma once

#include <stdbool.h>
#include <ffon.h>

/**
 * Search result item returned by collectDeepSearchItems.
 * All string fields are heap-allocated; caller frees the array but transfers
 * string ownership to ListItem (no extra strdup needed).
 */
typedef struct {
    char *label;      // display label with prefix (e.g., "- report.pdf", "+ docs")
    char *breadcrumb; // relative path context (e.g., "docs > projects > ")
    char *navPath;    // full absolute path to item (for path-based navigation)
} SearchResultItem;

/**
 * List item returned by provider commands (e.g., applications for "open with").
 */
typedef struct {
    char *label;
    char *data;
} ProviderListItem;

/**
 * Free an array of ProviderListItem returned by getCommandListItems.
 */
void providerFreeCommandListItems(ProviderListItem *items, int count);

/**
 * Provider interface - defines what a plugin must implement.
 *
 * Providers handle data operations only:
 * - Fetch children for FFON objects (e.g., directory listings)
 * - Extract editable content from elements
 * - Commit edits (e.g., rename files)
 * - Maintain their own state (e.g., current path)
 *
 * The application layer handles UI concerns:
 * - Mode switching, input buffer, cursor, UI refresh
 */
typedef struct Provider {
    // Identity
    const char *name;           // e.g., "filebrowser", "git", "docker"

    // Required: Fetch children for objects this provider handles
    FfonElement** (*fetch)(struct Provider *self, int *outCount);

    // Optional: Commit an edit (e.g., rename file)
    // Returns: true on success, false on failure
    bool (*commitEdit)(struct Provider *self, const char *oldContent, const char *newContent);

    // Optional: Lifecycle
    void (*init)(struct Provider *self);
    void (*cleanup)(struct Provider *self);

    // Optional: Navigation - append/pop path segment
    void (*pushPath)(struct Provider *self, const char *segment);
    void (*popPath)(struct Provider *self);
    const char* (*getCurrentPath)(struct Provider *self);

    // Optional: Create operations
    bool (*createDirectory)(struct Provider *self, const char *name);
    bool (*createFile)(struct Provider *self, const char *name);

    // Optional: Commands this provider supports
    const char** (*getCommands)(struct Provider *self, int *outCount);
    FfonElement* (*handleCommand)(struct Provider *self, const char *command,
                                   const char *elementKey, int elementType,
                                   char *errorMsg, int errorMsgSize);
    ProviderListItem* (*getCommandListItems)(struct Provider *self, const char *command, int *outCount);
    bool (*executeCommand)(struct Provider *self, const char *command, const char *selection);

    // Optional: Called after a radio child is selected within this provider's FFON tree.
    // groupKey: stripped key of the <radio> parent (e.g. "color scheme")
    // selectedValue: stripped text of the newly checked child (e.g. "light")
    void (*onRadioChange)(struct Provider *self, const char *groupKey, const char *selectedValue);

    // Optional: Set current path directly (for teleport navigation after deep search).
    void (*setCurrentPath)(struct Provider *self, const char *absolutePath);

    // Optional: Collect all items under current path for deep extended search.
    // Returns allocated array of SearchResultItem; caller frees the array (strings
    // are transferred to ListItem, not double-freed).
    // If NULL, createListExtendedSearch falls back to FFON-tree traversal.
    SearchResultItem* (*collectDeepSearchItems)(struct Provider *self, int *outCount);

    // Optional: Persistent config
    bool (*loadConfig)(struct Provider *self, const char *configPath);
    bool (*saveConfig)(struct Provider *self, const char *configPath);

    // Provider-private state (opaque pointer)
    void *state;
} Provider;

/**
 * ProviderOps - simplified interface for plugin authors.
 *
 * Plugin authors only need to provide:
 * 1. name (identity)
 * 2. fetch function (data source)
 * 3. commit function (optional, for editable elements)
 *
 * Everything else (path management) is handled generically.
 */
typedef struct ProviderOps {
    const char *name;        // e.g., "filebrowser"
    const char *displayName; // e.g., "file browser" (shown in UI)

    // Required: Fetch children at current path
    // path: current path from provider state (e.g., "/home/user")
    FfonElement** (*fetch)(const char *path, int *outCount);

    // Optional: Commit an edit (e.g., rename)
    // path: current path, oldName/newName: the content being changed
    bool (*commit)(const char *path, const char *oldName, const char *newName);

    // Optional: Create operations
    bool (*createDirectory)(const char *path, const char *name);
    bool (*createFile)(const char *path, const char *name);

    // Optional: Commands this provider supports
    const char** (*getCommands)(int *outCount);
    FfonElement* (*handleCommand)(const char *path, const char *command,
                                   const char *elementKey, int elementType,
                                   char *errorMsg, int errorMsgSize);
    ProviderListItem* (*getCommandListItems)(const char *path, const char *command, int *outCount);
    bool (*executeCommand)(const char *path, const char *command, const char *selection);

    // Optional: Collect all items under rootPath for deep extended search.
    SearchResultItem* (*collectDeepSearchItems)(const char *rootPath, int *outCount);
} ProviderOps;

/**
 * Create a provider from simplified ops.
 * Handles all boilerplate: path management, etc.
 *
 * @param ops The plugin-specific operations
 * @return A fully configured Provider (caller must free with providerDestroy)
 */
Provider* providerCreate(const ProviderOps *ops);

/**
 * Destroy a provider created with providerCreate.
 */
void providerDestroy(Provider *provider);

/**
 * Get the initial element for a provider.
 * Creates a root object with displayName as key, populated with initial children.
 * Returns: FfonElement* (caller owns), or NULL on failure
 */
FfonElement* providerGetInitialElement(Provider *provider);

/**
 * Get the config directory path for sicompass providers.
 * Returns: ~/.config/sicompass/providers/ (caller must free)
 */
char* providerGetConfigDir(void);

/**
 * Get the config file path for a specific provider.
 * Returns: ~/.config/sicompass/providers/<name>.json (caller must free)
 */
char* providerGetConfigPath(const char *providerName);

/**
 * Get the main unified config file path.
 * Returns: ~/.config/sicompass/settings.json (caller must free)
 */
char* providerGetMainConfigPath(void);

/**
 * Create a provider backed by a script (e.g., TypeScript run with Bun).
 *
 * On each fetch(), runs: bun run <scriptPath> <currentPath>
 * Parses the JSON array output into FFON elements.
 * The provider is read-only (no commit/create operations).
 *
 * @param name Provider name (e.g., "tutorial")
 * @param displayName Display name shown in UI (e.g., "tutorial")
 * @param scriptPath Absolute path to the script file
 * @return A fully configured Provider (caller must free with providerDestroy)
 */
Provider* scriptProviderCreate(const char *name, const char *displayName,
                               const char *scriptPath);

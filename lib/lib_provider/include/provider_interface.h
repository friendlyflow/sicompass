#pragma once

#include <stdbool.h>
#include <ffon.h>

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
    const char *tagPrefix;      // e.g., "<input>", "<git>", "<docker>"

    // Required: Does this provider handle this element?
    bool (*canHandle)(struct Provider *self, const char *elementKey);

    // Required: Fetch children for objects this provider handles
    FfonElement** (*fetch)(struct Provider *self, int *outCount);

    // Optional: Extract editable content from an element key
    // Returns: newly allocated string (caller must free), or NULL
    char* (*getEditableContent)(struct Provider *self, const char *elementKey);

    // Optional: Commit an edit (e.g., rename file)
    // Returns: true on success, false on failure
    bool (*commitEdit)(struct Provider *self, const char *oldContent, const char *newContent);

    // Optional: Update element after successful edit
    // Returns: newly allocated string with updated element key (caller must free)
    char* (*formatUpdatedKey)(struct Provider *self, const char *newContent);

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
 * 1. name and tagPrefix (identity)
 * 2. fetch function (data source)
 * 3. commit function (optional, for editable elements)
 *
 * Everything else (path management, tag handling) is handled generically.
 */
typedef struct ProviderOps {
    const char *name;        // e.g., "filebrowser"
    const char *displayName; // e.g., "file browser" (shown in UI)
    const char *tagPrefix;   // e.g., "<input>"

    // Required: Fetch children at current path
    // path: current path from provider state (e.g., "/home/user")
    FfonElement** (*fetch)(const char *path, int *outCount);

    // Optional: Commit an edit (e.g., rename)
    // path: current path, oldName/newName: the content being changed
    bool (*commit)(const char *path, const char *oldName, const char *newName);

    // Optional: Create operations
    bool (*createDirectory)(const char *path, const char *name);
    bool (*createFile)(const char *path, const char *name);
} ProviderOps;

/**
 * Create a provider from simplified ops.
 * Handles all boilerplate: path management, tag extraction/formatting, etc.
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

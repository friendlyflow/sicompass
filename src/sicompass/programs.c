#include "programs.h"
#include "provider.h"
#include "view.h"
#include <provider_interface.h>
#include <settings_provider.h>
#include <json-c/json.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <sys/stat.h>
#include <dirent.h>
#include <dlfcn.h>

// Built-in program catalog
static const char *ALL_KNOWN_PROGRAMS[] = {
    "tutorial", "sales demo",
    "chat client", "email client", "web browser"
};
static const int ALL_KNOWN_PROGRAMS_COUNT = 5;

// User-discovered plugins
typedef struct {
    char name[64];
    char displayName[64];
    char entryPath[4096];
    bool supportsConfigFiles;
    bool isNative;
} UserPlugin;

static UserPlugin s_userPlugins[32];
static int s_userPluginCount = 0;

// File-static references for hot enable/disable
static Provider *s_settingsProvider = NULL;

// Read a string value from settings.json for a given section and key.
static char* readSettingsValue(const char *section, const char *key) {
    char *configPath = providerGetMainConfigPath();
    if (!configPath) return NULL;
    json_object *root = json_object_from_file(configPath);
    free(configPath);
    if (!root) return NULL;

    json_object *sectionObj;
    if (!json_object_object_get_ex(root, section, &sectionObj)) {
        json_object_put(root);
        return NULL;
    }
    json_object *valObj;
    if (!json_object_object_get_ex(sectionObj, key, &valObj)) {
        json_object_put(root);
        return NULL;
    }
    const char *str = json_object_get_string(valObj);
    char *result = str ? strdup(str) : NULL;
    json_object_put(root);
    return result;
}

static const char *DEFAULT_PROGRAMS[] = {"tutorial"};
static const int DEFAULT_PROGRAMS_COUNT = 1;

static void ensureConfigDir(const char *configPath) {
    char *dir = strdup(configPath);
    if (!dir) return;
    char *lastSep = strrchr(dir, '/');
    if (lastSep) *lastSep = '\0';
    for (char *p = dir + 1; *p; p++) {
        if (*p == '/') {
            char c = *p;
            *p = '\0';
            mkdir(dir, 0755);
            *p = c;
        }
    }
    mkdir(dir, 0755);
    free(dir);
}

static void writeDefaultConfig(const char *configPath) {
    ensureConfigDir(configPath);
    json_object *root = json_object_from_file(configPath);
    if (!root) root = json_object_new_object();

    json_object *sicompassObj = NULL;
    if (!json_object_object_get_ex(root, "sicompass", &sicompassObj)) {
        sicompassObj = json_object_new_object();
        json_object_object_add(root, "sicompass", sicompassObj);
    }
    json_object *arr = json_object_new_array();
    for (int i = 0; i < DEFAULT_PROGRAMS_COUNT; i++) {
        json_object_array_add(arr, json_object_new_string(DEFAULT_PROGRAMS[i]));
    }
    json_object_object_add(sicompassObj, "programsToLoad", arr);
    json_object_to_file_ext(configPath, root, JSON_C_TO_STRING_PRETTY);
    json_object_put(root);
}

// Match display name to provider name, ignoring spaces.
// e.g., "chat client" matches "chatclient".
static bool nameMatchesProvider(const char *displayName, const char *providerName) {
    if (strcmp(displayName, providerName) == 0) return true;
    const char *d = displayName, *p = providerName;
    while (*d && *p) {
        if (*d == ' ') { d++; continue; }
        if (*d != *p) return false;
        d++; p++;
    }
    while (*d == ' ') d++;
    return *d == '\0' && *p == '\0';
}

static Provider* loadProgram(const char *name, Provider *settingsProvider) {
    if (strcmp(name, "tutorial") == 0) {
        Provider *p = scriptProviderCreate("tutorial", "tutorial --> here you can go up, down or right", TUTORIAL_SCRIPT_PATH);
        if (p) {
            providerRegister(p);
            settingsAddSection(settingsProvider, "tutorial");
        }
        return p;
    } else if (strcmp(name, "sales demo") == 0) {
        Provider *p = scriptProviderCreate("sales demo", "sales demo", SALES_DEMO_SCRIPT_PATH);
        if (p) {
            p->supportsConfigFiles = true;
            providerRegister(p);
            settingsAddSectionText(settingsProvider, "sales demo",
                                   "save folder (product configuration)",
                                   "saveFolder", "Downloads");
        }
        return p;
    } else if (strcmp(name, "chat client") == 0) {
        Provider *p = providerFactoryCreate("chat client");
        if (p) {
            providerRegister(p);
            settingsAddSectionText(settingsProvider, "chat client",
                                   "homeserver URL", "chatHomeserver",
                                   "https://matrix.org");
            settingsAddSectionText(settingsProvider, "chat client",
                                   "access token", "chatAccessToken", "");
            settingsAddSectionText(settingsProvider, "chat client",
                                   "username", "chatUsername", "");
            settingsAddSectionText(settingsProvider, "chat client",
                                   "password", "chatPassword", "");
        }
        return p;
    } else if (strcmp(name, "email client") == 0) {
        Provider *p = providerFactoryCreate("email client");
        if (p) {
            providerRegister(p);
            settingsAddSectionText(settingsProvider, "email client",
                                   "IMAP URL", "emailImapUrl",
                                   "imaps://imap.gmail.com");
            settingsAddSectionText(settingsProvider, "email client",
                                   "SMTP URL", "emailSmtpUrl",
                                   "smtps://smtp.gmail.com");
            settingsAddSectionText(settingsProvider, "email client",
                                   "username", "emailUsername", "");
            settingsAddSectionText(settingsProvider, "email client",
                                   "password", "emailPassword", "");
            settingsAddSectionText(settingsProvider, "email client",
                                   "client ID (OAuth)", "emailClientId", "");
            settingsAddSectionText(settingsProvider, "email client",
                                   "client secret (OAuth)", "emailClientSecret", "");
        }
        return p;
    } else if (strcmp(name, "web browser") == 0) {
        Provider *p = providerFactoryCreate("web browser");
        if (p) {
            providerRegister(p);
            settingsAddSection(settingsProvider, "web browser");
        }
        return p;
    } else {
        // Check if it's a user plugin
        for (int i = 0; i < s_userPluginCount; i++) {
            if (strcmp(name, s_userPlugins[i].name) == 0) {
                Provider *p = NULL;
                if (s_userPlugins[i].isNative) {
                    void *handle = dlopen(s_userPlugins[i].entryPath, RTLD_NOW);
                    if (!handle) return NULL;
                    typedef const ProviderOps* (*InitFn)(void);
                    InitFn initFn;
                    *(void **)&initFn = dlsym(handle, "sicompass_plugin_init");
                    if (!initFn) { dlclose(handle); return NULL; }
                    const ProviderOps *ops = initFn();
                    if (!ops) { dlclose(handle); return NULL; }
                    p = providerCreate(ops);
                } else {
                    p = scriptProviderCreate(s_userPlugins[i].name,
                                             s_userPlugins[i].displayName,
                                             s_userPlugins[i].entryPath);
                }
                if (p) {
                    p->supportsConfigFiles = s_userPlugins[i].supportsConfigFiles;
                    providerRegister(p);
                    settingsAddSection(settingsProvider, s_userPlugins[i].name);
                }
                return p;
            }
        }

        // Unknown program name: try loading as a remote FFON service.
        char *remoteUrl = readSettingsValue(name, "remoteUrl");
        if (remoteUrl && remoteUrl[0]) {
            #ifdef REMOTE_SCRIPT_PATH
            Provider *p = scriptProviderCreate(name, name, REMOTE_SCRIPT_PATH);
            if (p) {
                snprintf((char*)p->state, 4096, "%s", name);

                char *apiKey = readSettingsValue(name, "apiKey");
                if (apiKey && apiKey[0]) {
                    providerRegisterAuth(remoteUrl, apiKey);
                }
                free(apiKey);

                providerRegister(p);
                settingsAddSectionText(settingsProvider, name,
                                       "remote URL", "remoteUrl", "");
                settingsAddSectionText(settingsProvider, name,
                                       "API key", "apiKey", "");
            }
            free(remoteUrl);
            return p;
            #endif
        }
        free(remoteUrl);
    }
    return NULL;
}

// Discover user-installed plugins from ~/.config/sicompass/plugins/
static void discoverUserPlugins(void) {
    s_userPluginCount = 0;
    char *pluginsDir = providerGetPluginsDir();
    if (!pluginsDir) return;

    DIR *dir = opendir(pluginsDir);
    if (!dir) {
        free(pluginsDir);
        return;
    }

    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL && s_userPluginCount < 32) {
        if (entry->d_name[0] == '.') continue;

        // Build path to plugin.json
        char manifestPath[4096];
        snprintf(manifestPath, sizeof(manifestPath), "%s%s/plugin.json",
                 pluginsDir, entry->d_name);

        json_object *manifest = json_object_from_file(manifestPath);
        if (!manifest) continue;

        json_object *nameObj, *displayNameObj, *entryObj;
        if (!json_object_object_get_ex(manifest, "name", &nameObj) ||
            !json_object_object_get_ex(manifest, "displayName", &displayNameObj) ||
            !json_object_object_get_ex(manifest, "entry", &entryObj)) {
            json_object_put(manifest);
            continue;
        }

        UserPlugin *up = &s_userPlugins[s_userPluginCount];
        strncpy(up->name, json_object_get_string(nameObj), sizeof(up->name) - 1);
        up->name[sizeof(up->name) - 1] = '\0';
        strncpy(up->displayName, json_object_get_string(displayNameObj), sizeof(up->displayName) - 1);
        up->displayName[sizeof(up->displayName) - 1] = '\0';
        snprintf(up->entryPath, sizeof(up->entryPath), "%s%s/%s",
                 pluginsDir, entry->d_name, json_object_get_string(entryObj));

        json_object *configFilesObj;
        up->supportsConfigFiles = json_object_object_get_ex(manifest, "supportsConfigFiles", &configFilesObj)
                                  && json_object_get_boolean(configFilesObj);

        json_object *typeObj;
        up->isNative = json_object_object_get_ex(manifest, "type", &typeObj)
                       && strcmp(json_object_get_string(typeObj), "native") == 0;

        json_object_put(manifest);
        s_userPluginCount++;
    }

    closedir(dir);
    free(pluginsDir);
}

// Check if a name is in the programsToLoad array
static bool isInProgramsToLoad(const char *name, json_object *programsArr) {
    if (!programsArr) return false;
    int len = json_object_array_length(programsArr);
    for (int i = 0; i < len; i++) {
        const char *val = json_object_get_string(json_object_array_get_idx(programsArr, i));
        if (val && strcmp(val, name) == 0) return true;
    }
    return false;
}

// Register the "programs" priority section with checkboxes for all known programs
static void registerProgramsSection(Provider *settingsProvider, json_object *programsArr) {
    settingsAddPrioritySection(settingsProvider, "Available programs:");

    // Built-in programs
    for (int i = 0; i < ALL_KNOWN_PROGRAMS_COUNT; i++) {
        const char *name = ALL_KNOWN_PROGRAMS[i];
        char configKey[80];
        snprintf(configKey, sizeof(configKey), "enable_%s", name);
        bool enabled = isInProgramsToLoad(name, programsArr);
        settingsAddSectionCheckbox(settingsProvider, "Available programs:", name, configKey, enabled);
    }

    // User plugins
    for (int i = 0; i < s_userPluginCount; i++) {
        char configKey[80];
        snprintf(configKey, sizeof(configKey), "enable_%s", s_userPlugins[i].name);
        bool enabled = isInProgramsToLoad(s_userPlugins[i].name, programsArr);
        settingsAddSectionCheckbox(settingsProvider, "Available programs:",
                                   s_userPlugins[i].displayName, configKey, enabled);
    }
}

void programsLoad(Provider *settingsProvider) {
    s_settingsProvider = settingsProvider;

    // Discover user plugins first
    discoverUserPlugins();

    char *configPath = providerGetMainConfigPath();
    if (!configPath) return;

    json_object *root = json_object_from_file(configPath);
    if (!root) {
        writeDefaultConfig(configPath);
        root = json_object_from_file(configPath);
    }
    if (!root) { free(configPath); return; }

    json_object *sicompassObj = NULL;
    json_object *programsArr = NULL;
    if (json_object_object_get_ex(root, "sicompass", &sicompassObj)) {
        json_object_object_get_ex(sicompassObj, "programsToLoad", &programsArr);
    }

    // Register store checkboxes BEFORE loading programs
    registerProgramsSection(settingsProvider, programsArr);

    // Remove "file browser" from programsToLoad if present (now always loaded separately)
    if (programsArr && json_object_is_type(programsArr, json_type_array)) {
        int len = json_object_array_length(programsArr);
        bool found = false;
        for (int i = 0; i < len; i++) {
            const char *val = json_object_get_string(json_object_array_get_idx(programsArr, i));
            if (val && strcmp(val, "file browser") == 0) { found = true; break; }
        }
        if (found) {
            json_object *newArr = json_object_new_array();
            for (int i = 0; i < len; i++) {
                const char *val = json_object_get_string(json_object_array_get_idx(programsArr, i));
                if (val && strcmp(val, "file browser") != 0)
                    json_object_array_add(newArr, json_object_new_string(val));
            }
            json_object_object_add(sicompassObj, "programsToLoad", newArr);
            programsArr = newArr;
            json_object_to_file_ext(configPath, root, JSON_C_TO_STRING_PRETTY);
        }
    }

    // Load enabled programs
    if (programsArr && json_object_is_type(programsArr, json_type_array)) {
        int len = json_object_array_length(programsArr);
        for (int i = 0; i < len; i++) {
            json_object *item = json_object_array_get_idx(programsArr, i);
            const char *name = json_object_get_string(item);
            if (name) loadProgram(name, settingsProvider);
        }
    }

    json_object_put(root);
    free(configPath);
}

void programsUpdateEnabled(const char *name, bool enabled) {
    if (strcmp(name, "file browser") == 0) return;  // always present
    char *configPath = providerGetMainConfigPath();
    if (!configPath) return;

    json_object *root = json_object_from_file(configPath);
    if (!root) root = json_object_new_object();

    json_object *sicompassObj = NULL;
    if (!json_object_object_get_ex(root, "sicompass", &sicompassObj)) {
        sicompassObj = json_object_new_object();
        json_object_object_add(root, "sicompass", sicompassObj);
    }

    // Read existing programsToLoad
    json_object *oldArr = NULL;
    json_object_object_get_ex(sicompassObj, "programsToLoad", &oldArr);

    // Build new array
    json_object *newArr = json_object_new_array();
    if (oldArr && json_object_is_type(oldArr, json_type_array)) {
        int len = json_object_array_length(oldArr);
        for (int i = 0; i < len; i++) {
            const char *val = json_object_get_string(json_object_array_get_idx(oldArr, i));
            if (val && strcmp(val, name) != 0) {
                json_object_array_add(newArr, json_object_new_string(val));
            }
        }
    }
    if (enabled) {
        json_object_array_add(newArr, json_object_new_string(name));
    }

    json_object_object_add(sicompassObj, "programsToLoad", newArr);
    json_object_to_file_ext(configPath, root, JSON_C_TO_STRING_PRETTY);
    json_object_put(root);
    free(configPath);
}

// Rebuild the settings provider's FFON element after internal state changes
// (e.g. after settingsRemoveSection or loadProgram adds new settings sections).
static void refreshSettingsFfon(AppRenderer *appRenderer) {
    if (!s_settingsProvider || !appRenderer || appRenderer->ffonCount == 0) return;
    int settingsIdx = appRenderer->ffonCount - 1;
    if (appRenderer->providers[settingsIdx] != s_settingsProvider) return;
    ffonElementDestroy(appRenderer->ffon[settingsIdx]);
    appRenderer->ffon[settingsIdx] = providerGetInitialElement(s_settingsProvider);
}

void programsEnableProvider(const char *name, AppRenderer *appRenderer) {
    if (!s_settingsProvider || !appRenderer) return;

    // Load the program (registers provider + settings sections)
    Provider *newProvider = loadProgram(name, s_settingsProvider);
    if (!newProvider) return;

    // Init the provider
    if (newProvider->init) newProvider->init(newProvider);

    // Get initial element
    FfonElement *elem = providerGetInitialElement(newProvider);
    if (!elem) return;

    // Grow arrays if needed
    if (appRenderer->ffonCount >= appRenderer->ffonCapacity) {
        int newCap = appRenderer->ffonCapacity + 4;
        FfonElement **newFfon = realloc(appRenderer->ffon, newCap * sizeof(FfonElement*));
        Provider **newProviders = realloc(appRenderer->providers, newCap * sizeof(Provider*));
        if (!newFfon || !newProviders) return;
        appRenderer->ffon = newFfon;
        appRenderer->providers = newProviders;
        appRenderer->ffonCapacity = newCap;
    }

    // Find alphabetically sorted insertion point (before settings, which is last)
    int settingsIdx = appRenderer->ffonCount - 1;
    int insertIdx = 0;
    for (int i = 0; i < settingsIdx; i++) {
        if (strcasecmp(newProvider->name, appRenderer->providers[i]->name) > 0) {
            insertIdx = i + 1;
        }
    }

    // Shift entries right from insertIdx to make room
    for (int i = appRenderer->ffonCount; i > insertIdx; i--) {
        appRenderer->ffon[i] = appRenderer->ffon[i - 1];
        appRenderer->providers[i] = appRenderer->providers[i - 1];
    }
    appRenderer->ffon[insertIdx] = elem;
    appRenderer->providers[insertIdx] = newProvider;
    appRenderer->ffonCount++;

    // Rebuild settings FFON — new program may have registered settings sections
    refreshSettingsFfon(appRenderer);

    // Adjust navigation if pointing at settings
    if (appRenderer->currentId.ids[0] >= insertIdx) {
        appRenderer->currentId.ids[0]++;
    }

    createListCurrentLayer(appRenderer);
    appRenderer->needsRedraw = true;
}

void programsDisableProvider(const char *name, AppRenderer *appRenderer) {
    if (!appRenderer) return;

    // Find provider index
    int removeIdx = -1;
    for (int i = 0; i < appRenderer->ffonCount; i++) {
        if (appRenderer->providers[i] && nameMatchesProvider(name, appRenderer->providers[i]->name)) {
            removeIdx = i;
            break;
        }
    }

    if (removeIdx >= 0) {
        Provider *provider = appRenderer->providers[removeIdx];

        // Free the FFON element
        ffonElementDestroy(appRenderer->ffon[removeIdx]);

        // Shift remaining entries left
        for (int i = removeIdx; i < appRenderer->ffonCount - 1; i++) {
            appRenderer->ffon[i] = appRenderer->ffon[i + 1];
            appRenderer->providers[i] = appRenderer->providers[i + 1];
        }
        appRenderer->ffonCount--;

        // Cleanup provider
        providerUnregister(provider);
        providerDestroy(provider);

        // Adjust navigation
        if (appRenderer->currentId.ids[0] == removeIdx) {
            appRenderer->currentId.ids[0] = 0;
            appRenderer->currentId.depth = 1;
        } else if (appRenderer->currentId.ids[0] > removeIdx) {
            appRenderer->currentId.ids[0]--;
        }
        if (appRenderer->currentId.ids[0] >= appRenderer->ffonCount) {
            appRenderer->currentId.ids[0] = appRenderer->ffonCount - 1;
        }
    }

    // Always remove settings section and rebuild settings FFON,
    // even if the provider wasn't in the UI arrays (handles orphaned sections).
    if (s_settingsProvider) {
        settingsRemoveSection(s_settingsProvider, name);
        refreshSettingsFfon(appRenderer);
    }

    if (removeIdx >= 0) {
        createListCurrentLayer(appRenderer);
        appRenderer->needsRedraw = true;
    }
}

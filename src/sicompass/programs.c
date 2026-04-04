#include "programs.h"
#include "app_state.h"
#include <provider_interface.h>
#include <settings_provider.h>
#include <json-c/json.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <sys/stat.h>
#include <dirent.h>
#ifdef _WIN32
#include <win_compat.h>
#else
#include <dlfcn.h>
#endif

// Plugin manifest types
typedef enum { PLUGIN_SCRIPT, PLUGIN_FACTORY, PLUGIN_NATIVE } PluginType;
typedef enum { SETTING_TEXT, SETTING_CHECKBOX, SETTING_RADIO } SettingType;

typedef struct {
    SettingType type;
    char label[128];
    char key[64];
    char defaultValue[256];
    char options[8][64];  // radio only
    int optionCount;
    bool defaultChecked;  // checkbox only
} PluginSetting;

typedef struct {
    char name[64];
    char displayName[64];
    char entryPath[4096];
    PluginType type;
    bool supportsConfigFiles;
    PluginSetting settings[16];
    int settingCount;
} PluginManifest;

// Built-in program manifests
static const PluginManifest BUILTIN_MANIFESTS[] = {
    {
        .name = "tutorial",
        .displayName = "tutorial --> here you can go up, down or right",
        .entryPath = TUTORIAL_SCRIPT_PATH,
        .type = PLUGIN_SCRIPT,
    },
    {
        .name = "sales demo",
        .displayName = "sales demo",
        .entryPath = SALES_DEMO_SCRIPT_PATH,
        .type = PLUGIN_SCRIPT,
        .supportsConfigFiles = true,
        .settings = {{
            .type = SETTING_TEXT,
            .label = "save folder (product configuration)",
            .key = "saveFolder",
            .defaultValue = "Downloads",
        }},
        .settingCount = 1,
    },
    {
        .name = "chat client",
        .displayName = "chat client",
        .type = PLUGIN_FACTORY,
        .settings = {
            { .type = SETTING_TEXT, .label = "homeserver URL",
              .key = "chatHomeserver", .defaultValue = "https://matrix.org" },
            { .type = SETTING_TEXT, .label = "access token",
              .key = "chatAccessToken" },
            { .type = SETTING_TEXT, .label = "username",
              .key = "chatUsername" },
            { .type = SETTING_TEXT, .label = "password",
              .key = "chatPassword" },
        },
        .settingCount = 4,
    },
    {
        .name = "email client",
        .displayName = "email client",
        .type = PLUGIN_FACTORY,
        .settings = {
            { .type = SETTING_TEXT, .label = "IMAP URL",
              .key = "emailImapUrl", .defaultValue = "imaps://imap.gmail.com" },
            { .type = SETTING_TEXT, .label = "SMTP URL",
              .key = "emailSmtpUrl", .defaultValue = "smtps://smtp.gmail.com" },
            { .type = SETTING_TEXT, .label = "username",
              .key = "emailUsername" },
            { .type = SETTING_TEXT, .label = "password",
              .key = "emailPassword" },
            { .type = SETTING_TEXT, .label = "client ID (OAuth)",
              .key = "emailClientId" },
            { .type = SETTING_TEXT, .label = "client secret (OAuth)",
              .key = "emailClientSecret" },
        },
        .settingCount = 6,
    },
    {
        .name = "web browser",
        .displayName = "web browser",
        .type = PLUGIN_FACTORY,
    },
};
static const int BUILTIN_MANIFEST_COUNT = 5;

// User-discovered plugins
static PluginManifest s_userPlugins[32];
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

static bool isInDefaultPrograms(const char *name) {
    for (int i = 0; i < DEFAULT_PROGRAMS_COUNT; i++) {
        if (strcmp(name, DEFAULT_PROGRAMS[i]) == 0) return true;
    }
    return false;
}

// Check if a program is enabled in the "Available programs:" config section.
// Falls back to DEFAULT_PROGRAMS if no config entry exists.
static bool isEnabledInConfig(const char *name, json_object *availableSection) {
    if (!availableSection) return isInDefaultPrograms(name);
    char key[80];
    snprintf(key, sizeof(key), "enable_%s", name);
    json_object *val;
    if (json_object_object_get_ex(availableSection, key, &val)) {
        return json_object_get_boolean(val);
    }
    return isInDefaultPrograms(name);
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

static const PluginManifest* findManifest(const char *name) {
    for (int i = 0; i < BUILTIN_MANIFEST_COUNT; i++) {
        if (strcmp(name, BUILTIN_MANIFESTS[i].name) == 0)
            return &BUILTIN_MANIFESTS[i];
    }
    for (int i = 0; i < s_userPluginCount; i++) {
        if (strcmp(name, s_userPlugins[i].name) == 0)
            return &s_userPlugins[i];
    }
    return NULL;
}

static void applyManifestSettings(Provider *settingsProvider, const PluginManifest *m) {
    if (m->settingCount == 0) {
        settingsAddSection(settingsProvider, m->name);
        return;
    }
    for (int i = 0; i < m->settingCount; i++) {
        const PluginSetting *s = &m->settings[i];
        switch (s->type) {
        case SETTING_TEXT:
            settingsAddSectionText(settingsProvider, m->name,
                                   s->label, s->key, s->defaultValue);
            break;
        case SETTING_CHECKBOX:
            settingsAddSectionCheckbox(settingsProvider, m->name,
                                       s->label, s->key, s->defaultChecked);
            break;
        case SETTING_RADIO: {
            const char *opts[8];
            for (int j = 0; j < s->optionCount && j < 8; j++)
                opts[j] = s->options[j];
            settingsAddSectionRadio(settingsProvider, m->name,
                                    s->label, s->key,
                                    opts, s->optionCount, s->defaultValue);
            break;
        }
        }
    }
}

static Provider* loadProgram(const char *name, Provider *settingsProvider) {
    const PluginManifest *m = findManifest(name);
    if (m) {
        Provider *p = NULL;
        switch (m->type) {
        case PLUGIN_SCRIPT:
            p = scriptProviderCreate(m->name, m->displayName, m->entryPath);
            break;
        case PLUGIN_FACTORY:
            p = providerFactoryCreate(m->name);
            break;
        case PLUGIN_NATIVE: {
            void *handle = dlopen(m->entryPath, RTLD_NOW);
            if (!handle) return NULL;
            typedef const ProviderOps* (*InitFn)(void);
            InitFn initFn;
            *(void **)&initFn = dlsym(handle, "sicompass_plugin_init");
            if (!initFn) { dlclose(handle); return NULL; }
            const ProviderOps *ops = initFn();
            if (!ops) { dlclose(handle); return NULL; }
            p = providerCreate(ops);
            break;
        }
        }
        if (p) {
            p->supportsConfigFiles = m->supportsConfigFiles;
            providerRegister(p);
            applyManifestSettings(settingsProvider, m);
        }
        return p;
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

        PluginManifest *up = &s_userPlugins[s_userPluginCount];
        memset(up, 0, sizeof(*up));
        strncpy(up->name, json_object_get_string(nameObj), sizeof(up->name) - 1);
        strncpy(up->displayName, json_object_get_string(displayNameObj), sizeof(up->displayName) - 1);
        snprintf(up->entryPath, sizeof(up->entryPath), "%s%s/%s",
                 pluginsDir, entry->d_name, json_object_get_string(entryObj));

        json_object *configFilesObj;
        up->supportsConfigFiles = json_object_object_get_ex(manifest, "supportsConfigFiles", &configFilesObj)
                                  && json_object_get_boolean(configFilesObj);

        json_object *typeObj;
        if (json_object_object_get_ex(manifest, "type", &typeObj)) {
            const char *typeStr = json_object_get_string(typeObj);
            if (strcmp(typeStr, "native") == 0)
                up->type = PLUGIN_NATIVE;
            else if (strcmp(typeStr, "factory") == 0)
                up->type = PLUGIN_FACTORY;
        }

        // Parse optional settings array
        json_object *settingsArr;
        if (json_object_object_get_ex(manifest, "settings", &settingsArr)
            && json_object_is_type(settingsArr, json_type_array)) {
            int slen = json_object_array_length(settingsArr);
            if (slen > 16) slen = 16;
            for (int s = 0; s < slen; s++) {
                json_object *sObj = json_object_array_get_idx(settingsArr, s);
                PluginSetting *ps = &up->settings[up->settingCount];

                json_object *sType, *sLabel, *sKey, *sDefault;
                if (!json_object_object_get_ex(sObj, "type", &sType) ||
                    !json_object_object_get_ex(sObj, "label", &sLabel) ||
                    !json_object_object_get_ex(sObj, "key", &sKey))
                    continue;

                const char *typeStr = json_object_get_string(sType);
                if (strcmp(typeStr, "text") == 0)
                    ps->type = SETTING_TEXT;
                else if (strcmp(typeStr, "checkbox") == 0) {
                    ps->type = SETTING_CHECKBOX;
                    json_object *checked;
                    ps->defaultChecked = json_object_object_get_ex(sObj, "defaultChecked", &checked)
                                         && json_object_get_boolean(checked);
                } else if (strcmp(typeStr, "radio") == 0) {
                    ps->type = SETTING_RADIO;
                    json_object *optsArr;
                    if (json_object_object_get_ex(sObj, "options", &optsArr)) {
                        int olen = json_object_array_length(optsArr);
                        if (olen > 8) olen = 8;
                        for (int o = 0; o < olen; o++) {
                            strncpy(ps->options[o],
                                    json_object_get_string(json_object_array_get_idx(optsArr, o)),
                                    sizeof(ps->options[o]) - 1);
                        }
                        ps->optionCount = olen;
                    }
                } else continue;

                strncpy(ps->label, json_object_get_string(sLabel), sizeof(ps->label) - 1);
                strncpy(ps->key, json_object_get_string(sKey), sizeof(ps->key) - 1);
                if (json_object_object_get_ex(sObj, "default", &sDefault))
                    strncpy(ps->defaultValue, json_object_get_string(sDefault), sizeof(ps->defaultValue) - 1);

                up->settingCount++;
            }
        }

        json_object_put(manifest);
        s_userPluginCount++;
    }

    closedir(dir);
    free(pluginsDir);
}

// Register the "programs" priority section with checkboxes for all known programs
static void registerProgramsSection(Provider *settingsProvider, json_object *availableSection) {
    settingsAddPrioritySection(settingsProvider, "Available programs:");

    // Built-in programs
    for (int i = 0; i < BUILTIN_MANIFEST_COUNT; i++) {
        const char *name = BUILTIN_MANIFESTS[i].name;
        char configKey[80];
        snprintf(configKey, sizeof(configKey), "enable_%s", name);
        bool enabled = isEnabledInConfig(name, availableSection);
        settingsAddSectionCheckbox(settingsProvider, "Available programs:", name, configKey, enabled);
    }

    // User plugins
    for (int i = 0; i < s_userPluginCount; i++) {
        char configKey[80];
        snprintf(configKey, sizeof(configKey), "enable_%s", s_userPlugins[i].name);
        bool enabled = isEnabledInConfig(s_userPlugins[i].name, availableSection);
        settingsAddSectionCheckbox(settingsProvider, "Available programs:",
                                   s_userPlugins[i].displayName, configKey, enabled);
    }
}

void programsLoad(Provider *settingsProvider) {
    s_settingsProvider = settingsProvider;

    // Discover user plugins first
    discoverUserPlugins();

    // Read existing config to get enable_* state
    char *configPath = providerGetMainConfigPath();
    json_object *root = NULL;
    json_object *availableSection = NULL;
    if (configPath) {
        root = json_object_from_file(configPath);
        if (root) {
            json_object_object_get_ex(root, "Available programs:", &availableSection);

            // Migration: convert obsolete programsToLoad into enable_* keys
            json_object *sicompassObj;
            if (json_object_object_get_ex(root, "sicompass", &sicompassObj)) {
                json_object *programsToLoad;
                if (json_object_object_get_ex(sicompassObj, "programsToLoad", &programsToLoad)) {
                    // Ensure "Available programs:" section exists
                    if (!availableSection) {
                        availableSection = json_object_new_object();
                        json_object_object_add(root, "Available programs:", availableSection);
                    }
                    // Copy each program name into enable_* keys
                    if (json_object_is_type(programsToLoad, json_type_array)) {
                        int n = json_object_array_length(programsToLoad);
                        for (int i = 0; i < n; i++) {
                            json_object *item = json_object_array_get_idx(programsToLoad, i);
                            const char *name = json_object_get_string(item);
                            if (!name || !name[0]) continue;
                            char key[80];
                            snprintf(key, sizeof(key), "enable_%s", name);
                            json_object_object_add(availableSection,
                                                   key, json_object_new_boolean(true));
                        }
                    }
                    json_object_object_del(sicompassObj, "programsToLoad");
                    json_object_to_file_ext(configPath, root, JSON_C_TO_STRING_PRETTY);
                }
            }
        }
        free(configPath);
    }

    // Register store checkboxes BEFORE loading programs
    registerProgramsSection(settingsProvider, availableSection);

    // Load enabled programs
    for (int i = 0; i < BUILTIN_MANIFEST_COUNT; i++) {
        if (isEnabledInConfig(BUILTIN_MANIFESTS[i].name, availableSection))
            loadProgram(BUILTIN_MANIFESTS[i].name, settingsProvider);
    }
    for (int i = 0; i < s_userPluginCount; i++) {
        if (isEnabledInConfig(s_userPlugins[i].name, availableSection))
            loadProgram(s_userPlugins[i].name, settingsProvider);
    }

    if (root) json_object_put(root);
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

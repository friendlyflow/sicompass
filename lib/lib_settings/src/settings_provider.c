#include <settings_provider.h>
#include <provider_interface.h>
#include <provider_tags.h>
#include <ffon.h>
#include <json-c/json.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <sys/stat.h>

#define SETTINGS_MAX_SECTIONS 16
#define SETTINGS_SECTION_NAME_MAX 64
#define SETTINGS_MAX_RADIO_ENTRIES 16
#define SETTINGS_RADIO_KEY_MAX 64
#define SETTINGS_RADIO_OPTION_MAX 64
#define SETTINGS_MAX_RADIO_OPTIONS 8

typedef struct {
    char sectionName[SETTINGS_SECTION_NAME_MAX];
    char radioKey[SETTINGS_RADIO_KEY_MAX];
    char configKey[SETTINGS_RADIO_KEY_MAX];
    char options[SETTINGS_MAX_RADIO_OPTIONS][SETTINGS_RADIO_OPTION_MAX];
    int optionCount;
    char currentValue[SETTINGS_RADIO_OPTION_MAX];
} SettingsRadioEntry;

typedef struct {
    char currentPath[4096];  // must be first field (layout-compatible with GenericProviderState)
    const ProviderOps *ops;  // must be second field (unused, kept for layout compat)
    char colorScheme[32];
    SettingsApplyFn applyCallback;
    void *userdata;
    char sections[SETTINGS_MAX_SECTIONS][SETTINGS_SECTION_NAME_MAX];
    int sectionCount;
    SettingsRadioEntry radioEntries[SETTINGS_MAX_RADIO_ENTRIES];
    int radioEntryCount;
} SettingsProviderState;

// Build and return the full pre-populated settings tree.
// Returns an array of top-level section objects with their children already attached.
static FfonElement** settingsFetch(Provider *self, int *outCount) {
    SettingsProviderState *state = (SettingsProviderState *)self->state;

    int total = 1 + state->sectionCount;  // sicompass + registered sections
    FfonElement **arr = malloc(total * sizeof(FfonElement *));
    if (!arr) { *outCount = 0; return NULL; }
    int n = 0;

    // sicompass section: color scheme radio group
    bool isDark = (strcmp(state->colorScheme, "dark") == 0);
    FfonElement *radioGroup = ffonElementCreateObject("<radio>color scheme");
    ffonObjectAddElement(radioGroup->data.object,
        ffonElementCreateString(isDark ? "<checked>dark" : "dark"));
    ffonObjectAddElement(radioGroup->data.object,
        ffonElementCreateString(isDark ? "light" : "<checked>light"));

    FfonElement *sicompassObj = ffonElementCreateObject("sicompass");
    ffonObjectAddElement(sicompassObj->data.object, radioGroup);
    arr[n++] = sicompassObj;

    // Registered sections — radio group if configured, otherwise placeholder
    for (int i = 0; i < state->sectionCount; i++) {
        FfonElement *sectionObj = ffonElementCreateObject(state->sections[i]);

        // Look for a registered radio entry for this section
        SettingsRadioEntry *radio = NULL;
        for (int j = 0; j < state->radioEntryCount; j++) {
            if (strcmp(state->radioEntries[j].sectionName, state->sections[i]) == 0) {
                radio = &state->radioEntries[j];
                break;
            }
        }

        if (radio) {
            char radioKey[SETTINGS_RADIO_KEY_MAX + 8];
            snprintf(radioKey, sizeof(radioKey), "<radio>%s", radio->radioKey);
            FfonElement *radioGroup = ffonElementCreateObject(radioKey);
            for (int j = 0; j < radio->optionCount; j++) {
                bool checked = (strcmp(radio->options[j], radio->currentValue) == 0);
                char optBuf[SETTINGS_RADIO_OPTION_MAX + 10];
                snprintf(optBuf, sizeof(optBuf), "%s%s",
                         checked ? "<checked>" : "", radio->options[j]);
                ffonObjectAddElement(radioGroup->data.object, ffonElementCreateString(optBuf));
            }
            ffonObjectAddElement(sectionObj->data.object, radioGroup);
        } else {
            ffonObjectAddElement(sectionObj->data.object,
                ffonElementCreateString("no settings"));
        }

        arr[n++] = sectionObj;
    }

    *outCount = n;
    return arr;
}

static void settingsEnsureConfigDir(void) {
    char *configDir = providerGetConfigDir();
    if (!configDir) return;
    for (char *p = configDir + 1; *p; p++) {
        if (*p == '/') {
            char c = *p;
            *p = '\0';
            mkdir(configDir, 0755);
            *p = c;
        }
    }
    mkdir(configDir, 0755);
    free(configDir);
}

static void settingsSaveConfig(SettingsProviderState *state, const char *configPath) {
    settingsEnsureConfigDir();
    json_object *root = json_object_new_object();
    json_object_object_add(root, "colorScheme",
                           json_object_new_string(state->colorScheme));
    for (int i = 0; i < state->radioEntryCount; i++) {
        json_object_object_add(root, state->radioEntries[i].configKey,
                               json_object_new_string(state->radioEntries[i].currentValue));
    }
    json_object_to_file_ext(configPath, root, JSON_C_TO_STRING_PRETTY);
    json_object_put(root);
}

static void settingsLoadConfig(SettingsProviderState *state, const char *configPath) {
    json_object *root = json_object_from_file(configPath);
    if (!root) return;
    json_object *obj;
    if (json_object_object_get_ex(root, "colorScheme", &obj)) {
        const char *cs = json_object_get_string(obj);
        if (cs && (strcmp(cs, "dark") == 0 || strcmp(cs, "light") == 0)) {
            strncpy(state->colorScheme, cs, sizeof(state->colorScheme) - 1);
            state->colorScheme[sizeof(state->colorScheme) - 1] = '\0';
        }
    }
    for (int i = 0; i < state->radioEntryCount; i++) {
        SettingsRadioEntry *e = &state->radioEntries[i];
        if (json_object_object_get_ex(root, e->configKey, &obj)) {
            const char *val = json_object_get_string(obj);
            if (val) {
                for (int j = 0; j < e->optionCount; j++) {
                    if (strcmp(val, e->options[j]) == 0) {
                        strncpy(e->currentValue, val, sizeof(e->currentValue) - 1);
                        e->currentValue[sizeof(e->currentValue) - 1] = '\0';
                        break;
                    }
                }
            }
        }
    }
    json_object_put(root);
}

static void settingsInit(Provider *self) {
    SettingsProviderState *state = (SettingsProviderState *)self->state;
    strcpy(state->currentPath, "/");

    char *configPath = providerGetConfigPath("settings");
    if (configPath) {
        settingsLoadConfig(state, configPath);
        free(configPath);
    }

    if (state->applyCallback) {
        state->applyCallback("colorScheme", state->colorScheme, state->userdata);
        for (int i = 0; i < state->radioEntryCount; i++) {
            state->applyCallback(state->radioEntries[i].configKey,
                                 state->radioEntries[i].currentValue,
                                 state->userdata);
        }
    }
}

static void settingsOnRadioChange(Provider *self, const char *groupKey,
                                   const char *selectedValue) {
    SettingsProviderState *state = (SettingsProviderState *)self->state;

    if (strcmp(groupKey, "color scheme") == 0) {
        strncpy(state->colorScheme, selectedValue, sizeof(state->colorScheme) - 1);
        state->colorScheme[sizeof(state->colorScheme) - 1] = '\0';

        char *configPath = providerGetConfigPath("settings");
        if (configPath) {
            settingsSaveConfig(state, configPath);
            free(configPath);
        }

        if (state->applyCallback) {
            state->applyCallback("colorScheme", state->colorScheme, state->userdata);
        }
        return;
    }

    for (int i = 0; i < state->radioEntryCount; i++) {
        SettingsRadioEntry *e = &state->radioEntries[i];
        if (strcmp(groupKey, e->radioKey) == 0) {
            strncpy(e->currentValue, selectedValue, sizeof(e->currentValue) - 1);
            e->currentValue[sizeof(e->currentValue) - 1] = '\0';

            char *configPath = providerGetConfigPath("settings");
            if (configPath) {
                settingsSaveConfig(state, configPath);
                free(configPath);
            }

            if (state->applyCallback) {
                state->applyCallback(e->configKey, e->currentValue, state->userdata);
            }
            return;
        }
    }
}

static void settingsPushPath(Provider *self, const char *segment) {
    SettingsProviderState *state = (SettingsProviderState *)self->state;
    int pathLen = strlen(state->currentPath);
    int segLen = strlen(segment);

    if (segLen > 0 && segment[segLen - 1] == '/') segLen--;

    if (pathLen > 0 && state->currentPath[pathLen - 1] != '/') {
        if (pathLen + 1 < (int)sizeof(state->currentPath)) {
            state->currentPath[pathLen++] = '/';
            state->currentPath[pathLen] = '\0';
        }
    }
    if (pathLen + segLen < (int)sizeof(state->currentPath)) {
        strncat(state->currentPath, segment, segLen);
    }
}

static void settingsPopPath(Provider *self) {
    SettingsProviderState *state = (SettingsProviderState *)self->state;
    int len = strlen(state->currentPath);
    if (len <= 1) return;
    if (state->currentPath[len - 1] == '/') state->currentPath[--len] = '\0';
    char *lastSlash = strrchr(state->currentPath, '/');
    if (lastSlash && lastSlash != state->currentPath) {
        *lastSlash = '\0';
    } else if (lastSlash == state->currentPath) {
        state->currentPath[1] = '\0';
    }
}

static const char* settingsGetCurrentPath(Provider *self) {
    return ((SettingsProviderState *)self->state)->currentPath;
}

Provider* settingsProviderCreate(SettingsApplyFn applyCallback, void *userdata) {
    SettingsProviderState *state = calloc(1, sizeof(SettingsProviderState));
    if (!state) return NULL;

    strcpy(state->currentPath, "/");
    strcpy(state->colorScheme, "dark");
    state->applyCallback = applyCallback;
    state->userdata = userdata;
    state->sectionCount = 0;

    Provider *provider = calloc(1, sizeof(Provider));
    if (!provider) { free(state); return NULL; }

    provider->name = "settings";
    provider->state = state;
    provider->fetch = settingsFetch;
    provider->init = settingsInit;
    provider->pushPath = settingsPushPath;
    provider->popPath = settingsPopPath;
    provider->getCurrentPath = settingsGetCurrentPath;
    provider->onRadioChange = settingsOnRadioChange;

    return provider;
}

void settingsAddSection(Provider *provider, const char *sectionName) {
    if (!provider || !sectionName) return;
    SettingsProviderState *state = (SettingsProviderState *)provider->state;
    if (state->sectionCount >= SETTINGS_MAX_SECTIONS) return;
    strncpy(state->sections[state->sectionCount], sectionName,
            SETTINGS_SECTION_NAME_MAX - 1);
    state->sections[state->sectionCount][SETTINGS_SECTION_NAME_MAX - 1] = '\0';
    state->sectionCount++;
}

void settingsAddSectionRadio(Provider *provider,
                             const char *sectionName,
                             const char *radioKey,
                             const char *configKey,
                             const char **options,
                             int optionCount,
                             const char *defaultValue) {
    if (!provider || !sectionName || !radioKey || !configKey || !options || optionCount <= 0) return;
    SettingsProviderState *state = (SettingsProviderState *)provider->state;
    if (state->radioEntryCount >= SETTINGS_MAX_RADIO_ENTRIES) return;
    if (optionCount > SETTINGS_MAX_RADIO_OPTIONS) optionCount = SETTINGS_MAX_RADIO_OPTIONS;

    SettingsRadioEntry *e = &state->radioEntries[state->radioEntryCount];
    strncpy(e->sectionName, sectionName, sizeof(e->sectionName) - 1);
    e->sectionName[sizeof(e->sectionName) - 1] = '\0';
    strncpy(e->radioKey, radioKey, sizeof(e->radioKey) - 1);
    e->radioKey[sizeof(e->radioKey) - 1] = '\0';
    strncpy(e->configKey, configKey, sizeof(e->configKey) - 1);
    e->configKey[sizeof(e->configKey) - 1] = '\0';
    e->optionCount = optionCount;
    for (int i = 0; i < optionCount; i++) {
        strncpy(e->options[i], options[i], sizeof(e->options[i]) - 1);
        e->options[i][sizeof(e->options[i]) - 1] = '\0';
    }
    strncpy(e->currentValue, defaultValue ? defaultValue : options[0], sizeof(e->currentValue) - 1);
    e->currentValue[sizeof(e->currentValue) - 1] = '\0';
    state->radioEntryCount++;

    // Register the section if not already present
    bool found = false;
    for (int i = 0; i < state->sectionCount; i++) {
        if (strcmp(state->sections[i], sectionName) == 0) { found = true; break; }
    }
    if (!found) settingsAddSection(provider, sectionName);
}

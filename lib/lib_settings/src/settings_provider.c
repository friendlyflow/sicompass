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
#define SETTINGS_MAX_TEXT_ENTRIES 16
#define SETTINGS_TEXT_VALUE_MAX 256

typedef struct {
    char sectionName[SETTINGS_SECTION_NAME_MAX];
    char radioKey[SETTINGS_RADIO_KEY_MAX];
    char configKey[SETTINGS_RADIO_KEY_MAX];
    char options[SETTINGS_MAX_RADIO_OPTIONS][SETTINGS_RADIO_OPTION_MAX];
    int optionCount;
    char currentValue[SETTINGS_RADIO_OPTION_MAX];
} SettingsRadioEntry;

typedef struct {
    char sectionName[SETTINGS_SECTION_NAME_MAX];
    char label[SETTINGS_RADIO_KEY_MAX];
    char configKey[SETTINGS_RADIO_KEY_MAX];
    char currentValue[SETTINGS_TEXT_VALUE_MAX];
} SettingsTextEntry;

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
    SettingsTextEntry textEntries[SETTINGS_MAX_TEXT_ENTRIES];
    int textEntryCount;
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

    // Registered sections — radio groups, text entries, or placeholder
    for (int i = 0; i < state->sectionCount; i++) {
        FfonElement *sectionObj = ffonElementCreateObject(state->sections[i]);
        bool hasContent = false;

        // Look for registered radio entries for this section
        for (int j = 0; j < state->radioEntryCount; j++) {
            if (strcmp(state->radioEntries[j].sectionName, state->sections[i]) == 0) {
                SettingsRadioEntry *radio = &state->radioEntries[j];
                char radioKey[SETTINGS_RADIO_KEY_MAX + 8];
                snprintf(radioKey, sizeof(radioKey), "<radio>%s", radio->radioKey);
                FfonElement *radioGroup = ffonElementCreateObject(radioKey);
                for (int k = 0; k < radio->optionCount; k++) {
                    bool checked = (strcmp(radio->options[k], radio->currentValue) == 0);
                    char optBuf[SETTINGS_RADIO_OPTION_MAX + 10];
                    snprintf(optBuf, sizeof(optBuf), "%s%s",
                             checked ? "<checked>" : "", radio->options[k]);
                    ffonObjectAddElement(radioGroup->data.object, ffonElementCreateString(optBuf));
                }
                ffonObjectAddElement(sectionObj->data.object, radioGroup);
                hasContent = true;
            }
        }

        // Look for registered text entries for this section
        for (int j = 0; j < state->textEntryCount; j++) {
            if (strcmp(state->textEntries[j].sectionName, state->sections[i]) == 0) {
                SettingsTextEntry *text = &state->textEntries[j];
                FfonElement *textObj = ffonElementCreateObject(text->label);
                char inputBuf[SETTINGS_TEXT_VALUE_MAX + 20];
                snprintf(inputBuf, sizeof(inputBuf), "<input>%s</input>", text->currentValue);
                ffonObjectAddElement(textObj->data.object, ffonElementCreateString(inputBuf));
                ffonObjectAddElement(sectionObj->data.object, textObj);
                hasContent = true;
            }
        }

        if (!hasContent) {
            ffonObjectAddElement(sectionObj->data.object,
                ffonElementCreateString("no settings"));
        }

        arr[n++] = sectionObj;
    }

    *outCount = n;
    return arr;
}

static void settingsEnsureConfigDir(void) {
    char *configPath = providerGetMainConfigPath();
    if (!configPath) return;
    // Walk up to the parent directory of the config file
    char *dir = strdup(configPath);
    free(configPath);
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

static void settingsSaveConfig(SettingsProviderState *state, const char *configPath) {
    settingsEnsureConfigDir();
    // Read existing file so we preserve fields we don't own (e.g. programsToLoad)
    json_object *root = json_object_from_file(configPath);
    if (!root) root = json_object_new_object();

    // sicompass section: colorScheme
    json_object *sicompassObj = NULL;
    if (!json_object_object_get_ex(root, "sicompass", &sicompassObj)) {
        sicompassObj = json_object_new_object();
        json_object_object_add(root, "sicompass", sicompassObj);
    }
    json_object_object_add(sicompassObj, "colorScheme",
                           json_object_new_string(state->colorScheme));

    // Per-section radio entries namespaced by section name
    for (int i = 0; i < state->radioEntryCount; i++) {
        SettingsRadioEntry *e = &state->radioEntries[i];
        json_object *sectionObj = NULL;
        if (!json_object_object_get_ex(root, e->sectionName, &sectionObj)) {
            sectionObj = json_object_new_object();
            json_object_object_add(root, e->sectionName, sectionObj);
        }
        json_object_object_add(sectionObj, e->configKey,
                               json_object_new_string(e->currentValue));
    }

    // Per-section text entries namespaced by section name
    for (int i = 0; i < state->textEntryCount; i++) {
        SettingsTextEntry *e = &state->textEntries[i];
        json_object *sectionObj = NULL;
        if (!json_object_object_get_ex(root, e->sectionName, &sectionObj)) {
            sectionObj = json_object_new_object();
            json_object_object_add(root, e->sectionName, sectionObj);
        }
        json_object_object_add(sectionObj, e->configKey,
                               json_object_new_string(e->currentValue));
    }

    json_object_to_file_ext(configPath, root, JSON_C_TO_STRING_PRETTY);
    json_object_put(root);
}

static void settingsLoadConfig(SettingsProviderState *state, const char *configPath) {
    json_object *root = json_object_from_file(configPath);
    if (!root) return;

    // sicompass section: colorScheme
    json_object *sicompassObj;
    if (json_object_object_get_ex(root, "sicompass", &sicompassObj)) {
        json_object *obj;
        if (json_object_object_get_ex(sicompassObj, "colorScheme", &obj)) {
            const char *cs = json_object_get_string(obj);
            if (cs && (strcmp(cs, "dark") == 0 || strcmp(cs, "light") == 0)) {
                strncpy(state->colorScheme, cs, sizeof(state->colorScheme) - 1);
                state->colorScheme[sizeof(state->colorScheme) - 1] = '\0';
            }
        }
    }

    // Per-section radio entries namespaced by section name
    for (int i = 0; i < state->radioEntryCount; i++) {
        SettingsRadioEntry *e = &state->radioEntries[i];
        json_object *sectionObj;
        if (json_object_object_get_ex(root, e->sectionName, &sectionObj)) {
            json_object *obj;
            if (json_object_object_get_ex(sectionObj, e->configKey, &obj)) {
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
    }

    // Per-section text entries namespaced by section name
    for (int i = 0; i < state->textEntryCount; i++) {
        SettingsTextEntry *e = &state->textEntries[i];
        json_object *sectionObj;
        if (json_object_object_get_ex(root, e->sectionName, &sectionObj)) {
            json_object *obj;
            if (json_object_object_get_ex(sectionObj, e->configKey, &obj)) {
                const char *val = json_object_get_string(obj);
                if (val && val[0] != '\0') {
                    strncpy(e->currentValue, val, sizeof(e->currentValue) - 1);
                    e->currentValue[sizeof(e->currentValue) - 1] = '\0';
                }
            }
        }
    }

    json_object_put(root);
}

static void settingsInit(Provider *self) {
    SettingsProviderState *state = (SettingsProviderState *)self->state;
    strcpy(state->currentPath, "/");

    char *configPath = providerGetMainConfigPath();
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
        for (int i = 0; i < state->textEntryCount; i++) {
            state->applyCallback(state->textEntries[i].configKey,
                                 state->textEntries[i].currentValue,
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

        char *configPath = providerGetMainConfigPath();
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

            char *configPath = providerGetMainConfigPath();
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

// commitEdit: detect text entry edits based on current path and update state.
static bool settingsCommitEdit(Provider *self, const char *oldContent __attribute__((unused)), const char *newContent) {
    SettingsProviderState *state = (SettingsProviderState *)self->state;

    // Path format: /<section>/<label> — extract the section and label
    const char *path = state->currentPath;
    if (path[0] != '/') return false;

    // Find section from path: skip leading '/', extract up to next '/'
    const char *sectionStart = path + 1;
    const char *sectionEnd = strchr(sectionStart, '/');
    if (!sectionEnd) return false;

    char section[SETTINGS_SECTION_NAME_MAX];
    int sectionLen = (int)(sectionEnd - sectionStart);
    if (sectionLen >= (int)sizeof(section)) sectionLen = (int)sizeof(section) - 1;
    strncpy(section, sectionStart, sectionLen);
    section[sectionLen] = '\0';

    // The label is everything after the second '/'
    const char *label = sectionEnd + 1;

    for (int i = 0; i < state->textEntryCount; i++) {
        SettingsTextEntry *e = &state->textEntries[i];
        if (strcmp(e->sectionName, section) == 0 && strcmp(e->label, label) == 0) {
            strncpy(e->currentValue, newContent, sizeof(e->currentValue) - 1);
            e->currentValue[sizeof(e->currentValue) - 1] = '\0';

            char *configPath = providerGetMainConfigPath();
            if (configPath) {
                settingsSaveConfig(state, configPath);
                free(configPath);
            }

            if (state->applyCallback) {
                state->applyCallback(e->configKey, e->currentValue, state->userdata);
            }
            return true;
        }
    }
    return false;
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
    provider->commitEdit = settingsCommitEdit;

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

void settingsAddSectionText(Provider *provider,
                            const char *sectionName,
                            const char *label,
                            const char *configKey,
                            const char *defaultValue) {
    if (!provider || !sectionName || !label || !configKey) return;
    SettingsProviderState *state = (SettingsProviderState *)provider->state;
    if (state->textEntryCount >= SETTINGS_MAX_TEXT_ENTRIES) return;

    SettingsTextEntry *e = &state->textEntries[state->textEntryCount];
    strncpy(e->sectionName, sectionName, sizeof(e->sectionName) - 1);
    e->sectionName[sizeof(e->sectionName) - 1] = '\0';
    strncpy(e->label, label, sizeof(e->label) - 1);
    e->label[sizeof(e->label) - 1] = '\0';
    strncpy(e->configKey, configKey, sizeof(e->configKey) - 1);
    e->configKey[sizeof(e->configKey) - 1] = '\0';
    strncpy(e->currentValue, defaultValue ? defaultValue : "", sizeof(e->currentValue) - 1);
    e->currentValue[sizeof(e->currentValue) - 1] = '\0';
    state->textEntryCount++;

    // Register the section if not already present
    bool found = false;
    for (int i = 0; i < state->sectionCount; i++) {
        if (strcmp(state->sections[i], sectionName) == 0) { found = true; break; }
    }
    if (!found) settingsAddSection(provider, sectionName);
}

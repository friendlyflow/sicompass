#include "programs.h"
#include "provider.h"
#include <provider_interface.h>
#include <filebrowser_provider.h>
#include <settings_provider.h>
#include <json-c/json.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <sys/stat.h>

static const char *DEFAULT_PROGRAMS[] = {"tutorial", "file browser"};
static const int DEFAULT_PROGRAMS_COUNT = 2;

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
    // Read-merge-write: preserve any existing settings in the file
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

static void loadProgram(const char *name, Provider *settingsProvider) {
    if (strcmp(name, "tutorial") == 0) {
        Provider *p = scriptProviderCreate("tutorial", "tutorial", TUTORIAL_SCRIPT_PATH);
        if (p) {
            providerRegister(p);
            settingsAddSection(settingsProvider, "tutorial");
        }
    } else if (strcmp(name, "file browser") == 0) {
        Provider *p = filebrowserGetProvider();
        providerRegister(p);
        const char *sortOptions[] = {"alphanumerically", "chronologically"};
        settingsAddSectionRadio(settingsProvider, "file browser",
                                "global sorting", "sortOrder",
                                sortOptions, 2, "alphanumerically");
    }
}

void programsLoad(Provider *settingsProvider) {
    char *configPath = providerGetMainConfigPath();
    if (!configPath) return;

    json_object *root = json_object_from_file(configPath);
    if (!root) {
        writeDefaultConfig(configPath);
        free(configPath);
        for (int i = 0; i < DEFAULT_PROGRAMS_COUNT; i++) {
            loadProgram(DEFAULT_PROGRAMS[i], settingsProvider);
        }
        return;
    }
    free(configPath);

    json_object *sicompassObj;
    if (!json_object_object_get_ex(root, "sicompass", &sicompassObj)) {
        json_object_put(root);
        return;
    }

    json_object *arr;
    if (!json_object_object_get_ex(sicompassObj, "programsToLoad", &arr) ||
        !json_object_is_type(arr, json_type_array)) {
        json_object_put(root);
        return;
    }

    int len = json_object_array_length(arr);
    for (int i = 0; i < len; i++) {
        json_object *item = json_object_array_get_idx(arr, i);
        const char *name = json_object_get_string(item);
        if (name) loadProgram(name, settingsProvider);
    }

    json_object_put(root);
}

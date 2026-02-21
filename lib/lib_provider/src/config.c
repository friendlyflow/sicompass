#include "provider_interface.h"
#include "platform.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CONFIG_SUBDIR "sicompass/providers"

char* providerGetConfigDir(void) {
    char *configHome = platformGetConfigHome();
    if (!configHome) return NULL;

    const char *sep = platformGetPathSeparator();
    size_t len = strlen(configHome) + strlen(CONFIG_SUBDIR) + strlen(sep) + 1;
    char *result = malloc(len);
    if (!result) {
        free(configHome);
        return NULL;
    }

    snprintf(result, len, "%s%s%s", configHome, CONFIG_SUBDIR, sep);
    free(configHome);

    return result;
}

char* providerGetConfigPath(const char *providerName) {
    if (!providerName) return NULL;

    char *configDir = providerGetConfigDir();
    if (!configDir) return NULL;

    size_t len = strlen(configDir) + strlen(providerName) + strlen(".json") + 1;
    char *result = malloc(len);
    if (!result) {
        free(configDir);
        return NULL;
    }

    snprintf(result, len, "%s%s.json", configDir, providerName);
    free(configDir);

    return result;
}

char* providerGetMainConfigPath(void) {
    char *configHome = platformGetConfigHome();
    if (!configHome) return NULL;

    const char *sep = platformGetPathSeparator();
    const char *subdir = "sicompass";
    const char *filename = "settings.json";
    size_t len = strlen(configHome) + strlen(subdir) + strlen(sep) + strlen(filename) + 1;
    char *result = malloc(len);
    if (!result) { free(configHome); return NULL; }
    snprintf(result, len, "%s%s%s%s", configHome, subdir, sep, filename);
    free(configHome);

    return result;
}

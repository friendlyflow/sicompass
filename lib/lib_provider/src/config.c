#include "provider_interface.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#define CONFIG_SUBDIR "sicompass/providers"

char* providerGetConfigDir(void) {
    const char *configHome = getenv("XDG_CONFIG_HOME");
    char *result;

    if (configHome && configHome[0] != '\0') {
        size_t len = strlen(configHome) + 1 + strlen(CONFIG_SUBDIR) + 2;
        result = malloc(len);
        if (!result) return NULL;
        snprintf(result, len, "%s/%s/", configHome, CONFIG_SUBDIR);
    } else {
        const char *home = getenv("HOME");
        if (!home) return NULL;

        size_t len = strlen(home) + strlen("/.config/") + strlen(CONFIG_SUBDIR) + 2;
        result = malloc(len);
        if (!result) return NULL;
        snprintf(result, len, "%s/.config/%s/", home, CONFIG_SUBDIR);
    }

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

#pragma once

#include <stdbool.h>
#include <provider_interface.h>

// Forward declaration
typedef struct AppRenderer AppRenderer;

// Read the programs config and register all listed providers.
// settingsProvider must be created before calling this so programs can register their settings.
void programsLoad(Provider *settingsProvider);

// Update the programsToLoad array in settings.json (add/remove name).
void programsUpdateEnabled(const char *name, bool enabled);

// Hot-enable a provider at runtime: create, register, insert into appRenderer arrays.
void programsEnableProvider(const char *name, AppRenderer *appRenderer);

// Hot-disable a provider at runtime: remove from appRenderer arrays, unregister, destroy.
void programsDisableProvider(const char *name, AppRenderer *appRenderer);

#pragma once

#include <provider_interface.h>

// Read the programs config and register all listed providers.
// settingsProvider must be created before calling this so programs can register their settings.
void programsLoad(Provider *settingsProvider);

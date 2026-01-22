#pragma once

#include <stdbool.h>
#include <ffon.h>
#include <provider_interface.h>

// Forward declaration
typedef struct AppRenderer AppRenderer;

// Provider registry
void providerRegister(Provider *provider);
Provider* providerFindForElement(const char *elementKey);
Provider* providerFindByName(const char *name);
void providerInitAll(void);
void providerCleanupAll(void);

// Navigation using providers
bool providerNavigateRight(AppRenderer *appRenderer);
bool providerNavigateLeft(AppRenderer *appRenderer);

// Get current path for an element's provider
const char* providerGetCurrentPath(const char *elementKey);

// Data operations for editing (called by handlers.c)
char* providerGetEditableContent(const char *elementKey);
bool providerCommitEdit(const char *elementKey, const char *oldContent, const char *newContent);
char* providerFormatUpdatedKey(const char *elementKey, const char *newContent);

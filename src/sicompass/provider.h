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

// Create operations
bool providerCreateDirectory(const char *elementKey, const char *name);
bool providerCreateFile(const char *elementKey, const char *name);

// Command operations
const char** providerGetCommands(const char *elementKey, int *outCount);
FfonElement* providerHandleCommand(const char *elementKey, const char *command,
                                    int elementType,
                                    char *errorMsg, int errorMsgSize);
ProviderListItem* providerGetCommandListItems(const char *elementKey, const char *command, int *outCount);
bool providerExecuteCommand(const char *elementKey, const char *command, const char *selection);

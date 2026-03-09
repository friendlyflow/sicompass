#pragma once

#include <stdbool.h>
#include <ffon.h>
#include <provider_interface.h>

// Forward declaration
typedef struct AppRenderer AppRenderer;

// Provider registry
void providerRegister(Provider *provider);

// Auth registry: register an API key for a URL origin (used for Bearer auth on <link> fetches)
void providerRegisterAuth(const char *origin, const char *apiKey);
Provider* providerFindByName(const char *name);
int providerGetRegisteredCount(void);
Provider* providerGetRegisteredAt(int i);
void providerInitAll(void);
void providerCleanupAll(void);

// Get active provider from navigation context
Provider* providerGetActive(AppRenderer *appRenderer);

// Navigation using providers
bool providerNavigateRight(AppRenderer *appRenderer);
bool providerNavigateLeft(AppRenderer *appRenderer);

// Provider operations (dispatch via active provider)
const char* providerGetCurrentPath(AppRenderer *appRenderer);
bool providerCommitEdit(AppRenderer *appRenderer, const char *oldContent, const char *newContent);
bool providerCreateDirectory(AppRenderer *appRenderer, const char *name);
bool providerCreateFile(AppRenderer *appRenderer, const char *name);
bool providerDeleteItem(AppRenderer *appRenderer, const char *name);
bool providerCopyItem(AppRenderer *appRenderer, const char *srcDir, const char *srcName,
                      const char *destDir, const char *destName);

// Command operations
const char** providerGetCommands(AppRenderer *appRenderer, int *outCount);
FfonElement* providerHandleCommand(AppRenderer *appRenderer, const char *command,
                                    const char *elementKey, int elementType,
                                    char *errorMsg, int errorMsgSize);
ProviderListItem* providerGetCommandListItems(AppRenderer *appRenderer, const char *command, int *outCount);
bool providerExecuteCommand(AppRenderer *appRenderer, const char *command, const char *selection);

// Refresh the current directory listing by clearing the cached children and re-fetching
void providerRefreshCurrentDirectory(AppRenderer *appRenderer);

// Refresh the nearest ancestor <link> tag by clearing its cached children.
// Returns true if a link was found and refreshed, false if not inside a link.
bool providerRefreshLink(AppRenderer *appRenderer);

// Teleport a provider to absoluteDir: set its path, clear root FFON children, re-fetch.
// Returns the index of targetFilename in the new listing, or -1 if not found.
int providerNavigateToPath(AppRenderer *appRenderer, int rootIdx,
                           const char *absoluteDir, const char *targetFilename);

// Notify the active provider that a radio item was selected.
// elementId: ID of the newly checked radio child element.
void providerNotifyRadioChanged(AppRenderer *appRenderer, IdArray *elementId);

// Notify the active provider that a button element was activated.
// elementId: ID of the activated <button> string element.
void providerNotifyButtonPressed(AppRenderer *appRenderer, IdArray *elementId);

// Notify the active provider that a checkbox element was toggled.
// elementId: ID of the toggled <checkbox> string element.
void providerNotifyCheckboxChanged(AppRenderer *appRenderer, IdArray *elementId);

// Remove a provider from the global registry.
void providerUnregister(Provider *provider);

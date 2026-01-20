#ifndef PROVIDER_H
#define PROVIDER_H

#include <stdbool.h>
#include <ffon.h>

// Forward declaration
typedef struct AppRenderer AppRenderer;

// Callback to fetch children for an FFON_OBJECT element
// Parameters:
//   - appRenderer: the app state (includes currentUri)
//   - parent_key: the key of the object being entered (e.g., "Documents/")
//   - out_count: output parameter for number of elements returned
// Returns: array of FfonElement* for the children, or NULL on error
typedef FfonElement** (*ProviderFetchCallback)(AppRenderer *appRenderer, const char *parent_key, int *out_count);

// Provider registration
void providerSetFetchCallback(ProviderFetchCallback callback);
ProviderFetchCallback providerGetFetchCallback(void);

// Generic navigation functions that use the provider
// These replace direct IdArray manipulation for inter-library navigation
bool providerNavigateRight(AppRenderer *appRenderer);
bool providerNavigateLeft(AppRenderer *appRenderer);

// URI helpers
void providerUriAppend(char *uri, int max_len, const char *segment);
void providerUriPop(char *uri);

#endif /* PROVIDER_H */

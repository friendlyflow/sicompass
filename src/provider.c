#include "provider.h"
#include "view.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static ProviderFetchCallback g_fetchCallback = NULL;

void providerSetFetchCallback(ProviderFetchCallback callback) {
    g_fetchCallback = callback;
}

ProviderFetchCallback providerGetFetchCallback(void) {
    return g_fetchCallback;
}

void providerUriAppend(char *uri, int max_len, const char *segment) {
    if (!uri || !segment) return;

    int uri_len = strlen(uri);
    int seg_len = strlen(segment);

    // Remove trailing slash from segment if present (directories have "name/")
    if (seg_len > 0 && segment[seg_len - 1] == '/') {
        seg_len--;
    }

    // Ensure we have a slash before appending (unless uri is empty or already ends with /)
    if (uri_len > 0 && uri[uri_len - 1] != '/') {
        if (uri_len + 1 < max_len) {
            uri[uri_len++] = '/';
            uri[uri_len] = '\0';
        }
    }

    // Append segment
    if (uri_len + seg_len < max_len) {
        strncat(uri, segment, seg_len);
    }
}

void providerUriPop(char *uri) {
    if (!uri) return;

    int len = strlen(uri);
    if (len <= 1) return;  // Don't pop past root "/"

    // Remove trailing slash if present
    if (uri[len - 1] == '/') {
        uri[--len] = '\0';
    }

    // Find last slash
    char *last_slash = strrchr(uri, '/');
    if (last_slash && last_slash != uri) {
        *last_slash = '\0';
    } else if (last_slash == uri) {
        // Keep root "/"
        uri[1] = '\0';
    }
}

bool providerNavigateRight(AppRenderer *appRenderer) {
    // Get current element
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
    if (!arr || count == 0) {
        return false;
    }

    int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (idx < 0 || idx >= count) {
        return false;
    }

    FfonElement *elem = arr[idx];
    if (elem->type != FFON_OBJECT) {
        return false;
    }

    FfonObject *obj = elem->data.object;

    // If the object already has children loaded, just navigate into it
    if (obj->count > 0) {
        idArrayPush(&appRenderer->currentId, 0);
        return true;
    }

    // Otherwise, fetch children from the provider
    if (!g_fetchCallback) {
        return false;
    }

    // Get the key of the object we're entering
    const char *key = obj->key;

    // Update URI before fetching
    providerUriAppend(appRenderer->currentUri, MAX_URI_LENGTH, key);
    printf("providerNavigateRight: key='%s', uri='%s'\n", key, appRenderer->currentUri);

    // Fetch children from the provider
    int childCount = 0;
    FfonElement **children = g_fetchCallback(appRenderer, key, &childCount);
    printf("providerNavigateRight: fetched %d children\n", childCount);
    if (!children || childCount == 0) {
        // Revert URI on failure
        providerUriPop(appRenderer->currentUri);
        if (children) free(children);
        return false;
    }

    // Add fetched children
    for (int i = 0; i < childCount; i++) {
        ffonObjectAddElement(obj, children[i]);
    }
    free(children);

    // Navigate into the object
    idArrayPush(&appRenderer->currentId, 0);

    return true;
}

bool providerNavigateLeft(AppRenderer *appRenderer) {
    if (appRenderer->currentId.depth <= 1) {
        return false;
    }

    // Pop URI segment
    providerUriPop(appRenderer->currentUri);

    idArrayPop(&appRenderer->currentId);
    return true;
}

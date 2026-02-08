#include "view.h"
#include "provider.h"
#include "unicode_search.h"
#include <stdlib.h>
#include <string.h>

void clearListCurrentLayer(AppRenderer *appRenderer) {
    if (appRenderer->totalListCurrentLayer) {
        for (int i = 0; i < appRenderer->totalListCount; i++) {
            free(appRenderer->totalListCurrentLayer[i].label);
            free(appRenderer->totalListCurrentLayer[i].data);
        }
        free(appRenderer->totalListCurrentLayer);
        appRenderer->totalListCurrentLayer = NULL;
        appRenderer->totalListCount = 0;
    }

    if (appRenderer->filteredListCurrentLayer) {
        // Don't free values, they're shared with totalListCurrentLayer
        free(appRenderer->filteredListCurrentLayer);
        appRenderer->filteredListCurrentLayer = NULL;
        appRenderer->filteredListCount = 0;
    }

    appRenderer->listIndex = 0;
}

void createListCurrentLayer(AppRenderer *appRenderer) {
    clearListCurrentLayer(appRenderer);

    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
        appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT
    ) {
        // List all elements in current layer
        int count;
        FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
        if (!arr) return;

        appRenderer->totalListCurrentLayer = calloc(count, sizeof(ListItem));
        if (!appRenderer->totalListCurrentLayer) return;

        IdArray thisId;
        idArrayCopy(&thisId, &appRenderer->currentId);
        thisId.ids[thisId.depth - 1] = 0;

        for (int i = 0; i < count; i++) {
            FfonElement *elem = arr[i];

            idArrayCopy(&appRenderer->totalListCurrentLayer[appRenderer->totalListCount].id, &thisId);

            if (elem->type == FFON_STRING) {
                // Strip provider tags from display
                bool hasTags = (providerFindForElement(elem->data.string) != NULL);
                char *stripped = hasTags ? providerGetEditableContent(elem->data.string) : NULL;
                char prefixed[MAX_LINE_LENGTH];
                snprintf(prefixed, sizeof(prefixed), "%s %s",
                         hasTags ? "-i" : "-",
                         stripped ? stripped : elem->data.string);
                appRenderer->totalListCurrentLayer[appRenderer->totalListCount].label =
                    strdup(prefixed);
                free(stripped);
            } else {
                // Strip provider tags from display
                bool hasTags = (providerFindForElement(elem->data.object->key) != NULL);
                char *stripped = hasTags ? providerGetEditableContent(elem->data.object->key) : NULL;
                char prefixed[MAX_LINE_LENGTH];
                snprintf(prefixed, sizeof(prefixed), "%s %s",
                         hasTags ? "+i" : "+",
                         stripped ? stripped : elem->data.object->key);
                appRenderer->totalListCurrentLayer[appRenderer->totalListCount].label =
                    strdup(prefixed);
                free(stripped);
            }

            appRenderer->totalListCount++;
            thisId.ids[thisId.depth - 1]++;
        }

    } else if (appRenderer->currentCoordinate == COORDINATE_COMMAND) {
        if (appRenderer->currentCommand == COMMAND_PROVIDER) {
            // Provider command needs secondary selection (e.g., applications for open-with)
            int ecount;
            FfonElement **earr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                              &appRenderer->currentId, &ecount);
            if (earr && ecount > 0) {
                int eidx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                if (eidx >= 0 && eidx < ecount) {
                    const char *elementKey = (earr[eidx]->type == FFON_STRING) ?
                        earr[eidx]->data.string : earr[eidx]->data.object->key;

                    int itemCount = 0;
                    ProviderListItem *items = providerGetCommandListItems(elementKey,
                        appRenderer->providerCommandName, &itemCount);
                    if (items && itemCount > 0) {
                        appRenderer->totalListCurrentLayer = calloc(itemCount, sizeof(ListItem));
                        if (!appRenderer->totalListCurrentLayer) {
                            providerFreeCommandListItems(items, itemCount);
                            return;
                        }
                        for (int i = 0; i < itemCount; i++) {
                            appRenderer->totalListCurrentLayer[i].id.depth = 1;
                            appRenderer->totalListCurrentLayer[i].id.ids[0] = i;
                            appRenderer->totalListCurrentLayer[i].label = strdup(items[i].label);
                            appRenderer->totalListCurrentLayer[i].data = items[i].data ? strdup(items[i].data) : NULL;
                            appRenderer->totalListCount++;
                        }
                        providerFreeCommandListItems(items, itemCount);
                    }
                }
            }
        } else {
            // List available commands: app commands + provider commands
            const char *appCommands[] = {"editor mode", "operator mode"};
            int numAppCommands = 2;

            // Get provider commands for current element
            int numProviderCommands = 0;
            const char **providerCmds = NULL;
            int ecount;
            FfonElement **earr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                              &appRenderer->currentId, &ecount);
            if (earr && ecount > 0) {
                int eidx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                if (eidx >= 0 && eidx < ecount) {
                    const char *elementKey = (earr[eidx]->type == FFON_STRING) ?
                        earr[eidx]->data.string : earr[eidx]->data.object->key;
                    providerCmds = providerGetCommands(elementKey, &numProviderCommands);
                }
            }

            int numCommands = numAppCommands + numProviderCommands;
            appRenderer->totalListCurrentLayer = calloc(numCommands, sizeof(ListItem));
            if (!appRenderer->totalListCurrentLayer) return;

            int idx = 0;
            for (int i = 0; i < numAppCommands; i++) {
                appRenderer->totalListCurrentLayer[idx].id.depth = 1;
                appRenderer->totalListCurrentLayer[idx].id.ids[0] = idx;
                appRenderer->totalListCurrentLayer[idx].label = strdup(appCommands[i]);
                appRenderer->totalListCount++;
                idx++;
            }
            for (int i = 0; i < numProviderCommands; i++) {
                appRenderer->totalListCurrentLayer[idx].id.depth = 1;
                appRenderer->totalListCurrentLayer[idx].id.ids[0] = idx;
                appRenderer->totalListCurrentLayer[idx].label = strdup(providerCmds[i]);
                appRenderer->totalListCount++;
                idx++;
            }
        }
    }
}

void populateListCurrentLayer(AppRenderer *appRenderer, const char *searchString) {
    if (!searchString || strlen(searchString) == 0) {
        // No search, use all items
        if (appRenderer->filteredListCurrentLayer) {
            free(appRenderer->filteredListCurrentLayer);
        }
        appRenderer->filteredListCurrentLayer = NULL;
        appRenderer->filteredListCount = 0;
        appRenderer->listIndex = 0;
        return;
    }

    // Simple substring search
    if (appRenderer->filteredListCurrentLayer) {
        free(appRenderer->filteredListCurrentLayer);
    }

    appRenderer->filteredListCurrentLayer = calloc(appRenderer->totalListCount, sizeof(ListItem));
    if (!appRenderer->filteredListCurrentLayer) return;

    appRenderer->filteredListCount = 0;

    for (int i = 0; i < appRenderer->totalListCount; i++) {
        if (utf8_stristr(appRenderer->totalListCurrentLayer[i].label, searchString) != NULL) {
            idArrayCopy(&appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].id,
                         &appRenderer->totalListCurrentLayer[i].id);
            appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].label =
                appRenderer->totalListCurrentLayer[i].label; // Share pointer
            appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].data =
                appRenderer->totalListCurrentLayer[i].data; // Share pointer
            appRenderer->filteredListCount++;
        }
    }

    // Reset list index
    if (appRenderer->listIndex >= appRenderer->filteredListCount) {
        appRenderer->listIndex = appRenderer->filteredListCount > 0 ? appRenderer->filteredListCount - 1 : 0;
    }
}

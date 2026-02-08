#include "view.h"
#include "provider.h"
#include <platform.h>
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
        if (appRenderer->currentCommand == COMMAND_OPEN_WITH) {
            // List installed applications
            int appCount = 0;
            PlatformApplication *apps = platformGetApplications(&appCount);
            if (!apps || appCount == 0) {
                platformFreeApplications(apps, appCount);
                return;
            }

            appRenderer->totalListCurrentLayer = calloc(appCount, sizeof(ListItem));
            if (!appRenderer->totalListCurrentLayer) {
                platformFreeApplications(apps, appCount);
                return;
            }

            for (int i = 0; i < appCount; i++) {
                appRenderer->totalListCurrentLayer[i].id.depth = 1;
                appRenderer->totalListCurrentLayer[i].id.ids[0] = i;
                appRenderer->totalListCurrentLayer[i].label = strdup(apps[i].name);
                appRenderer->totalListCurrentLayer[i].data = strdup(apps[i].exec);
                appRenderer->totalListCount++;
            }

            platformFreeApplications(apps, appCount);
        } else {
            // List available commands
            const char *commands[] = {
                "editor mode",
                "operator mode",
                "create directory",
                "create file",
                "open file with"
            };
            int numCommands = sizeof(commands) / sizeof(commands[0]);

            appRenderer->totalListCurrentLayer = calloc(numCommands, sizeof(ListItem));
            if (!appRenderer->totalListCurrentLayer) return;

            for (int i = 0; i < numCommands; i++) {
                appRenderer->totalListCurrentLayer[i].id.depth = 1;
                appRenderer->totalListCurrentLayer[i].id.ids[0] = i;
                appRenderer->totalListCurrentLayer[i].label = strdup(commands[i]);
                appRenderer->totalListCount++;
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
        if (strstr(appRenderer->totalListCurrentLayer[i].label, searchString) != NULL) {
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

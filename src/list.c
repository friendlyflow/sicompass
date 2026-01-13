#include "view.h"
#include <stdlib.h>
#include <string.h>

void clearListAuxilaries(AppRenderer *appRenderer) {
    if (appRenderer->totalListAuxilaries) {
        for (int i = 0; i < appRenderer->totalListCount; i++) {
            free(appRenderer->totalListAuxilaries[i].value);
        }
        free(appRenderer->totalListAuxilaries);
        appRenderer->totalListAuxilaries = NULL;
        appRenderer->totalListCount = 0;
    }

    if (appRenderer->filteredListAuxilaries) {
        // Don't free values, they're shared with totalListAuxilaries
        free(appRenderer->filteredListAuxilaries);
        appRenderer->filteredListAuxilaries = NULL;
        appRenderer->filteredListCount = 0;
    }

    appRenderer->listIndex = 0;
}

void createListAuxilaries(AppRenderer *appRenderer) {
    clearListAuxilaries(appRenderer);

    if (appRenderer->currentCoordinate == COORDINATE_LIST) {
        // List all elements in current layer
        int count;
        FfonElement **arr = getFfonAtId(appRenderer, &appRenderer->currentId, &count);
        if (!arr) return;

        appRenderer->totalListAuxilaries = calloc(count, sizeof(ListItem));
        if (!appRenderer->totalListAuxilaries) return;

        IdArray thisId;
        idArrayCopy(&thisId, &appRenderer->currentId);
        thisId.ids[thisId.depth - 1] = 0;

        for (int i = 0; i < count; i++) {
            FfonElement *elem = arr[i];

            idArrayCopy(&appRenderer->totalListAuxilaries[appRenderer->totalListCount].id, &thisId);

            if (elem->type == FFON_STRING) {
                appRenderer->totalListAuxilaries[appRenderer->totalListCount].value =
                    strdup(elem->data.string);
            } else {
                appRenderer->totalListAuxilaries[appRenderer->totalListCount].value =
                    strdup(elem->data.object->key);
            }

            appRenderer->totalListCount++;
            thisId.ids[thisId.depth - 1]++;
        }

    } else if (appRenderer->currentCoordinate == COORDINATE_COMMAND) {
        // List available commands
        const char *commands[] = {
            "editor mode",
            "operator mode"
        };
        int numCommands = sizeof(commands) / sizeof(commands[0]);

        appRenderer->totalListAuxilaries = calloc(numCommands, sizeof(ListItem));
        if (!appRenderer->totalListAuxilaries) return;

        for (int i = 0; i < numCommands; i++) {
            appRenderer->totalListAuxilaries[i].id.depth = 1;
            appRenderer->totalListAuxilaries[i].id.ids[0] = i;
            appRenderer->totalListAuxilaries[i].value = strdup(commands[i]);
            appRenderer->totalListCount++;
        }
    }
}

void populateListAuxilaries(AppRenderer *appRenderer, const char *searchString) {
    if (!searchString || strlen(searchString) == 0) {
        // No search, use all items
        if (appRenderer->filteredListAuxilaries) {
            free(appRenderer->filteredListAuxilaries);
        }
        appRenderer->filteredListAuxilaries = NULL;
        appRenderer->filteredListCount = 0;
        appRenderer->listIndex = 0;
        return;
    }

    // Simple substring search
    if (appRenderer->filteredListAuxilaries) {
        free(appRenderer->filteredListAuxilaries);
    }

    appRenderer->filteredListAuxilaries = calloc(appRenderer->totalListCount, sizeof(ListItem));
    if (!appRenderer->filteredListAuxilaries) return;

    appRenderer->filteredListCount = 0;

    for (int i = 0; i < appRenderer->totalListCount; i++) {
        if (strstr(appRenderer->totalListAuxilaries[i].value, searchString) != NULL) {
            idArrayCopy(&appRenderer->filteredListAuxilaries[appRenderer->filteredListCount].id,
                         &appRenderer->totalListAuxilaries[i].id);
            appRenderer->filteredListAuxilaries[appRenderer->filteredListCount].value =
                appRenderer->totalListAuxilaries[i].value; // Share pointer
            appRenderer->filteredListCount++;
        }
    }

    // Reset list index
    if (appRenderer->listIndex >= appRenderer->filteredListCount) {
        appRenderer->listIndex = appRenderer->filteredListCount > 0 ? appRenderer->filteredListCount - 1 : 0;
    }
}

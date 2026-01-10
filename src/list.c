#include "view.h"
#include <stdlib.h>
#include <string.h>

void clearListRight(AppRenderer *appRenderer) {
    if (appRenderer->totalListRight) {
        for (int i = 0; i < appRenderer->totalListCount; i++) {
            free(appRenderer->totalListRight[i].value);
        }
        free(appRenderer->totalListRight);
        appRenderer->totalListRight = NULL;
        appRenderer->totalListCount = 0;
    }

    if (appRenderer->filteredListRight) {
        // Don't free values, they're shared with totalListRight
        free(appRenderer->filteredListRight);
        appRenderer->filteredListRight = NULL;
        appRenderer->filteredListCount = 0;
    }

    appRenderer->listIndex = 0;
}

void createListRight(AppRenderer *appRenderer) {
    clearListRight(appRenderer);

    if (appRenderer->currentCoordinate == COORDINATE_RIGHT_INFO) {
        // List all elements in current layer
        int count;
        FfonElement **arr = getFfonAtId(appRenderer, &appRenderer->currentId, &count);
        if (!arr) return;

        appRenderer->totalListRight = calloc(count, sizeof(ListItem));
        if (!appRenderer->totalListRight) return;

        IdArray thisId;
        idArrayCopy(&thisId, &appRenderer->currentId);
        thisId.ids[thisId.depth - 1] = 0;

        for (int i = 0; i < count; i++) {
            FfonElement *elem = arr[i];

            idArrayCopy(&appRenderer->totalListRight[appRenderer->totalListCount].id, &thisId);

            if (elem->type == FFON_STRING) {
                appRenderer->totalListRight[appRenderer->totalListCount].value =
                    strdup(elem->data.string);
            } else {
                appRenderer->totalListRight[appRenderer->totalListCount].value =
                    strdup(elem->data.object->key);
            }

            appRenderer->totalListCount++;
            thisId.ids[thisId.depth - 1]++;
        }

    } else if (appRenderer->currentCoordinate == COORDINATE_RIGHT_COMMAND) {
        // List available commands
        const char *commands[] = {
            "editor mode",
            "visitor mode"
        };
        int numCommands = sizeof(commands) / sizeof(commands[0]);

        appRenderer->totalListRight = calloc(numCommands, sizeof(ListItem));
        if (!appRenderer->totalListRight) return;

        for (int i = 0; i < numCommands; i++) {
            appRenderer->totalListRight[i].id.depth = 1;
            appRenderer->totalListRight[i].id.ids[0] = i;
            appRenderer->totalListRight[i].value = strdup(commands[i]);
            appRenderer->totalListCount++;
        }
    }
}

void populateListRight(AppRenderer *appRenderer, const char *searchString) {
    if (!searchString || strlen(searchString) == 0) {
        // No filter, use all items
        if (appRenderer->filteredListRight) {
            free(appRenderer->filteredListRight);
        }
        appRenderer->filteredListRight = NULL;
        appRenderer->filteredListCount = 0;
        appRenderer->listIndex = 0;
        return;
    }

    // Simple substring search
    if (appRenderer->filteredListRight) {
        free(appRenderer->filteredListRight);
    }

    appRenderer->filteredListRight = calloc(appRenderer->totalListCount, sizeof(ListItem));
    if (!appRenderer->filteredListRight) return;

    appRenderer->filteredListCount = 0;

    for (int i = 0; i < appRenderer->totalListCount; i++) {
        if (strstr(appRenderer->totalListRight[i].value, searchString) != NULL) {
            idArrayCopy(&appRenderer->filteredListRight[appRenderer->filteredListCount].id,
                         &appRenderer->totalListRight[i].id);
            appRenderer->filteredListRight[appRenderer->filteredListCount].value =
                appRenderer->totalListRight[i].value; // Share pointer
            appRenderer->filteredListCount++;
        }
    }

    // Reset list index
    if (appRenderer->listIndex >= appRenderer->filteredListCount) {
        appRenderer->listIndex = appRenderer->filteredListCount > 0 ? appRenderer->filteredListCount - 1 : 0;
    }
}

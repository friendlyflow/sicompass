#include "view.h"
#include <stdlib.h>
#include <string.h>

void clearListRight(EditorState *state) {
    if (state->totalListRight) {
        for (int i = 0; i < state->totalListCount; i++) {
            free(state->totalListRight[i].value);
        }
        free(state->totalListRight);
        state->totalListRight = NULL;
        state->totalListCount = 0;
    }

    if (state->filteredListRight) {
        // Don't free values, they're shared with totalListRight
        free(state->filteredListRight);
        state->filteredListRight = NULL;
        state->filteredListCount = 0;
    }

    state->listIndex = 0;
}

void createListRight(EditorState *state) {
    clearListRight(state);

    if (state->currentCoordinate == COORDINATE_RIGHT_INFO) {
        // List all elements in current layer
        int count;
        SfonElement **arr = getSfonAtId(state, &state->currentId, &count);
        if (!arr) return;

        state->totalListRight = calloc(count, sizeof(ListItem));
        if (!state->totalListRight) return;

        IdArray thisId;
        idArrayCopy(&thisId, &state->currentId);
        thisId.ids[thisId.depth - 1] = 0;

        for (int i = 0; i < count; i++) {
            SfonElement *elem = arr[i];

            idArrayCopy(&state->totalListRight[state->totalListCount].id, &thisId);

            if (elem->type == SFON_STRING) {
                state->totalListRight[state->totalListCount].value =
                    strdup(elem->data.string);
            } else {
                state->totalListRight[state->totalListCount].value =
                    strdup(elem->data.object->key);
            }

            state->totalListCount++;
            thisId.ids[thisId.depth - 1]++;
        }

    } else if (state->currentCoordinate == COORDINATE_RIGHT_COMMAND) {
        // List available commands
        const char *commands[] = {
            "editor mode",
            "visitor mode"
        };
        int numCommands = sizeof(commands) / sizeof(commands[0]);

        state->totalListRight = calloc(numCommands, sizeof(ListItem));
        if (!state->totalListRight) return;

        for (int i = 0; i < numCommands; i++) {
            state->totalListRight[i].id.depth = 1;
            state->totalListRight[i].id.ids[0] = i;
            state->totalListRight[i].value = strdup(commands[i]);
            state->totalListCount++;
        }
    }
}

void populateListRight(EditorState *state, const char *searchString) {
    if (!searchString || strlen(searchString) == 0) {
        // No filter, use all items
        if (state->filteredListRight) {
            free(state->filteredListRight);
        }
        state->filteredListRight = NULL;
        state->filteredListCount = 0;
        state->listIndex = 0;
        return;
    }

    // Simple substring search
    if (state->filteredListRight) {
        free(state->filteredListRight);
    }

    state->filteredListRight = calloc(state->totalListCount, sizeof(ListItem));
    if (!state->filteredListRight) return;

    state->filteredListCount = 0;

    for (int i = 0; i < state->totalListCount; i++) {
        if (strstr(state->totalListRight[i].value, searchString) != NULL) {
            idArrayCopy(&state->filteredListRight[state->filteredListCount].id,
                         &state->totalListRight[i].id);
            state->filteredListRight[state->filteredListCount].value =
                state->totalListRight[i].value; // Share pointer
            state->filteredListCount++;
        }
    }

    // Reset list index
    if (state->listIndex >= state->filteredListCount) {
        state->listIndex = state->filteredListCount > 0 ? state->filteredListCount - 1 : 0;
    }
}

#include "view.h"
#include <stdlib.h>
#include <string.h>

SiCompassApplication* appRendererCreate(SiCompassApplication* app) {
    AppRenderer *appRenderer = calloc(1, sizeof(AppRenderer));
    if (!appRenderer) return NULL;

    // Initialize FFON array
    appRenderer->ffonCapacity = 100;
    appRenderer->ffon = calloc(appRenderer->ffonCapacity, sizeof(FfonElement*));
    if (!appRenderer->ffon) {
        free(appRenderer);
        return NULL;
    }

    // Initialize input buffer
    appRenderer->inputBufferCapacity = 1024;
    appRenderer->inputBuffer = calloc(appRenderer->inputBufferCapacity, sizeof(char));
    if (!appRenderer->inputBuffer) {
        free(appRenderer->ffon);
        free(appRenderer);
        return NULL;
    }

    // Initialize undo history
    appRenderer->undoHistory = calloc(UNDO_HISTORY_SIZE, sizeof(UndoEntry));
    if (!appRenderer->undoHistory) {
        free(appRenderer->inputBuffer);
        free(appRenderer->ffon);
        free(appRenderer);
        return NULL;
    }

    // Initialize ID arrays
    idArrayInit(&appRenderer->currentId);
    idArrayInit(&appRenderer->previousId);
    idArrayInit(&appRenderer->currentInsertId);

    // Initialize caret
    appRenderer->caretState = caretCreate();
    if (!appRenderer->caretState) {
        free(appRenderer->undoHistory);
        free(appRenderer->inputBuffer);
        free(appRenderer->ffon);
        free(appRenderer);
        return NULL;
    }

    appRenderer->selectionAnchor = -1;
    appRenderer->running = true;
    appRenderer->needsRedraw = true;

    // Initialize AccessKit adapter (internal pointer will be set by accesskitInit)
    appRenderer->accesskitAdapter.adapter = NULL;

    // Initialize window state for thread-safe accessibility
    windowStateInit(&appRenderer->state, 0, appRenderer);  // ACCESSKIT_ROOT_ID = 0

    app->appRenderer = appRenderer;

    // Initialize AccessKit
    accesskitInit(app);

    return app;
}

void appRendererDestroy(AppRenderer *appRenderer) {
    if (!appRenderer) return;

    // Free FFON elements
    for (int i = 0; i < appRenderer->ffonCount; i++) {
        ffonElementDestroy(appRenderer->ffon[i]);
    }
    free(appRenderer->ffon);

    // Free input buffer
    free(appRenderer->inputBuffer);

    // Free undo history
    for (int i = 0; i < appRenderer->undoHistoryCount; i++) {
        if (appRenderer->undoHistory[i].prevElement) {
            ffonElementDestroy(appRenderer->undoHistory[i].prevElement);
        }
        if (appRenderer->undoHistory[i].newElement) {
            ffonElementDestroy(appRenderer->undoHistory[i].newElement);
        }
    }
    free(appRenderer->undoHistory);

    // Free clipboard
    if (appRenderer->clipboard) {
        ffonElementDestroy(appRenderer->clipboard);
    }

    // Free caret
    caretDestroy(appRenderer->caretState);

    // Free list items
    clearListCurrentLayer(appRenderer);

    // Free AccessKit adapter
    accesskitDestroy(appRenderer);

    free(appRenderer);
}

const char* coordinateToString(Coordinate coord) {
    switch (coord) {
        case COORDINATE_OPERATOR_GENERAL: return "operator mode";
        case COORDINATE_OPERATOR_INSERT: return "operator insert";
        case COORDINATE_EDITOR_GENERAL: return "editor mode";
        case COORDINATE_EDITOR_INSERT: return "editor insert";
        case COORDINATE_EDITOR_NORMAL: return "editor normal";
        case COORDINATE_EDITOR_VISUAL: return "editor visual";
        case COORDINATE_SIMPLE_SEARCH: return "search";
        case COORDINATE_COMMAND: return "run command";
        case COORDINATE_EXTENDED_SEARCH: return "ext search";
        default: return "unknown";
    }
}

const char* taskToString(Task task) {
    switch (task) {
        case TASK_NONE: return "none";
        case TASK_INPUT: return "input";
        case TASK_APPEND: return "append";
        case TASK_APPEND_APPEND: return "append append";
        case TASK_INSERT: return "insert";
        case TASK_INSERT_INSERT: return "insert insert";
        case TASK_DELETE: return "delete";
        case TASK_K_ARROW_UP: return "up";
        case TASK_J_ARROW_DOWN: return "down";
        case TASK_H_ARROW_LEFT: return "left";
        case TASK_L_ARROW_RIGHT: return "right";
        case TASK_CUT: return "cut";
        case TASK_COPY: return "copy";
        case TASK_PASTE: return "paste";
        default: return "unknown";
    }
}

bool isLineKey(const char *line) {
    if (!line) return false;
    size_t len = strlen(line);
    return len > 0 && line[len - 1] == ':';
}

void setErrorMessage(AppRenderer *appRenderer, const char *message) {
    snprintf(appRenderer->errorMessage, sizeof(appRenderer->errorMessage), "%s", message);
}

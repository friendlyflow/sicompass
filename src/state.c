#include "view.h"
#include <stdlib.h>
#include <string.h>

SiCompassApplication* appRendererCreate(void) {
    // TODO dat was hier AppRenderer!!! misschien niet goed nu!
    SiCompassApplication *app = calloc(1, sizeof(SiCompassApplication));
    if (!app) return NULL;

    // Initialize FFON array
    app->appRenderer->ffonCapacity = 100;
    app->appRenderer->ffon = calloc(app->appRenderer->ffonCapacity, sizeof(FfonElement*));
    if (!app->appRenderer->ffon) {
        free(app->appRenderer);
        return NULL;
    }

    // Initialize input buffer
    app->appRenderer->inputBufferCapacity = 1024;
    app->appRenderer->inputBuffer = calloc(app->appRenderer->inputBufferCapacity, sizeof(char));
    if (!app->appRenderer->inputBuffer) {
        free(app->appRenderer->ffon);
        free(app->appRenderer);
        return NULL;
    }

    // Initialize undo history
    app->appRenderer->undoHistory = calloc(UNDO_HISTORY_SIZE, sizeof(UndoEntry));
    if (!app->appRenderer->undoHistory) {
        free(app->appRenderer->inputBuffer);
        free(app->appRenderer->ffon);
        free(app->appRenderer);
        return NULL;
    }

    // Initialize ID arrays
    idArrayInit(&app->appRenderer->currentId);
    idArrayInit(&app->appRenderer->previousId);
    idArrayInit(&app->appRenderer->currentInsertId);

    app->appRenderer->running = true;
    app->appRenderer->needsRedraw = true;

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
        free(appRenderer->undoHistory[i].line);
    }
    free(appRenderer->undoHistory);

    // Free clipboard
    if (appRenderer->clipboard) {
        ffonElementDestroy(appRenderer->clipboard);
    }

    // Free list items
    clearListRight(appRenderer);

    free(appRenderer);
}

void idArrayInit(IdArray *arr) {
    arr->depth = 0;
    memset(arr->ids, 0, sizeof(arr->ids));
}

void idArrayCopy(IdArray *dst, const IdArray *src) {
    dst->depth = src->depth;
    memcpy(dst->ids, src->ids, sizeof(int) * src->depth);
}

bool idArrayEqual(const IdArray *a, const IdArray *b) {
    if (a->depth != b->depth) return false;
    return memcmp(a->ids, b->ids, sizeof(int) * a->depth) == 0;
}

void idArrayPush(IdArray *arr, int val) {
    if (arr->depth < MAX_ID_DEPTH) {
        arr->ids[arr->depth++] = val;
    }
}

int idArrayPop(IdArray *arr) {
    if (arr->depth > 0) {
        return arr->ids[--arr->depth];
    }
    return -1;
}

char* idArrayToString(const IdArray *arr) {
    static char buffer[MAX_ID_DEPTH * 16];
    buffer[0] = '\0';

    for (int i = 0; i < arr->depth; i++) {
        if (i > 0) strcat(buffer, ",");
        char num[16];
        snprintf(num, sizeof(num), "%d", arr->ids[i]);
        strcat(buffer, num);
    }

    return buffer;
}

const char* coordinateToString(Coordinate coord) {
    switch (coord) {
        case COORDINATE_LEFT_VISITOR_GENERAL: return "visitor mode";
        case COORDINATE_LEFT_VISITOR_INSERT: return "visitor insert mode";
        case COORDINATE_LEFT_EDITOR_GENERAL: return "editor mode";
        case COORDINATE_LEFT_EDITOR_INSERT: return "editor insert mode";
        case COORDINATE_LEFT_EDITOR_NORMAL: return "normal mode";
        case COORDINATE_LEFT_EDITOR_VISUAL: return "visual mode";
        case COORDINATE_RIGHT_INFO: return "info mode";
        case COORDINATE_RIGHT_COMMAND: return "command mode";
        case COORDINATE_RIGHT_FIND: return "find mode";
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

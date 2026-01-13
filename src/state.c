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

    appRenderer->running = true;
    appRenderer->needsRedraw = true;

    app->appRenderer = appRenderer;

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

    // Free caret
    caretDestroy(appRenderer->caretState);

    // Free list items
    clearListAuxilaries(appRenderer);

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
        case COORDINATE_OPERATOR_GENERAL: return "op";
        case COORDINATE_OPERATOR_INSERT: return "op i";
        case COORDINATE_EDITOR_GENERAL: return "edit";
        case COORDINATE_EDITOR_INSERT: return "edit i";
        case COORDINATE_EDITOR_NORMAL: return "edit n";
        case COORDINATE_EDITOR_VISUAL: return "edit v";
        case COORDINATE_LIST: return "list";
        case COORDINATE_COMMAND: return "command";
        case COORDINATE_FIND: return "find";
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

#include "view.h"
#include <stdlib.h>
#include <string.h>

EditorState* editorStateCreate(void) {
    EditorState *state = calloc(1, sizeof(EditorState));
    if (!state) return NULL;

    // Initialize FFON array
    state->ffonCapacity = 100;
    state->ffon = calloc(state->ffonCapacity, sizeof(FfonElement*));
    if (!state->ffon) {
        free(state);
        return NULL;
    }

    // Initialize input buffer
    state->inputBufferCapacity = 1024;
    state->inputBuffer = calloc(state->inputBufferCapacity, sizeof(char));
    if (!state->inputBuffer) {
        free(state->ffon);
        free(state);
        return NULL;
    }

    // Initialize undo history
    state->undoHistory = calloc(UNDO_HISTORY_SIZE, sizeof(UndoEntry));
    if (!state->undoHistory) {
        free(state->inputBuffer);
        free(state->ffon);
        free(state);
        return NULL;
    }

    // Initialize ID arrays
    idArrayInit(&state->currentId);
    idArrayInit(&state->previousId);
    idArrayInit(&state->currentInsertId);

    state->running = true;
    state->needsRedraw = true;

    return state;
}

void editorStateDestroy(EditorState *state) {
    if (!state) return;

    // Free FFON elements
    for (int i = 0; i < state->ffonCount; i++) {
        ffonElementDestroy(state->ffon[i]);
    }
    free(state->ffon);

    // Free input buffer
    free(state->inputBuffer);

    // Free undo history
    for (int i = 0; i < state->undoHistoryCount; i++) {
        free(state->undoHistory[i].line);
    }
    free(state->undoHistory);

    // Free clipboard
    if (state->clipboard) {
        ffonElementDestroy(state->clipboard);
    }

    // Free list items
    clearListRight(state);

    free(state);
}

bool initSdl(EditorState *state) {
    if (!SDL_Init(SDL_INIT_VIDEO)) {
        fprintf(stderr, "SDL_Init failed: %s\n", SDL_GetError());
        return false;
    }

    if (!TTF_Init()) {
        fprintf(stderr, "TTF_Init failed: %s\n", SDL_GetError());
        SDL_Quit();
        return false;
    }

    // Create window
    state->window = SDL_CreateWindow(
        "FFON Editor",
        1280, 720,
        SDL_WINDOW_RESIZABLE
    );
    if (!state->window) {
        fprintf(stderr, "SDL_CreateWindow failed: %s\n", SDL_GetError());
        TTF_Quit();
        SDL_Quit();
        return false;
    }

    // Create renderer
    state->renderer = SDL_CreateRenderer(state->window, NULL);
    if (!state->renderer) {
        fprintf(stderr, "SDL_CreateRenderer failed: %s\n", SDL_GetError());
        SDL_DestroyWindow(state->window);
        TTF_Quit();
        SDL_Quit();
        return false;
    }

    // Load font (try common monospace fonts)
    const char *fontPaths[] = {
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/System/Library/Fonts/Monaco.dfont",
        "C:\\Windows\\Fonts\\consola.ttf",
        NULL
    };

    int fontSize = 16;
    for (int i = 0; fontPaths[i] != NULL; i++) {
        state->font = TTF_OpenFont(fontPaths[i], fontSize);
        if (state->font) break;
    }

    if (!state->font) {
        fprintf(stderr, "Failed to load font\n");
        SDL_DestroyRenderer(state->renderer);
        SDL_DestroyWindow(state->window);
        TTF_Quit();
        SDL_Quit();
        return false;
    }

    // Get font metrics
    state->fontHeight = TTF_GetFontHeight(state->font);

    // Measure character width (use 'M' as it's typically the widest)
    int w, h;
    if (TTF_GetStringSize(state->font, "M", 0, &w, &h)) {
        state->charWidth = w;
    } else {
        state->charWidth = fontSize / 2; // fallback
    }

    // Enable text input
    SDL_StartTextInput(state->window);

    return true;
}

void cleanupSdl(EditorState *state) {
    if (!state) return;

    SDL_StopTextInput(state->window);

    if (state->font) {
        TTF_CloseFont(state->font);
    }

    if (state->renderer) {
        SDL_DestroyRenderer(state->renderer);
    }

    if (state->window) {
        SDL_DestroyWindow(state->window);
    }

    TTF_Quit();
    SDL_Quit();
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

void setErrorMessage(EditorState *state, const char *message) {
    snprintf(state->errorMessage, sizeof(state->errorMessage), "%s", message);
}

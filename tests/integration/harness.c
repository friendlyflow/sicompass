#include "harness.h"
#include <provider_interface.h>
#include <settings_provider.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

// ============================================================
// Stubs for symbols from state.c (not linked)
// ============================================================

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
        case COORDINATE_SCROLL: return "scroll mode";
        case COORDINATE_SCROLL_SEARCH: return "scroll search";
        case COORDINATE_INPUT_SEARCH: return "input search";
        case COORDINATE_DASHBOARD: return "dashboard";
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

char* escapeHtmlToText(const char *html) {
    return html ? strdup(html) : NULL;
}

// ============================================================
// Stubs for symbols from caret.c (not linked)
// ============================================================

struct CaretState {
    bool visible;
    uint64_t lastBlinkTime;
    uint32_t blinkInterval;
};

CaretState* caretCreate(void) {
    CaretState *c = calloc(1, sizeof(CaretState));
    if (c) { c->visible = true; c->blinkInterval = 800; }
    return c;
}

void caretDestroy(CaretState *caret) { free(caret); }
void caretUpdate(CaretState *caret, uint64_t currentTime) { (void)caret; (void)currentTime; }
void caretReset(CaretState *caret, uint64_t currentTime) {
    if (caret) { caret->visible = true; caret->lastBlinkTime = currentTime; }
}

// ============================================================
// Stubs for accesskit (not linked)
// ============================================================

void accesskitInit(SiCompassApplication *app) { (void)app; }
void accesskitDestroy(AppRenderer *appRenderer) { (void)appRenderer; }
void accesskitSpeak(AppRenderer *appRenderer, const char *text) { (void)appRenderer; (void)text; }
void accesskitSpeakCurrentElement(AppRenderer *appRenderer) { (void)appRenderer; }
void accesskitSpeakModeChange(AppRenderer *appRenderer, const char *context) { (void)appRenderer; (void)context; }
void accesskitUpdateWindowFocus(AppRenderer *appRenderer, bool isFocused) { (void)appRenderer; (void)isFocused; }

void windowStateInit(struct windowState *state, accesskit_node_id initialFocus, AppRenderer *appRenderer) {
    if (state) { state->focus = initialFocus; state->appRenderer = appRenderer; state->mutex = NULL; }
}
void windowStateDestroy(struct windowState *state) { (void)state; }
void windowStateLock(struct windowState *state) { (void)state; }
void windowStateUnlock(struct windowState *state) { (void)state; }

// ============================================================
// Stubs for render.c constants
// ============================================================

const accesskit_node_id ELEMENT_ID = 1;
const Sint32 SET_FOCUS_MSG = 1;

// ============================================================
// Palette definitions (from view.c, needed by tests)
// ============================================================

const ColorPalette PALETTE_DARK = {
    .background      = 0x000000FF,
    .text            = 0xFFFFFFFF,
    .headerseparator = 0x333333FF,
    .selected        = 0x2D4A28FF,
    .extsearch       = 0x696969FF,
    .scrollsearch    = 0x264F78FF,
    .error           = 0xFF0000FF,
};

const ColorPalette PALETTE_LIGHT = {
    .background      = 0xFFFFFFFF,
    .text            = 0x000000FF,
    .headerseparator = 0xE0E0E0FF,
    .selected        = 0xC0ECB8FF,
    .extsearch       = 0x333333FF,
    .scrollsearch    = 0xA8C7FAFF,
    .error           = 0xFF0000FF,
};

// ============================================================
// SDL clipboard wraps (intercepted via --wrap linker flags)
// ============================================================

static char s_clipboard[4096] = {0};

char* __wrap_SDL_GetClipboardText(void) {
    return strdup(s_clipboard);
}

bool __wrap_SDL_SetClipboardText(const char *text) {
    if (text) snprintf(s_clipboard, sizeof(s_clipboard), "%s", text);
    else s_clipboard[0] = '\0';
    return true;
}

bool __wrap_SDL_HasClipboardText(void) {
    return s_clipboard[0] != '\0';
}

void __wrap_SDL_free(void *mem) {
    free(mem);
}

// ============================================================
// Platform stub (--wrap)
// ============================================================

bool __wrap_platformOpenWithDefault(const char *filePath) {
    (void)filePath;
    return true;
}

// ============================================================
// AppRenderer creation (headless)
// ============================================================

AppRenderer* harnessCreateAppRenderer(void) {
    AppRenderer *a = calloc(1, sizeof(AppRenderer));
    if (!a) return NULL;

    a->ffonCapacity = 16;
    a->ffon = calloc(a->ffonCapacity, sizeof(FfonElement*));
    a->providers = calloc(a->ffonCapacity, sizeof(Provider*));

    a->inputBufferCapacity = 4096;
    a->inputBuffer = calloc(a->inputBufferCapacity, sizeof(char));
    a->inputPrefix[0] = '\0';
    a->inputSuffix[0] = '\0';

    a->savedInputBufferCapacity = 1024;
    a->savedInputBuffer = calloc(a->savedInputBufferCapacity, sizeof(char));

    a->undoHistory = calloc(UNDO_HISTORY_SIZE, sizeof(UndoEntry));

    idArrayInit(&a->currentId);
    idArrayInit(&a->previousId);
    idArrayInit(&a->currentInsertId);
    idArrayInit(&a->saveAsReturnId);

    a->caretState = caretCreate();
    a->selectionAnchor = -1;
    a->running = true;
    a->needsRedraw = true;
    a->palette = &PALETTE_DARK;

    // Default window dimensions for scroll/page calculations
    a->windowHeight = 600;
    a->cachedLineHeight = 20;

    return a;
}

void harnessDestroyAppRenderer(AppRenderer *appRenderer) {
    if (!appRenderer) return;

    for (int i = 0; i < appRenderer->ffonCount; i++) {
        ffonElementDestroy(appRenderer->ffon[i]);
    }
    free(appRenderer->ffon);
    free(appRenderer->providers);
    free(appRenderer->inputBuffer);
    free(appRenderer->savedInputBuffer);

    for (int i = 0; i < appRenderer->undoHistoryCount; i++) {
        if (appRenderer->undoHistory[i].prevElement)
            ffonElementDestroy(appRenderer->undoHistory[i].prevElement);
        if (appRenderer->undoHistory[i].newElement)
            ffonElementDestroy(appRenderer->undoHistory[i].newElement);
    }
    free(appRenderer->undoHistory);

    if (appRenderer->clipboard)
        ffonElementDestroy(appRenderer->clipboard);

    caretDestroy(appRenderer->caretState);
    clearListCurrentLayer(appRenderer);
    free(appRenderer);
}

// ============================================================
// Provider setup
// ============================================================

static void noopApplySettings(const char *key, const char *value, void *userdata) {
    (void)key; (void)value; (void)userdata;
}

// ============================================================
// Mock "sales demo" provider (for save/load integration tests)
// ============================================================

static FfonElement** salesDemoFetch(const char *path, int *count) {
    (void)path;
    *count = 2;
    FfonElement **elems = malloc(2 * sizeof(FfonElement*));
    elems[0] = ffonElementCreateString("product A");
    elems[1] = ffonElementCreateString("product B");
    return elems;
}

static const ProviderOps salesDemoOps = {
    .name = "sales demo",
    .displayName = "sales demo",
    .fetch = salesDemoFetch,
};

bool harnessSetupProviders(AppRenderer *appRenderer, const char *fbTmpDir) {
    // Create file browser
    Provider *fb = providerFactoryCreate("file browser");
    if (!fb) return false;
    providerRegister(fb);

    // Create web browser
    Provider *wb = providerFactoryCreate("web browser");
    if (wb) providerRegister(wb);

    // Create mock "sales demo" provider with config file support
    Provider *salesDemo = providerCreate(&salesDemoOps);
    if (!salesDemo) return false;
    salesDemo->supportsConfigFiles = true;
    providerRegister(salesDemo);

    // Create settings with no-op callback
    Provider *settings = settingsProviderCreate(noopApplySettings, appRenderer);

    // Register settings last
    providerRegister(settings);

    // Init all providers
    providerInitAll();

    // Set file browser to the temp directory
    if (fb->setCurrentPath) {
        fb->setCurrentPath(fb, fbTmpDir);
    }

    // Build ffon/providers arrays
    int total = providerGetRegisteredCount();
    appRenderer->ffonCount = 0;
    for (int i = 0; i < total; i++) {
        Provider *p = providerGetRegisteredAt(i);
        FfonElement *elem = providerGetInitialElement(p);
        if (elem) {
            appRenderer->ffon[appRenderer->ffonCount] = elem;
            appRenderer->providers[appRenderer->ffonCount] = p;
            appRenderer->ffonCount++;
        }
    }

    if (appRenderer->ffonCount == 0) return false;

    // Start at first provider
    idArrayInit(&appRenderer->currentId);
    idArrayPush(&appRenderer->currentId, 0);
    appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    appRenderer->previousCoordinate = COORDINATE_OPERATOR_GENERAL;

    createListCurrentLayer(appRenderer);
    return true;
}

// ============================================================
// Key simulation
// ============================================================

void pressKey(AppRenderer *app, SDL_Keycode key, SDL_Keymod mod) {
    SDL_Event event = {0};
    event.type = SDL_EVENT_KEY_DOWN;
    event.key.key = key;
    event.key.mod = mod;
    handleKeys(app, &event);
}

void pressDown(AppRenderer *app)   { pressKey(app, SDLK_DOWN, 0); }
void pressUp(AppRenderer *app)     { pressKey(app, SDLK_UP, 0); }
void pressRight(AppRenderer *app)  { pressKey(app, SDLK_RIGHT, 0); }
void pressLeft(AppRenderer *app)   { pressKey(app, SDLK_LEFT, 0); }
void pressEnter(AppRenderer *app)  { pressKey(app, SDLK_RETURN, 0); }
void pressEscape(AppRenderer *app) { pressKey(app, SDLK_ESCAPE, 0); }
void pressTab(AppRenderer *app)    { pressKey(app, SDLK_TAB, 0); }

void pressCtrl(AppRenderer *app, SDL_Keycode key) {
    pressKey(app, key, SDL_KMOD_LCTRL);
}

void pressCtrlShift(AppRenderer *app, SDL_Keycode key) {
    pressKey(app, key, SDL_KMOD_LCTRL | SDL_KMOD_LSHIFT);
}

void typeText(AppRenderer *app, const char *text) {
    if (!text) return;
    // Feed each character individually through handleInput
    const char *p = text;
    while (*p) {
        // Find the end of the current UTF-8 character
        int len = 1;
        unsigned char c = (unsigned char)*p;
        if (c >= 0xC0 && c < 0xE0) len = 2;
        else if (c >= 0xE0 && c < 0xF0) len = 3;
        else if (c >= 0xF0) len = 4;

        char buf[5] = {0};
        for (int i = 0; i < len && p[i]; i++) buf[i] = p[i];
        handleInput(app, buf);
        p += len;
    }
}

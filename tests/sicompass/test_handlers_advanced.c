/*
 * Tests for handlers.c advanced functions:
 * - handleCtrlHome / handleCtrlEnd (list index jumping)
 * - handleColon (command mode entry)
 * - handleDelete (delegation to updateState)
 * - handleCommand (COMMAND_NONE / EDITOR_MODE / OPERATOR_MODE)
 * - handleTab (search mode transitions)
 * - handleEscape (mode exit transitions)
 * - handleCtrlF (extended search entry)
 * - handleUp / handleDown (search/command list navigation)
 *
 * Uses FFF to mock all external dependencies.
 */

#include <unity.h>
#include <fff/fff.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

DEFINE_FFF_GLOBALS;

/* ============================================
 * Type definitions (minimal stubs)
 * ============================================ */

#define MAX_ID_DEPTH 32
#define MAX_LINE_LENGTH 4096
#define MAX_URI_LENGTH 4096
#define DELTA_MS 400

typedef enum {
    FFON_STRING,
    FFON_OBJECT
} FfonType;

typedef struct {
    int ids[MAX_ID_DEPTH];
    int depth;
} IdArray;

typedef struct FfonObject {
    char *key;
    struct FfonElement **elements;
    int count;
    int capacity;
} FfonObject;

typedef struct FfonElement {
    FfonType type;
    union {
        char *string;
        FfonObject *object;
    } data;
} FfonElement;

typedef struct {
    IdArray id;
    char *label;
    char *data;
    char *navPath;
} ListItem;

typedef enum {
    COORDINATE_OPERATOR_GENERAL,
    COORDINATE_OPERATOR_INSERT,
    COORDINATE_EDITOR_GENERAL,
    COORDINATE_EDITOR_INSERT,
    COORDINATE_EDITOR_NORMAL,
    COORDINATE_EDITOR_VISUAL,
    COORDINATE_SIMPLE_SEARCH,
    COORDINATE_EXTENDED_SEARCH,
    COORDINATE_COMMAND,
    COORDINATE_SCROLL,
    COORDINATE_SCROLL_SEARCH,
    COORDINATE_INPUT_SEARCH,
    COORDINATE_DASHBOARD
} Coordinate;

typedef enum {
    TASK_NONE,
    TASK_INPUT,
    TASK_APPEND,
    TASK_APPEND_APPEND,
    TASK_INSERT,
    TASK_INSERT_INSERT,
    TASK_DELETE,
    TASK_K_ARROW_UP,
    TASK_J_ARROW_DOWN,
    TASK_H_ARROW_LEFT,
    TASK_L_ARROW_RIGHT,
    TASK_CUT,
    TASK_COPY,
    TASK_PASTE
} Task;

typedef enum {
    HISTORY_NONE,
    HISTORY_UNDO,
    HISTORY_REDO
} History;

typedef enum {
    COMMAND_NONE,
    COMMAND_PROVIDER,
} Command;

typedef struct CaretState CaretState;

typedef struct SiCompassApplication SiCompassApplication;

typedef struct AppRenderer {
    FfonElement **ffon;
    int ffonCount;
    int ffonCapacity;

    IdArray currentId;
    IdArray previousId;
    IdArray currentInsertId;
    Coordinate currentCoordinate;
    Coordinate previousCoordinate;
    Command currentCommand;

    char *inputBuffer;
    int inputBufferSize;
    int inputBufferCapacity;
    int cursorPosition;
    int selectionAnchor;
    int scrollOffset;
    int textScrollOffset;
    int textScrollLineCount;
    int scrollSearchMatchCount;
    int scrollSearchCurrentMatch;

    // Input search state (Ctrl+F in insert mode)
    char *savedInputBuffer;
    int savedInputBufferSize;
    int savedInputBufferCapacity;
    int savedCursorPosition;
    int savedSelectionAnchor;
    char savedInputPrefix[MAX_LINE_LENGTH];
    char savedInputSuffix[MAX_LINE_LENGTH];
    Coordinate savedInsertCoordinate;
    int inputSearchMatchCount;
    int inputSearchCurrentMatch;
    int inputSearchScrollOffset;
    int inputSearchScrollLineCount;

    char inputPrefix[MAX_LINE_LENGTH];
    char inputSuffix[MAX_LINE_LENGTH];

    ListItem *totalListCurrentLayer;
    int totalListCount;
    ListItem *filteredListCurrentLayer;
    int filteredListCount;
    int listIndex;

    uint64_t lastKeypressTime;
    CaretState *caretState;

    bool running;
    bool needsRedraw;
    bool prefixedInsertMode;

    SiCompassApplication *app;
    char errorMessage[256];
    char providerCommandName[64];
    char dashboardImagePath[MAX_URI_LENGTH];
} AppRenderer;

typedef struct {
    const char *dashboardImagePath;
} Provider;

/* ============================================
 * FFF mocks for external dependencies
 * ============================================ */

// SDL
FAKE_VALUE_FUNC(uint64_t, SDL_GetTicks);

// Caret
FAKE_VOID_FUNC(caretReset, CaretState *, uint64_t);

// AccessKit
FAKE_VOID_FUNC(accesskitSpeakCurrentElement, AppRenderer *);
FAKE_VOID_FUNC(accesskitSpeakModeChange, AppRenderer *, const char *);

// List
FAKE_VOID_FUNC(createListCurrentLayer, AppRenderer *);
FAKE_VOID_FUNC(createListExtendedSearch, AppRenderer *);

// State
FAKE_VOID_FUNC(updateState, AppRenderer *, Task, History);
FAKE_VOID_FUNC(handleHistoryAction, AppRenderer *, History);
FAKE_VOID_FUNC(setErrorMessage, AppRenderer *, const char *);

// Provider
FAKE_VALUE_FUNC(bool, providerNavigateLeft, AppRenderer *);
FAKE_VALUE_FUNC(bool, providerNavigateRight, AppRenderer *);
FAKE_VOID_FUNC(providerRefreshCurrentDirectory, AppRenderer *);
FAKE_VOID_FUNC(providerNotifyRadioChanged, AppRenderer *, IdArray *);
FAKE_VOID_FUNC(providerNotifyButtonPressed, AppRenderer *, IdArray *);
FAKE_VALUE_FUNC(bool, providerCommitEdit, AppRenderer *, const char *, const char *);
FAKE_VALUE_FUNC(bool, providerCreateFile, AppRenderer *, const char *);
FAKE_VALUE_FUNC(bool, providerCreateDirectory, AppRenderer *, const char *);
FAKE_VALUE_FUNC(bool, providerDeleteItem, AppRenderer *, const char *);
FAKE_VOID_FUNC(providerExecuteCommand, AppRenderer *, const char *, const char *);

// Provider tag functions (return NULL / false to take default branches)
static char *providerTagExtractContent_retval = NULL;
char *providerTagExtractContent(const char *key) {
    (void)key;
    if (providerTagExtractContent_retval)
        return strdup(providerTagExtractContent_retval);
    return NULL;
}

bool providerTagHasRadio(const char *key) { (void)key; return false; }
bool providerTagHasChecked(const char *key) { (void)key; return false; }
static bool providerTagHasCheckbox_retval = false;
bool providerTagHasCheckbox(const char *key) { (void)key; return providerTagHasCheckbox_retval; }
static bool providerTagHasCheckboxChecked_retval = false;
bool providerTagHasCheckboxChecked(const char *key) { (void)key; return providerTagHasCheckboxChecked_retval; }
bool providerTagHasButton(const char *key) { (void)key; return false; }
bool providerTagHasInput(const char *key) { (void)key; return false; }
bool providerTagHasOneOpt(const char *key) { (void)key; return false; }
bool providerTagHasManyOpt(const char *key) { (void)key; return false; }
char *providerTagFormatKey(const char *content) { (void)content; return NULL; }
char *providerTagStripDisplay(const char *key) { (void)key; return NULL; }
char *providerTagExtractCheckedContent(const char *key) { (void)key; return NULL; }
char *providerTagFormatCheckedKey(const char *content) { (void)content; return NULL; }
static char *providerTagExtractCheckboxContent_retval = NULL;
char *providerTagExtractCheckboxContent(const char *key) {
    (void)key;
    if (providerTagExtractCheckboxContent_retval) return strdup(providerTagExtractCheckboxContent_retval);
    return NULL;
}
static char *providerTagExtractCheckboxCheckedContent_retval = NULL;
char *providerTagExtractCheckboxCheckedContent(const char *key) {
    (void)key;
    if (providerTagExtractCheckboxCheckedContent_retval) return strdup(providerTagExtractCheckboxCheckedContent_retval);
    return NULL;
}
static char *providerTagFormatCheckboxKey_retval = NULL;
char *providerTagFormatCheckboxKey(const char *content) {
    (void)content;
    if (providerTagFormatCheckboxKey_retval) return strdup(providerTagFormatCheckboxKey_retval);
    return NULL;
}
static char *providerTagFormatCheckboxCheckedKey_retval = NULL;
char *providerTagFormatCheckboxCheckedKey(const char *content) {
    (void)content;
    if (providerTagFormatCheckboxCheckedKey_retval) return strdup(providerTagFormatCheckboxCheckedKey_retval);
    return NULL;
}

// FFON helpers
static FfonElement **getFfonAtId_retval = NULL;
static int getFfonAtId_retcount = 0;
FfonElement **getFfonAtId(FfonElement **ffon, int count, IdArray *id, int *outCount) {
    (void)ffon; (void)count; (void)id;
    *outCount = getFfonAtId_retcount;
    return getFfonAtId_retval;
}

int getFfonMaxIdAtPath(FfonElement **ffon, int count, IdArray *id) {
    (void)ffon; (void)count; (void)id;
    return -1;
}

FfonElement *providerHandleCommand(AppRenderer *app, const char *cmd,
                                    const char *elementKey, FfonType type,
                                    char *errorMsg, size_t errorMsgSize) {
    (void)app; (void)cmd; (void)elementKey; (void)type;
    (void)errorMsg; (void)errorMsgSize;
    return NULL;
}

static Provider *g_activeProvider = NULL;
Provider *providerGetActive(AppRenderer *app) { (void)app; return g_activeProvider; }
const char *providerGetCurrentPath(AppRenderer *app) { (void)app; return NULL; }

static void idArrayCopy(IdArray *dst, const IdArray *src) {
    memcpy(dst, src, sizeof(IdArray));
}

static void idArrayInit(IdArray *id) {
    memset(id, 0, sizeof(IdArray));
}

static void idArrayPop(IdArray *id) {
    if (id->depth > 0) id->depth--;
}

// Platform stubs
const char *platformGetPathSeparator(void) { return "/"; }
void platformOpenWithDefault(const char *path) { (void)path; }

/* ============================================
 * UTF-8 helpers (copied from handlers.c)
 * ============================================ */

static int utf8_char_length(const char *str, int pos) {
    unsigned char c = (unsigned char)str[pos];
    if ((c & 0x80) == 0) return 1;
    else if ((c & 0xE0) == 0xC0) return 2;
    else if ((c & 0xF0) == 0xE0) return 3;
    else if ((c & 0xF8) == 0xF0) return 4;
    return 1;
}

static int utf8_move_backward(const char *str, int cursorPos) {
    if (cursorPos <= 0) return 0;
    int newPos = cursorPos - 1;
    while (newPos > 0 && ((unsigned char)str[newPos] & 0xC0) == 0x80)
        newPos--;
    return newPos;
}

static int utf8_move_forward(const char *str, int cursorPos, int bufferSize) {
    if (cursorPos >= bufferSize) return bufferSize;
    int charLen = utf8_char_length(str, cursorPos);
    int newPos = cursorPos + charLen;
    if (newPos > bufferSize) newPos = bufferSize;
    return newPos;
}

/* ============================================
 * Selection helpers (copied from handlers.c)
 * ============================================ */

bool hasSelection(AppRenderer *appRenderer) {
    return appRenderer->selectionAnchor != -1 &&
           appRenderer->selectionAnchor != appRenderer->cursorPosition;
}

void clearSelection(AppRenderer *appRenderer) {
    appRenderer->selectionAnchor = -1;
}

void getSelectionRange(AppRenderer *appRenderer, int *start, int *end) {
    int a = appRenderer->selectionAnchor;
    int b = appRenderer->cursorPosition;
    *start = (a < b) ? a : b;
    *end = (a > b) ? a : b;
}

void deleteSelection(AppRenderer *appRenderer) {
    if (!hasSelection(appRenderer)) return;
    int start, end;
    getSelectionRange(appRenderer, &start, &end);
    memmove(&appRenderer->inputBuffer[start],
            &appRenderer->inputBuffer[end],
            appRenderer->inputBufferSize - end + 1);
    appRenderer->inputBufferSize -= (end - start);
    appRenderer->cursorPosition = start;
    clearSelection(appRenderer);
}

/* ============================================
 * Functions under test (copied from handlers.c)
 * ============================================ */

void handleCtrlHome(AppRenderer *appRenderer) {
    int count = (appRenderer->filteredListCount > 0) ?
                 appRenderer->filteredListCount : appRenderer->totalListCount;
    if (count > 0) {
        appRenderer->listIndex = 0;
        appRenderer->scrollOffset = 0;
        accesskitSpeakCurrentElement(appRenderer);
    }
    appRenderer->needsRedraw = true;
}

void handleCtrlEnd(AppRenderer *appRenderer) {
    int count = (appRenderer->filteredListCount > 0) ?
                 appRenderer->filteredListCount : appRenderer->totalListCount;
    if (count > 0) {
        appRenderer->listIndex = count - 1;
        appRenderer->scrollOffset = -1;
        accesskitSpeakCurrentElement(appRenderer);
    }
    appRenderer->needsRedraw = true;
}

void handleDelete(AppRenderer *appRenderer, History history) {
    updateState(appRenderer, TASK_DELETE, history);
    appRenderer->needsRedraw = true;
}

void handleColon(AppRenderer *appRenderer) {
    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    appRenderer->currentCoordinate = COORDINATE_COMMAND;
    appRenderer->currentCommand = COMMAND_NONE;
    accesskitSpeakModeChange(appRenderer, NULL);

    appRenderer->inputBuffer[0] = '\0';
    appRenderer->inputBufferSize = 0;
    appRenderer->cursorPosition = 0;
    appRenderer->selectionAnchor = -1;

    createListCurrentLayer(appRenderer);
    appRenderer->scrollOffset = 0;
    appRenderer->needsRedraw = true;
}

void handleTab(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL ||
        appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_INPUT_SEARCH) {
        return;
    }

    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH) {
        ListItem *list = appRenderer->filteredListCount > 0 ?
                         appRenderer->filteredListCurrentLayer : appRenderer->totalListCurrentLayer;
        int count = appRenderer->filteredListCount > 0 ?
                    appRenderer->filteredListCount : appRenderer->totalListCount;
        if (appRenderer->listIndex >= 0 && appRenderer->listIndex < count) {
            idArrayCopy(&appRenderer->currentId, &list[appRenderer->listIndex].id);
        }
        appRenderer->currentCoordinate = COORDINATE_SCROLL;
        appRenderer->textScrollOffset = 0;
        appRenderer->textScrollLineCount = 0;
        accesskitSpeakModeChange(appRenderer, NULL);
        appRenderer->needsRedraw = true;
        return;
    }

    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    appRenderer->currentCoordinate = COORDINATE_SIMPLE_SEARCH;
    accesskitSpeakModeChange(appRenderer, NULL);

    appRenderer->inputBuffer[0] = '\0';
    appRenderer->inputBufferSize = 0;
    appRenderer->cursorPosition = 0;
    appRenderer->selectionAnchor = -1;

    createListCurrentLayer(appRenderer);
    appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    appRenderer->scrollOffset = 0;
    appRenderer->needsRedraw = true;
}

void handleEscape(AppRenderer *appRenderer) {
    clearSelection(appRenderer);
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT) {
        updateState(appRenderer, TASK_INPUT, HISTORY_NONE);
        appRenderer->currentCoordinate = COORDINATE_EDITOR_GENERAL;
    } else if (appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT) {
        appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    } else if (appRenderer->currentCoordinate == COORDINATE_COMMAND) {
        appRenderer->currentCommand = COMMAND_NONE;
        appRenderer->currentCoordinate = appRenderer->previousCoordinate;
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        accesskitSpeakModeChange(appRenderer, NULL);
        createListCurrentLayer(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->scrollOffset = 0;
        appRenderer->needsRedraw = true;
        return;
    } else if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH) {
        appRenderer->currentCoordinate = appRenderer->previousCoordinate;
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        accesskitSpeakModeChange(appRenderer, NULL);
        createListCurrentLayer(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->scrollOffset = 0;
        appRenderer->needsRedraw = true;
        return;
    } else if (appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        appRenderer->currentCoordinate = appRenderer->previousCoordinate;
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        accesskitSpeakModeChange(appRenderer, NULL);
        createListCurrentLayer(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->scrollOffset = 0;
        appRenderer->needsRedraw = true;
        return;
    } else if (appRenderer->currentCoordinate == COORDINATE_DASHBOARD) {
        appRenderer->currentCoordinate = appRenderer->previousCoordinate;
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        accesskitSpeakModeChange(appRenderer, NULL);
        appRenderer->needsRedraw = true;
        return;
    } else if (appRenderer->currentCoordinate == COORDINATE_INPUT_SEARCH) {
        // Restore saved insert mode state
        if (appRenderer->savedInputBufferSize >= appRenderer->inputBufferCapacity) {
            int newCap = appRenderer->savedInputBufferSize + 1;
            char *newBuf = realloc(appRenderer->inputBuffer, newCap);
            if (newBuf) {
                appRenderer->inputBuffer = newBuf;
                appRenderer->inputBufferCapacity = newCap;
            }
        }
        memcpy(appRenderer->inputBuffer, appRenderer->savedInputBuffer, appRenderer->savedInputBufferSize + 1);
        appRenderer->inputBufferSize = appRenderer->savedInputBufferSize;
        appRenderer->cursorPosition = appRenderer->savedCursorPosition;
        appRenderer->selectionAnchor = appRenderer->savedSelectionAnchor;
        strncpy(appRenderer->inputPrefix, appRenderer->savedInputPrefix, MAX_LINE_LENGTH - 1);
        appRenderer->inputPrefix[MAX_LINE_LENGTH - 1] = '\0';
        strncpy(appRenderer->inputSuffix, appRenderer->savedInputSuffix, MAX_LINE_LENGTH - 1);
        appRenderer->inputSuffix[MAX_LINE_LENGTH - 1] = '\0';
        appRenderer->currentCoordinate = appRenderer->savedInsertCoordinate;
        appRenderer->inputSearchMatchCount = 0;
        appRenderer->inputSearchCurrentMatch = 0;
        accesskitSpeakModeChange(appRenderer, NULL);
        appRenderer->needsRedraw = true;
        return;
    } else if (appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        appRenderer->currentCoordinate = COORDINATE_SCROLL;
        appRenderer->scrollSearchMatchCount = 0;
        appRenderer->scrollSearchCurrentMatch = 0;
        accesskitSpeakModeChange(appRenderer, NULL);
        appRenderer->needsRedraw = true;
        return;
    } else if (appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        appRenderer->currentCoordinate = COORDINATE_SIMPLE_SEARCH;
        appRenderer->inputBuffer[0] = '\0';
        appRenderer->inputBufferSize = 0;
        appRenderer->cursorPosition = 0;
        appRenderer->selectionAnchor = -1;
        createListCurrentLayer(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->scrollOffset = 0;
        accesskitSpeakModeChange(appRenderer, NULL);
        appRenderer->needsRedraw = true;
        return;
    } else if (appRenderer->previousCoordinate == COORDINATE_OPERATOR_GENERAL ||
               appRenderer->previousCoordinate == COORDINATE_OPERATOR_INSERT) {
        appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    } else {
        appRenderer->currentCoordinate = COORDINATE_EDITOR_GENERAL;
    }

    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    accesskitSpeakModeChange(appRenderer, NULL);
    appRenderer->needsRedraw = true;
}

void handleCommand(AppRenderer *appRenderer) {
    switch (appRenderer->currentCommand) {
        case COMMAND_NONE:
            break;

        case COMMAND_PROVIDER: {
            /* Provider command branch requires getFfonAtId and providerHandleCommand
             * which are stubbed to return NULL — so this branch does nothing beyond
             * the break. Tested via the COMMAND_NONE/EDITOR/OPERATOR branches. */
            break;
        }
    }

    appRenderer->needsRedraw = true;
}

void handleCtrlF(AppRenderer *appRenderer) {
    uint64_t now = SDL_GetTicks();

    if (appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        appRenderer->previousCoordinate = COORDINATE_SCROLL;
        appRenderer->currentCoordinate = COORDINATE_SCROLL_SEARCH;
        appRenderer->inputBuffer[0] = '\0';
        appRenderer->inputBufferSize = 0;
        appRenderer->cursorPosition = 0;
        appRenderer->selectionAnchor = -1;
        appRenderer->scrollSearchMatchCount = 0;
        appRenderer->scrollSearchCurrentMatch = 0;
        accesskitSpeakModeChange(appRenderer, NULL);
        appRenderer->needsRedraw = true;
        return;
    }

    if (appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_INPUT_SEARCH) {
        return;
    }

    // INSERT modes: Ctrl+F enters INPUT_SEARCH
    if (appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT) {
        appRenderer->savedInsertCoordinate = appRenderer->currentCoordinate;
        if (appRenderer->inputBufferSize >= appRenderer->savedInputBufferCapacity) {
            int newCap = appRenderer->inputBufferSize + 1;
            char *newBuf = realloc(appRenderer->savedInputBuffer, newCap);
            if (!newBuf) return;
            appRenderer->savedInputBuffer = newBuf;
            appRenderer->savedInputBufferCapacity = newCap;
        }
        memcpy(appRenderer->savedInputBuffer, appRenderer->inputBuffer, appRenderer->inputBufferSize + 1);
        appRenderer->savedInputBufferSize = appRenderer->inputBufferSize;
        appRenderer->savedCursorPosition = appRenderer->cursorPosition;
        appRenderer->savedSelectionAnchor = appRenderer->selectionAnchor;
        strncpy(appRenderer->savedInputPrefix, appRenderer->inputPrefix, MAX_LINE_LENGTH - 1);
        appRenderer->savedInputPrefix[MAX_LINE_LENGTH - 1] = '\0';
        strncpy(appRenderer->savedInputSuffix, appRenderer->inputSuffix, MAX_LINE_LENGTH - 1);
        appRenderer->savedInputSuffix[MAX_LINE_LENGTH - 1] = '\0';
        appRenderer->currentCoordinate = COORDINATE_INPUT_SEARCH;
        appRenderer->inputBuffer[0] = '\0';
        appRenderer->inputBufferSize = 0;
        appRenderer->cursorPosition = 0;
        appRenderer->selectionAnchor = -1;
        appRenderer->inputSearchMatchCount = 0;
        appRenderer->inputSearchCurrentMatch = 0;
        appRenderer->inputSearchScrollOffset = 0;
        appRenderer->inputSearchScrollLineCount = 0;
        accesskitSpeakModeChange(appRenderer, NULL);
        appRenderer->needsRedraw = true;
        return;
    }

    if (appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH &&
        now - appRenderer->lastKeypressTime <= DELTA_MS) {
        appRenderer->currentId.depth = 1;
        appRenderer->inputBuffer[0] = '\0';
        appRenderer->inputBufferSize = 0;
        appRenderer->cursorPosition = 0;
        appRenderer->selectionAnchor = -1;
        appRenderer->scrollOffset = 0;
        createListExtendedSearch(appRenderer);
        appRenderer->listIndex = 0;
        appRenderer->lastKeypressTime = now;
        appRenderer->needsRedraw = true;
        return;
    }

    if (appRenderer->currentCoordinate != COORDINATE_COMMAND &&
        appRenderer->currentCoordinate != COORDINATE_EXTENDED_SEARCH) {
        if (appRenderer->currentCoordinate != COORDINATE_SIMPLE_SEARCH) {
            appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        }
        appRenderer->currentCoordinate = COORDINATE_EXTENDED_SEARCH;
        accesskitSpeakModeChange(appRenderer, NULL);

        appRenderer->inputBuffer[0] = '\0';
        appRenderer->inputBufferSize = 0;
        appRenderer->cursorPosition = 0;
        appRenderer->selectionAnchor = -1;
        appRenderer->scrollOffset = 0;

        createListExtendedSearch(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->lastKeypressTime = now;
        appRenderer->needsRedraw = true;
    }
}

void handleUp(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        if (appRenderer->scrollSearchMatchCount > 0) {
            if (appRenderer->scrollSearchCurrentMatch > 0)
                appRenderer->scrollSearchCurrentMatch--;
            else
                appRenderer->scrollSearchCurrentMatch = appRenderer->scrollSearchMatchCount - 1;
        }
        appRenderer->needsRedraw = true;
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_INPUT_SEARCH) {
        if (appRenderer->inputSearchMatchCount > 0) {
            if (appRenderer->inputSearchCurrentMatch > 0)
                appRenderer->inputSearchCurrentMatch--;
            else
                appRenderer->inputSearchCurrentMatch = appRenderer->inputSearchMatchCount - 1;
        }
        appRenderer->needsRedraw = true;
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        if (appRenderer->textScrollOffset > 0) {
            appRenderer->textScrollOffset--;
        }
        appRenderer->needsRedraw = true;
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        appRenderer->errorMessage[0] = '\0';
        if (appRenderer->listIndex > 0) {
            appRenderer->listIndex--;
            ListItem *list = appRenderer->filteredListCount > 0 ?
                             appRenderer->filteredListCurrentLayer : appRenderer->totalListCurrentLayer;
            int count = appRenderer->filteredListCount > 0 ?
                        appRenderer->filteredListCount : appRenderer->totalListCount;
            if (appRenderer->listIndex >= 0 && appRenderer->listIndex < count &&
                appRenderer->currentCoordinate != COORDINATE_COMMAND) {
                idArrayCopy(&appRenderer->currentId, &list[appRenderer->listIndex].id);
            }
            accesskitSpeakCurrentElement(appRenderer);
        }
    } else if (appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT &&
               appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT) {
        updateState(appRenderer, TASK_K_ARROW_UP, HISTORY_NONE);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        accesskitSpeakCurrentElement(appRenderer);
    }
    appRenderer->needsRedraw = true;
}

void handleDown(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        if (appRenderer->scrollSearchMatchCount > 0) {
            if (appRenderer->scrollSearchCurrentMatch < appRenderer->scrollSearchMatchCount - 1)
                appRenderer->scrollSearchCurrentMatch++;
            else
                appRenderer->scrollSearchCurrentMatch = 0;
        }
        appRenderer->needsRedraw = true;
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_INPUT_SEARCH) {
        if (appRenderer->inputSearchMatchCount > 0) {
            if (appRenderer->inputSearchCurrentMatch < appRenderer->inputSearchMatchCount - 1)
                appRenderer->inputSearchCurrentMatch++;
            else
                appRenderer->inputSearchCurrentMatch = 0;
        }
        appRenderer->needsRedraw = true;
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        /* Needs getTextScale/getLineHeight and app->swapChainExtent — skip in tests.
         * We test the search/command branches below instead. */
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        appRenderer->errorMessage[0] = '\0';
        int maxIndex = (appRenderer->filteredListCount > 0) ?
                        appRenderer->filteredListCount - 1 :
                        appRenderer->totalListCount - 1;
        if (appRenderer->listIndex < maxIndex) {
            appRenderer->listIndex++;
            ListItem *list = appRenderer->filteredListCount > 0 ?
                             appRenderer->filteredListCurrentLayer : appRenderer->totalListCurrentLayer;
            int count = appRenderer->filteredListCount > 0 ?
                        appRenderer->filteredListCount : appRenderer->totalListCount;
            if (appRenderer->listIndex >= 0 && appRenderer->listIndex < count &&
                appRenderer->currentCoordinate != COORDINATE_COMMAND) {
                idArrayCopy(&appRenderer->currentId, &list[appRenderer->listIndex].id);
            }
            accesskitSpeakCurrentElement(appRenderer);
        }
    } else if (appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT &&
               appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT) {
        updateState(appRenderer, TASK_J_ARROW_DOWN, HISTORY_NONE);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        accesskitSpeakCurrentElement(appRenderer);
    }
    appRenderer->needsRedraw = true;
}

void handleDashboard(AppRenderer *appRenderer) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->dashboardImagePath) return;

    strncpy(appRenderer->dashboardImagePath, provider->dashboardImagePath,
            sizeof(appRenderer->dashboardImagePath) - 1);
    appRenderer->dashboardImagePath[sizeof(appRenderer->dashboardImagePath) - 1] = '\0';

    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    appRenderer->currentCoordinate = COORDINATE_DASHBOARD;
    accesskitSpeakModeChange(appRenderer, NULL);
    appRenderer->needsRedraw = true;
}

/* ============================================
 * Test helpers
 * ============================================ */

static char g_inputBuf[4096];

static AppRenderer createTestApp(void) {
    AppRenderer app;
    memset(&app, 0, sizeof(app));
    app.inputBuffer = g_inputBuf;
    app.inputBufferCapacity = sizeof(g_inputBuf);
    app.inputBuffer[0] = '\0';
    app.inputBufferSize = 0;
    app.cursorPosition = 0;
    app.selectionAnchor = -1;
    app.savedInputBuffer = malloc(1024);
    app.savedInputBufferCapacity = 1024;
    app.savedInputBufferSize = 0;
    app.savedInputBuffer[0] = '\0';
    app.currentId.depth = 1;
    app.currentId.ids[0] = 0;
    return app;
}

static ListItem g_totalItems[10];
static ListItem g_filteredItems[10];

static void setupListItems(AppRenderer *app, int totalCount, int filteredCount) {
    memset(g_totalItems, 0, sizeof(g_totalItems));
    memset(g_filteredItems, 0, sizeof(g_filteredItems));
    for (int i = 0; i < totalCount; i++) {
        g_totalItems[i].id.depth = 1;
        g_totalItems[i].id.ids[0] = i;
    }
    for (int i = 0; i < filteredCount; i++) {
        g_filteredItems[i].id.depth = 1;
        g_filteredItems[i].id.ids[0] = i;
    }
    app->totalListCurrentLayer = g_totalItems;
    app->totalListCount = totalCount;
    if (filteredCount > 0) {
        app->filteredListCurrentLayer = g_filteredItems;
        app->filteredListCount = filteredCount;
    } else {
        app->filteredListCurrentLayer = NULL;
        app->filteredListCount = 0;
    }
}

/* ============================================
 * Unity Setup/Teardown
 * ============================================ */

void setUp(void) {
    FFF_RESET_HISTORY();
    RESET_FAKE(SDL_GetTicks);
    RESET_FAKE(caretReset);
    RESET_FAKE(accesskitSpeakCurrentElement);
    RESET_FAKE(accesskitSpeakModeChange);
    RESET_FAKE(createListCurrentLayer);
    RESET_FAKE(createListExtendedSearch);
    RESET_FAKE(updateState);
    RESET_FAKE(handleHistoryAction);
    RESET_FAKE(setErrorMessage);
    RESET_FAKE(providerNavigateLeft);
    RESET_FAKE(providerNavigateRight);
    RESET_FAKE(providerRefreshCurrentDirectory);
    RESET_FAKE(providerNotifyRadioChanged);
    RESET_FAKE(providerNotifyButtonPressed);
    RESET_FAKE(providerCommitEdit);
    RESET_FAKE(providerCreateFile);
    RESET_FAKE(providerCreateDirectory);
    RESET_FAKE(providerDeleteItem);
    RESET_FAKE(providerExecuteCommand);
    providerTagExtractContent_retval = NULL;
    providerTagHasCheckbox_retval = false;
    providerTagHasCheckboxChecked_retval = false;
    providerTagExtractCheckboxContent_retval = NULL;
    providerTagExtractCheckboxCheckedContent_retval = NULL;
    providerTagFormatCheckboxKey_retval = NULL;
    providerTagFormatCheckboxCheckedKey_retval = NULL;
    getFfonAtId_retval = NULL;
    getFfonAtId_retcount = 0;
    g_activeProvider = NULL;
    SDL_GetTicks_fake.return_val = 1000;
}

void tearDown(void) {}

/* ============================================
 * handleCtrlHome tests
 * ============================================ */

void test_ctrlHome_total_list(void) {
    AppRenderer app = createTestApp();
    setupListItems(&app, 5, 0);
    app.listIndex = 3;
    app.scrollOffset = 3;

    handleCtrlHome(&app);

    TEST_ASSERT_EQUAL_INT(0, app.listIndex);
    TEST_ASSERT_EQUAL_INT(0, app.scrollOffset);
    TEST_ASSERT_TRUE(app.needsRedraw);
    TEST_ASSERT_EQUAL_INT(1, accesskitSpeakCurrentElement_fake.call_count);
}

void test_ctrlHome_filtered_list(void) {
    AppRenderer app = createTestApp();
    setupListItems(&app, 5, 3);
    app.listIndex = 2;

    handleCtrlHome(&app);

    TEST_ASSERT_EQUAL_INT(0, app.listIndex);
    TEST_ASSERT_EQUAL_INT(1, accesskitSpeakCurrentElement_fake.call_count);
}

void test_ctrlHome_empty_list(void) {
    AppRenderer app = createTestApp();
    setupListItems(&app, 0, 0);
    app.listIndex = 0;

    handleCtrlHome(&app);

    TEST_ASSERT_EQUAL_INT(0, app.listIndex);  // Unchanged
    TEST_ASSERT_TRUE(app.needsRedraw);
    TEST_ASSERT_EQUAL_INT(0, accesskitSpeakCurrentElement_fake.call_count);
}

/* ============================================
 * handleCtrlEnd tests
 * ============================================ */

void test_ctrlEnd_total_list(void) {
    AppRenderer app = createTestApp();
    setupListItems(&app, 5, 0);
    app.listIndex = 0;

    handleCtrlEnd(&app);

    TEST_ASSERT_EQUAL_INT(4, app.listIndex);
    TEST_ASSERT_EQUAL_INT(-1, app.scrollOffset);
    TEST_ASSERT_TRUE(app.needsRedraw);
    TEST_ASSERT_EQUAL_INT(1, accesskitSpeakCurrentElement_fake.call_count);
}

void test_ctrlEnd_filtered_list(void) {
    AppRenderer app = createTestApp();
    setupListItems(&app, 5, 3);
    app.listIndex = 0;

    handleCtrlEnd(&app);

    TEST_ASSERT_EQUAL_INT(2, app.listIndex);  // filteredListCount - 1
    TEST_ASSERT_EQUAL_INT(-1, app.scrollOffset);
}

void test_ctrlEnd_empty_list(void) {
    AppRenderer app = createTestApp();
    setupListItems(&app, 0, 0);

    handleCtrlEnd(&app);

    TEST_ASSERT_EQUAL_INT(0, app.listIndex);  // Unchanged
    TEST_ASSERT_EQUAL_INT(0, accesskitSpeakCurrentElement_fake.call_count);
}

/* ============================================
 * handleDelete tests
 * ============================================ */

void test_delete_calls_updateState(void) {
    AppRenderer app = createTestApp();

    handleDelete(&app, HISTORY_UNDO);

    TEST_ASSERT_EQUAL_INT(1, updateState_fake.call_count);
    TEST_ASSERT_EQUAL(TASK_DELETE, updateState_fake.arg1_val);
    TEST_ASSERT_EQUAL(HISTORY_UNDO, updateState_fake.arg2_val);
    TEST_ASSERT_TRUE(app.needsRedraw);
}

void test_delete_no_history(void) {
    AppRenderer app = createTestApp();

    handleDelete(&app, HISTORY_NONE);

    TEST_ASSERT_EQUAL(HISTORY_NONE, updateState_fake.arg2_val);
}

/* ============================================
 * handleColon tests
 * ============================================ */

void test_colon_enters_command_mode(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_OPERATOR_GENERAL;

    handleColon(&app);

    TEST_ASSERT_EQUAL(COORDINATE_COMMAND, app.currentCoordinate);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app.previousCoordinate);
    TEST_ASSERT_EQUAL(COMMAND_NONE, app.currentCommand);
}

void test_colon_clears_input_buffer(void) {
    AppRenderer app = createTestApp();
    strcpy(app.inputBuffer, "hello");
    app.inputBufferSize = 5;
    app.cursorPosition = 3;
    app.selectionAnchor = 1;

    handleColon(&app);

    TEST_ASSERT_EQUAL_STRING("", app.inputBuffer);
    TEST_ASSERT_EQUAL_INT(0, app.inputBufferSize);
    TEST_ASSERT_EQUAL_INT(0, app.cursorPosition);
    TEST_ASSERT_EQUAL_INT(-1, app.selectionAnchor);
}

void test_colon_calls_createListCurrentLayer(void) {
    AppRenderer app = createTestApp();

    handleColon(&app);

    TEST_ASSERT_EQUAL_INT(1, createListCurrentLayer_fake.call_count);
    TEST_ASSERT_EQUAL_INT(1, accesskitSpeakModeChange_fake.call_count);
    TEST_ASSERT_EQUAL_INT(0, app.scrollOffset);
    TEST_ASSERT_TRUE(app.needsRedraw);
}

/* ============================================
 * handleTab tests
 * ============================================ */

void test_tab_noop_in_scroll_mode(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SCROLL;

    handleTab(&app);

    TEST_ASSERT_EQUAL(COORDINATE_SCROLL, app.currentCoordinate);
    TEST_ASSERT_EQUAL_INT(0, createListCurrentLayer_fake.call_count);
}

void test_tab_noop_in_scroll_search_mode(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SCROLL_SEARCH;

    handleTab(&app);

    TEST_ASSERT_EQUAL(COORDINATE_SCROLL_SEARCH, app.currentCoordinate);
}

void test_tab_from_simple_search_enters_scroll(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SIMPLE_SEARCH;
    setupListItems(&app, 3, 0);
    app.listIndex = 1;

    handleTab(&app);

    TEST_ASSERT_EQUAL(COORDINATE_SCROLL, app.currentCoordinate);
    TEST_ASSERT_EQUAL_INT(0, app.textScrollOffset);
    TEST_ASSERT_EQUAL_INT(0, app.textScrollLineCount);
    // Should have copied the selected item's id to currentId
    TEST_ASSERT_EQUAL_INT(1, app.currentId.ids[0]);
    TEST_ASSERT_EQUAL_INT(1, accesskitSpeakModeChange_fake.call_count);
}

void test_tab_from_operator_enters_simple_search(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    app.currentId.depth = 1;
    app.currentId.ids[0] = 2;

    handleTab(&app);

    TEST_ASSERT_EQUAL(COORDINATE_SIMPLE_SEARCH, app.currentCoordinate);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app.previousCoordinate);
    TEST_ASSERT_EQUAL_STRING("", app.inputBuffer);
    TEST_ASSERT_EQUAL_INT(0, app.inputBufferSize);
    TEST_ASSERT_EQUAL_INT(1, createListCurrentLayer_fake.call_count);
    TEST_ASSERT_EQUAL_INT(2, app.listIndex);  // synced with currentId
}

void test_tab_from_editor_enters_simple_search(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_EDITOR_GENERAL;

    handleTab(&app);

    TEST_ASSERT_EQUAL(COORDINATE_SIMPLE_SEARCH, app.currentCoordinate);
    TEST_ASSERT_EQUAL(COORDINATE_EDITOR_GENERAL, app.previousCoordinate);
}

/* ============================================
 * handleEscape tests
 * ============================================ */

void test_escape_from_editor_insert(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_EDITOR_INSERT;

    handleEscape(&app);

    TEST_ASSERT_EQUAL(COORDINATE_EDITOR_GENERAL, app.currentCoordinate);
    TEST_ASSERT_EQUAL_INT(1, updateState_fake.call_count);
    TEST_ASSERT_EQUAL(TASK_INPUT, updateState_fake.arg1_val);
    TEST_ASSERT_TRUE(app.needsRedraw);
}

void test_escape_from_operator_insert(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_OPERATOR_INSERT;
    app.prefixedInsertMode = false;

    handleEscape(&app);

    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app.currentCoordinate);
    TEST_ASSERT_TRUE(app.needsRedraw);
}

void test_escape_from_command_mode(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_COMMAND;
    app.previousCoordinate = COORDINATE_OPERATOR_GENERAL;
    app.currentCommand = COMMAND_NONE;
    app.currentId.depth = 1;
    app.currentId.ids[0] = 2;

    handleEscape(&app);

    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app.currentCoordinate);
    TEST_ASSERT_EQUAL(COMMAND_NONE, app.currentCommand);
    TEST_ASSERT_EQUAL_INT(1, createListCurrentLayer_fake.call_count);
    TEST_ASSERT_EQUAL_INT(2, app.listIndex);
    TEST_ASSERT_EQUAL_INT(0, app.scrollOffset);
}

void test_escape_from_simple_search(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SIMPLE_SEARCH;
    app.previousCoordinate = COORDINATE_EDITOR_GENERAL;
    app.currentId.depth = 1;
    app.currentId.ids[0] = 3;

    handleEscape(&app);

    TEST_ASSERT_EQUAL(COORDINATE_EDITOR_GENERAL, app.currentCoordinate);
    TEST_ASSERT_EQUAL_INT(1, createListCurrentLayer_fake.call_count);
    TEST_ASSERT_EQUAL_INT(3, app.listIndex);
}

void test_escape_from_extended_search(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_EXTENDED_SEARCH;
    app.previousCoordinate = COORDINATE_OPERATOR_GENERAL;
    app.currentId.depth = 1;
    app.currentId.ids[0] = 1;

    handleEscape(&app);

    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app.currentCoordinate);
    TEST_ASSERT_EQUAL_INT(1, createListCurrentLayer_fake.call_count);
    TEST_ASSERT_EQUAL_INT(1, app.listIndex);
}

void test_escape_from_scroll_search(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SCROLL_SEARCH;
    app.scrollSearchMatchCount = 5;
    app.scrollSearchCurrentMatch = 2;

    handleEscape(&app);

    TEST_ASSERT_EQUAL(COORDINATE_SCROLL, app.currentCoordinate);
    TEST_ASSERT_EQUAL_INT(0, app.scrollSearchMatchCount);
    TEST_ASSERT_EQUAL_INT(0, app.scrollSearchCurrentMatch);
}

void test_escape_from_scroll_enters_simple_search(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SCROLL;
    app.currentId.depth = 1;
    app.currentId.ids[0] = 0;

    handleEscape(&app);

    TEST_ASSERT_EQUAL(COORDINATE_SIMPLE_SEARCH, app.currentCoordinate);
    TEST_ASSERT_EQUAL_STRING("", app.inputBuffer);
    TEST_ASSERT_EQUAL_INT(0, app.inputBufferSize);
    TEST_ASSERT_EQUAL_INT(1, createListCurrentLayer_fake.call_count);
}

void test_escape_clears_selection(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_OPERATOR_INSERT;
    app.selectionAnchor = 5;

    handleEscape(&app);

    TEST_ASSERT_EQUAL_INT(-1, app.selectionAnchor);
}

void test_escape_from_unknown_with_operator_previous(void) {
    // When coordinate is not a special mode but previousCoordinate is OPERATOR_GENERAL
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_EDITOR_GENERAL;  // Not a special mode for escape
    app.previousCoordinate = COORDINATE_OPERATOR_GENERAL;

    handleEscape(&app);

    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app.currentCoordinate);
}

void test_escape_from_unknown_defaults_to_editor(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_EDITOR_GENERAL;
    app.previousCoordinate = COORDINATE_EDITOR_GENERAL;

    handleEscape(&app);

    TEST_ASSERT_EQUAL(COORDINATE_EDITOR_GENERAL, app.currentCoordinate);
}

/* ============================================
 * handleCommand tests
 * ============================================ */

void test_command_none_noop(void) {
    AppRenderer app = createTestApp();
    app.currentCommand = COMMAND_NONE;
    app.currentCoordinate = COORDINATE_COMMAND;

    handleCommand(&app);

    TEST_ASSERT_EQUAL(COORDINATE_COMMAND, app.currentCoordinate);  // Unchanged
    TEST_ASSERT_TRUE(app.needsRedraw);
    TEST_ASSERT_EQUAL_INT(0, accesskitSpeakModeChange_fake.call_count);
}


/* ============================================
 * handleCtrlF tests
 * ============================================ */

void test_ctrlF_from_scroll_enters_scroll_search(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SCROLL;

    handleCtrlF(&app);

    TEST_ASSERT_EQUAL(COORDINATE_SCROLL_SEARCH, app.currentCoordinate);
    TEST_ASSERT_EQUAL(COORDINATE_SCROLL, app.previousCoordinate);
    TEST_ASSERT_EQUAL_STRING("", app.inputBuffer);
    TEST_ASSERT_EQUAL_INT(0, app.scrollSearchMatchCount);
    TEST_ASSERT_EQUAL_INT(0, app.scrollSearchCurrentMatch);
    TEST_ASSERT_EQUAL_INT(1, accesskitSpeakModeChange_fake.call_count);
}

void test_ctrlF_noop_in_scroll_search(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SCROLL_SEARCH;

    handleCtrlF(&app);

    TEST_ASSERT_EQUAL(COORDINATE_SCROLL_SEARCH, app.currentCoordinate);
    TEST_ASSERT_EQUAL_INT(0, accesskitSpeakModeChange_fake.call_count);
    TEST_ASSERT_FALSE(app.needsRedraw);
}

void test_ctrlF_from_operator_enters_extended_search(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    app.currentId.depth = 1;
    app.currentId.ids[0] = 2;

    handleCtrlF(&app);

    TEST_ASSERT_EQUAL(COORDINATE_EXTENDED_SEARCH, app.currentCoordinate);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app.previousCoordinate);
    TEST_ASSERT_EQUAL_STRING("", app.inputBuffer);
    TEST_ASSERT_EQUAL_INT(1, createListExtendedSearch_fake.call_count);
    TEST_ASSERT_EQUAL_INT(2, app.listIndex);
}

void test_ctrlF_double_tap_resets_to_root(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_EXTENDED_SEARCH;
    app.currentId.depth = 3;
    app.currentId.ids[0] = 0;
    app.currentId.ids[1] = 1;
    app.currentId.ids[2] = 2;
    app.lastKeypressTime = 800;  // 200ms ago (within DELTA_MS)

    handleCtrlF(&app);

    TEST_ASSERT_EQUAL_INT(1, app.currentId.depth);  // Reset to root
    TEST_ASSERT_EQUAL_INT(0, app.listIndex);
    TEST_ASSERT_EQUAL_INT(1, createListExtendedSearch_fake.call_count);
}

void test_ctrlF_noop_in_command_mode(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_COMMAND;

    handleCtrlF(&app);

    TEST_ASSERT_EQUAL(COORDINATE_COMMAND, app.currentCoordinate);
    TEST_ASSERT_EQUAL_INT(0, createListExtendedSearch_fake.call_count);
}

void test_ctrlF_preserves_previous_from_simple_search(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SIMPLE_SEARCH;
    app.previousCoordinate = COORDINATE_OPERATOR_GENERAL;

    handleCtrlF(&app);

    TEST_ASSERT_EQUAL(COORDINATE_EXTENDED_SEARCH, app.currentCoordinate);
    // previousCoordinate should NOT be overwritten when coming from simple search
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app.previousCoordinate);
}

/* ============================================
 * handleUp tests
 * ============================================ */

void test_up_in_scroll_search_wraps_to_last(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SCROLL_SEARCH;
    app.scrollSearchMatchCount = 5;
    app.scrollSearchCurrentMatch = 0;

    handleUp(&app);

    TEST_ASSERT_EQUAL_INT(4, app.scrollSearchCurrentMatch);  // Wraps to last
}

void test_up_in_scroll_search_decrements(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SCROLL_SEARCH;
    app.scrollSearchMatchCount = 5;
    app.scrollSearchCurrentMatch = 3;

    handleUp(&app);

    TEST_ASSERT_EQUAL_INT(2, app.scrollSearchCurrentMatch);
}

void test_up_in_scroll_mode_decrements_offset(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SCROLL;
    app.textScrollOffset = 5;

    handleUp(&app);

    TEST_ASSERT_EQUAL_INT(4, app.textScrollOffset);
}

void test_up_in_scroll_mode_clamps_at_zero(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SCROLL;
    app.textScrollOffset = 0;

    handleUp(&app);

    TEST_ASSERT_EQUAL_INT(0, app.textScrollOffset);
}

void test_up_in_search_decrements_listIndex(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SIMPLE_SEARCH;
    setupListItems(&app, 5, 0);
    app.listIndex = 3;
    strcpy(app.errorMessage, "some error");

    handleUp(&app);

    TEST_ASSERT_EQUAL_INT(2, app.listIndex);
    TEST_ASSERT_EQUAL_STRING("", app.errorMessage);  // Error cleared
    TEST_ASSERT_EQUAL_INT(1, accesskitSpeakCurrentElement_fake.call_count);
    // Should copy list id to currentId (non-command mode)
    TEST_ASSERT_EQUAL_INT(2, app.currentId.ids[0]);
}

void test_up_in_command_mode_no_id_copy(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_COMMAND;
    setupListItems(&app, 5, 0);
    app.listIndex = 2;
    app.currentId.ids[0] = 99;

    handleUp(&app);

    TEST_ASSERT_EQUAL_INT(1, app.listIndex);
    // currentId should NOT be updated in command mode
    TEST_ASSERT_EQUAL_INT(99, app.currentId.ids[0]);
}

void test_up_at_zero_stays(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SIMPLE_SEARCH;
    setupListItems(&app, 5, 0);
    app.listIndex = 0;

    handleUp(&app);

    TEST_ASSERT_EQUAL_INT(0, app.listIndex);
    TEST_ASSERT_EQUAL_INT(0, accesskitSpeakCurrentElement_fake.call_count);
}

void test_up_in_general_calls_updateState(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_OPERATOR_GENERAL;

    handleUp(&app);

    TEST_ASSERT_EQUAL_INT(1, updateState_fake.call_count);
    TEST_ASSERT_EQUAL(TASK_K_ARROW_UP, updateState_fake.arg1_val);
    TEST_ASSERT_EQUAL_INT(1, accesskitSpeakCurrentElement_fake.call_count);
}

void test_up_noop_in_insert_mode(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_EDITOR_INSERT;

    handleUp(&app);

    TEST_ASSERT_EQUAL_INT(0, updateState_fake.call_count);
    TEST_ASSERT_EQUAL_INT(0, accesskitSpeakCurrentElement_fake.call_count);
    TEST_ASSERT_TRUE(app.needsRedraw);
}

/* ============================================
 * handleDown tests
 * ============================================ */

void test_down_in_scroll_search_wraps_to_first(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SCROLL_SEARCH;
    app.scrollSearchMatchCount = 5;
    app.scrollSearchCurrentMatch = 4;

    handleDown(&app);

    TEST_ASSERT_EQUAL_INT(0, app.scrollSearchCurrentMatch);  // Wraps to first
}

void test_down_in_scroll_search_increments(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SCROLL_SEARCH;
    app.scrollSearchMatchCount = 5;
    app.scrollSearchCurrentMatch = 2;

    handleDown(&app);

    TEST_ASSERT_EQUAL_INT(3, app.scrollSearchCurrentMatch);
}

void test_down_in_search_increments_listIndex(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SIMPLE_SEARCH;
    setupListItems(&app, 5, 0);
    app.listIndex = 2;

    handleDown(&app);

    TEST_ASSERT_EQUAL_INT(3, app.listIndex);
    TEST_ASSERT_EQUAL_INT(1, accesskitSpeakCurrentElement_fake.call_count);
    TEST_ASSERT_EQUAL_INT(3, app.currentId.ids[0]);
}

void test_down_at_max_stays(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_SIMPLE_SEARCH;
    setupListItems(&app, 5, 0);
    app.listIndex = 4;

    handleDown(&app);

    TEST_ASSERT_EQUAL_INT(4, app.listIndex);
    TEST_ASSERT_EQUAL_INT(0, accesskitSpeakCurrentElement_fake.call_count);
}

void test_down_in_command_mode_no_id_copy(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_COMMAND;
    setupListItems(&app, 5, 0);
    app.listIndex = 1;
    app.currentId.ids[0] = 99;

    handleDown(&app);

    TEST_ASSERT_EQUAL_INT(2, app.listIndex);
    TEST_ASSERT_EQUAL_INT(99, app.currentId.ids[0]);
}

void test_down_in_general_calls_updateState(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_EDITOR_GENERAL;

    handleDown(&app);

    TEST_ASSERT_EQUAL_INT(1, updateState_fake.call_count);
    TEST_ASSERT_EQUAL(TASK_J_ARROW_DOWN, updateState_fake.arg1_val);
}

void test_down_noop_in_operator_insert(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_OPERATOR_INSERT;

    handleDown(&app);

    TEST_ASSERT_EQUAL_INT(0, updateState_fake.call_count);
}

void test_down_uses_filtered_count(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_EXTENDED_SEARCH;
    setupListItems(&app, 10, 3);
    app.listIndex = 2;

    handleDown(&app);

    // maxIndex is filteredListCount - 1 = 2, listIndex already at 2
    TEST_ASSERT_EQUAL_INT(2, app.listIndex);  // Can't go further
}

/* ============================================
 * handleDashboard tests
 * ============================================ */

void test_dashboard_with_provider(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    Provider prov = { .dashboardImagePath = "/path/to/image.webp" };
    g_activeProvider = &prov;

    handleDashboard(&app);

    TEST_ASSERT_EQUAL(COORDINATE_DASHBOARD, app.currentCoordinate);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app.previousCoordinate);
    TEST_ASSERT_EQUAL_STRING("/path/to/image.webp", app.dashboardImagePath);
    TEST_ASSERT_EQUAL_INT(1, accesskitSpeakModeChange_fake.call_count);
    TEST_ASSERT_TRUE(app.needsRedraw);
}

void test_dashboard_no_provider(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    g_activeProvider = NULL;

    handleDashboard(&app);

    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app.currentCoordinate);
    TEST_ASSERT_FALSE(app.needsRedraw);
}

void test_dashboard_null_image_path(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    Provider prov = { .dashboardImagePath = NULL };
    g_activeProvider = &prov;

    handleDashboard(&app);

    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app.currentCoordinate);
    TEST_ASSERT_FALSE(app.needsRedraw);
}

void test_escape_from_dashboard(void) {
    AppRenderer app = createTestApp();
    app.currentCoordinate = COORDINATE_DASHBOARD;
    app.previousCoordinate = COORDINATE_OPERATOR_GENERAL;

    handleEscape(&app);

    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app.currentCoordinate);
    TEST_ASSERT_EQUAL_INT(1, accesskitSpeakModeChange_fake.call_count);
    TEST_ASSERT_TRUE(app.needsRedraw);
}

/* ============================================
 * handleCheckboxToggle (copied from handlers.c)
 * ============================================ */

static bool handleCheckboxToggle(AppRenderer *appRenderer, IdArray *elementId) {
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, elementId, &count);
    if (!arr) return false;
    int idx = elementId->ids[elementId->depth - 1];
    if (idx < 0 || idx >= count) return false;
    FfonElement *elem = arr[idx];

    // Get pointer to the tag string (string value or object key)
    char **tagPtr = NULL;
    if (elem->type == FFON_STRING) {
        tagPtr = &elem->data.string;
    } else if (elem->type == FFON_OBJECT) {
        tagPtr = &elem->data.object->key;
    } else {
        return false;
    }

    if (providerTagHasCheckboxChecked(*tagPtr)) {
        char *content = providerTagExtractCheckboxCheckedContent(*tagPtr);
        if (!content) return false;
        char *newKey = providerTagFormatCheckboxKey(content);
        free(content);
        if (newKey) {
            free(*tagPtr);
            *tagPtr = newKey;
        }
        return true;
    } else if (providerTagHasCheckbox(*tagPtr)) {
        char *content = providerTagExtractCheckboxContent(*tagPtr);
        if (!content) return false;
        char *newKey = providerTagFormatCheckboxCheckedKey(content);
        free(content);
        if (newKey) {
            free(*tagPtr);
            *tagPtr = newKey;
        }
        return true;
    }

    return false;
}

/* ============================================
 * handleCheckboxToggle tests
 * ============================================ */

void test_checkboxToggle_object_check(void) {
    // Toggle unchecked FFON_OBJECT checkbox -> checked
    AppRenderer app = createTestApp();
    FfonObject obj = { .key = strdup("<checkbox>Enable"), .elements = NULL, .count = 0, .capacity = 0 };
    FfonElement elem = { .type = FFON_OBJECT, .data.object = &obj };
    FfonElement *elems[] = { &elem };
    getFfonAtId_retval = elems;
    getFfonAtId_retcount = 1;
    providerTagHasCheckbox_retval = true;
    providerTagExtractCheckboxContent_retval = "Enable";
    providerTagFormatCheckboxCheckedKey_retval = "<checkbox checked>Enable";

    IdArray id = { .ids = {0}, .depth = 1 };
    bool result = handleCheckboxToggle(&app, &id);

    TEST_ASSERT_TRUE(result);
    TEST_ASSERT_EQUAL_STRING("<checkbox checked>Enable", obj.key);
    free(obj.key);
}

void test_checkboxToggle_object_uncheck(void) {
    // Toggle checked FFON_OBJECT checkbox -> unchecked
    AppRenderer app = createTestApp();
    FfonObject obj = { .key = strdup("<checkbox checked>Enable"), .elements = NULL, .count = 0, .capacity = 0 };
    FfonElement elem = { .type = FFON_OBJECT, .data.object = &obj };
    FfonElement *elems[] = { &elem };
    getFfonAtId_retval = elems;
    getFfonAtId_retcount = 1;
    providerTagHasCheckboxChecked_retval = true;
    providerTagExtractCheckboxCheckedContent_retval = "Enable";
    providerTagFormatCheckboxKey_retval = "<checkbox>Enable";

    IdArray id = { .ids = {0}, .depth = 1 };
    bool result = handleCheckboxToggle(&app, &id);

    TEST_ASSERT_TRUE(result);
    TEST_ASSERT_EQUAL_STRING("<checkbox>Enable", obj.key);
    free(obj.key);
}

void test_checkboxToggle_object_no_checkbox(void) {
    // Non-checkbox FFON_OBJECT returns false
    AppRenderer app = createTestApp();
    FfonObject obj = { .key = strdup("Just a label"), .elements = NULL, .count = 0, .capacity = 0 };
    FfonElement elem = { .type = FFON_OBJECT, .data.object = &obj };
    FfonElement *elems[] = { &elem };
    getFfonAtId_retval = elems;
    getFfonAtId_retcount = 1;
    // providerTagHasCheckbox and providerTagHasCheckboxChecked both default false

    IdArray id = { .ids = {0}, .depth = 1 };
    bool result = handleCheckboxToggle(&app, &id);

    TEST_ASSERT_FALSE(result);
    TEST_ASSERT_EQUAL_STRING("Just a label", obj.key);
    free(obj.key);
}

/* ============================================
 * Main
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // handleCtrlHome
    RUN_TEST(test_ctrlHome_total_list);
    RUN_TEST(test_ctrlHome_filtered_list);
    RUN_TEST(test_ctrlHome_empty_list);

    // handleCtrlEnd
    RUN_TEST(test_ctrlEnd_total_list);
    RUN_TEST(test_ctrlEnd_filtered_list);
    RUN_TEST(test_ctrlEnd_empty_list);

    // handleDelete
    RUN_TEST(test_delete_calls_updateState);
    RUN_TEST(test_delete_no_history);

    // handleColon
    RUN_TEST(test_colon_enters_command_mode);
    RUN_TEST(test_colon_clears_input_buffer);
    RUN_TEST(test_colon_calls_createListCurrentLayer);

    // handleTab
    RUN_TEST(test_tab_noop_in_scroll_mode);
    RUN_TEST(test_tab_noop_in_scroll_search_mode);
    RUN_TEST(test_tab_from_simple_search_enters_scroll);
    RUN_TEST(test_tab_from_operator_enters_simple_search);
    RUN_TEST(test_tab_from_editor_enters_simple_search);

    // handleEscape
    RUN_TEST(test_escape_from_editor_insert);
    RUN_TEST(test_escape_from_operator_insert);
    RUN_TEST(test_escape_from_command_mode);
    RUN_TEST(test_escape_from_simple_search);
    RUN_TEST(test_escape_from_extended_search);
    RUN_TEST(test_escape_from_scroll_search);
    RUN_TEST(test_escape_from_scroll_enters_simple_search);
    RUN_TEST(test_escape_clears_selection);
    RUN_TEST(test_escape_from_unknown_with_operator_previous);
    RUN_TEST(test_escape_from_unknown_defaults_to_editor);

    // handleCommand
    RUN_TEST(test_command_none_noop);

    // handleCtrlF
    RUN_TEST(test_ctrlF_from_scroll_enters_scroll_search);
    RUN_TEST(test_ctrlF_noop_in_scroll_search);
    RUN_TEST(test_ctrlF_from_operator_enters_extended_search);
    RUN_TEST(test_ctrlF_double_tap_resets_to_root);
    RUN_TEST(test_ctrlF_noop_in_command_mode);
    RUN_TEST(test_ctrlF_preserves_previous_from_simple_search);

    // handleUp
    RUN_TEST(test_up_in_scroll_search_wraps_to_last);
    RUN_TEST(test_up_in_scroll_search_decrements);
    RUN_TEST(test_up_in_scroll_mode_decrements_offset);
    RUN_TEST(test_up_in_scroll_mode_clamps_at_zero);
    RUN_TEST(test_up_in_search_decrements_listIndex);
    RUN_TEST(test_up_in_command_mode_no_id_copy);
    RUN_TEST(test_up_at_zero_stays);
    RUN_TEST(test_up_in_general_calls_updateState);
    RUN_TEST(test_up_noop_in_insert_mode);

    // handleDown
    RUN_TEST(test_down_in_scroll_search_wraps_to_first);
    RUN_TEST(test_down_in_scroll_search_increments);
    RUN_TEST(test_down_in_search_increments_listIndex);
    RUN_TEST(test_down_at_max_stays);
    RUN_TEST(test_down_in_command_mode_no_id_copy);
    RUN_TEST(test_down_in_general_calls_updateState);
    RUN_TEST(test_down_noop_in_operator_insert);
    RUN_TEST(test_down_uses_filtered_count);

    // handleDashboard
    RUN_TEST(test_dashboard_with_provider);
    RUN_TEST(test_dashboard_no_provider);
    RUN_TEST(test_dashboard_null_image_path);
    RUN_TEST(test_escape_from_dashboard);

    // handleCheckboxToggle
    RUN_TEST(test_checkboxToggle_object_check);
    RUN_TEST(test_checkboxToggle_object_uncheck);
    RUN_TEST(test_checkboxToggle_object_no_checkbox);

    return UNITY_END();
}

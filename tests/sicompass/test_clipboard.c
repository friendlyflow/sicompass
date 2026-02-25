/*
 * Tests for clipboard operations: handleCtrlX (cut), handleCtrlC (copy), handleCtrlV (paste)
 * Tests both element mode (editor general) and text mode (insert/search/command).
 */

#include <unity.h>
#include <fff/fff.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

DEFINE_FFF_GLOBALS;

// Mock updateHistory since we don't want to test history tracking here
FAKE_VOID_FUNC(updateHistory, void*, int, const void*, void*, void*, int);

// Mock SDL clipboard functions for text mode tests
FAKE_VALUE_FUNC(bool, SDL_SetClipboardText, const char*);
FAKE_VALUE_FUNC(char*, SDL_GetClipboardText);
FAKE_VALUE_FUNC(bool, SDL_HasClipboardText);
FAKE_VOID_FUNC(SDL_free, void*);

// Include necessary type definitions
#define MAX_ID_DEPTH 32
#define MAX_LINE_LENGTH 65536
#define UNDO_HISTORY_SIZE 500

typedef enum {
    FFON_STRING,
    FFON_OBJECT
} FfonType;

typedef struct FfonElement FfonElement;
typedef struct FfonObject FfonObject;

struct FfonObject {
    char *key;
    FfonElement **elements;
    int count;
    int capacity;
};

struct FfonElement {
    FfonType type;
    union {
        char *string;
        FfonObject *object;
    } data;
};

typedef struct {
    int ids[MAX_ID_DEPTH];
    int depth;
} IdArray;

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
    COORDINATE_OPERATOR_GENERAL,
    COORDINATE_OPERATOR_INSERT,
    COORDINATE_EDITOR_GENERAL,
    COORDINATE_EDITOR_INSERT,
    COORDINATE_EDITOR_NORMAL,
    COORDINATE_EDITOR_VISUAL,
    COORDINATE_SIMPLE_SEARCH,
    COORDINATE_EXTENDED_SEARCH,
    COORDINATE_COMMAND
} Coordinate;

typedef struct {
    IdArray id;
    Task task;
    FfonElement *prevElement;
    FfonElement *newElement;
} UndoEntry;

typedef struct {
    FfonElement **ffon;
    int ffonCount;
    int ffonCapacity;

    IdArray currentId;
    IdArray previousId;

    Coordinate currentCoordinate;

    FfonElement *clipboard;

    UndoEntry *undoHistory;
    int undoHistoryCount;
    int undoPosition;

    // Text input fields
    char *inputBuffer;
    int inputBufferSize;
    int inputBufferCapacity;
    int cursorPosition;
    int selectionAnchor;

    bool needsRedraw;
} AppRenderer;

// Helper functions for creating FFON elements
static FfonElement* ffonElementCreateString(const char *str) {
    FfonElement *elem = malloc(sizeof(FfonElement));
    elem->type = FFON_STRING;
    elem->data.string = strdup(str ? str : "");
    return elem;
}

static FfonObject* ffonObjectCreate(const char *key) {
    FfonObject *obj = malloc(sizeof(FfonObject));
    obj->key = strdup(key ? key : "");
    obj->elements = NULL;
    obj->count = 0;
    obj->capacity = 0;
    return obj;
}

static FfonElement* ffonElementCreateObject(const char *key) {
    FfonElement *elem = malloc(sizeof(FfonElement));
    elem->type = FFON_OBJECT;
    elem->data.object = ffonObjectCreate(key);
    return elem;
}

static void ffonObjectAddElement(FfonObject *obj, FfonElement *elem) {
    if (obj->count >= obj->capacity) {
        obj->capacity = obj->capacity == 0 ? 4 : obj->capacity * 2;
        obj->elements = realloc(obj->elements, sizeof(FfonElement*) * obj->capacity);
    }
    obj->elements[obj->count++] = elem;
}

static void ffonElementDestroy(FfonElement *elem) {
    if (!elem) return;
    if (elem->type == FFON_STRING) {
        free(elem->data.string);
    } else if (elem->type == FFON_OBJECT) {
        FfonObject *obj = elem->data.object;
        for (int i = 0; i < obj->count; i++) {
            ffonElementDestroy(obj->elements[i]);
        }
        free(obj->elements);
        free(obj->key);
        free(obj);
    }
    free(elem);
}

static FfonElement* ffonElementClone(FfonElement *elem) {
    if (!elem) return NULL;
    if (elem->type == FFON_STRING) {
        return ffonElementCreateString(elem->data.string);
    } else {
        FfonElement *newElem = ffonElementCreateObject(elem->data.object->key);
        for (int i = 0; i < elem->data.object->count; i++) {
            ffonObjectAddElement(newElem->data.object,
                               ffonElementClone(elem->data.object->elements[i]));
        }
        return newElem;
    }
}

// Selection helpers (matching handlers.c)
static bool hasSelection(AppRenderer *appRenderer) {
    return appRenderer->selectionAnchor != -1 &&
           appRenderer->selectionAnchor != appRenderer->cursorPosition;
}

static void clearSelection(AppRenderer *appRenderer) {
    appRenderer->selectionAnchor = -1;
}

static void getSelectionRange(AppRenderer *appRenderer, int *start, int *end) {
    int a = appRenderer->selectionAnchor;
    int b = appRenderer->cursorPosition;
    *start = (a < b) ? a : b;
    *end = (a > b) ? a : b;
}

static void deleteSelection(AppRenderer *appRenderer) {
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

// Element-mode clipboard functions (simplified from update.c, element path only)
static void handleCtrlX_element(AppRenderer *appRenderer) {
    FfonElement **_ffon = appRenderer->ffon;
    int _ffon_count = appRenderer->ffonCount;
    FfonObject *parentObj = NULL;

    for (int i = 0; i < appRenderer->currentId.depth - 1; i++) {
        int idx = appRenderer->currentId.ids[i];
        if (idx < 0 || idx >= _ffon_count || _ffon[idx]->type != FFON_OBJECT) {
            return;
        }
        parentObj = _ffon[idx]->data.object;
        _ffon = parentObj->elements;
        _ffon_count = parentObj->count;
    }

    int currentIdx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (currentIdx < 0 || currentIdx >= _ffon_count) return;

    FfonElement *elem = _ffon[currentIdx];

    if (appRenderer->clipboard) {
        ffonElementDestroy(appRenderer->clipboard);
    }

    appRenderer->clipboard = ffonElementClone(elem);

    ffonElementDestroy(_ffon[currentIdx]);
    for (int j = currentIdx; j < _ffon_count - 1; j++) {
        _ffon[j] = _ffon[j + 1];
    }
    _ffon_count--;

    if (parentObj) {
        parentObj->count = _ffon_count;
    } else {
        appRenderer->ffonCount = _ffon_count;
    }

    if (currentIdx > 0) {
        appRenderer->currentId.ids[appRenderer->currentId.depth - 1]--;
    }

    appRenderer->needsRedraw = true;
}

static void handleCtrlC_element(AppRenderer *appRenderer) {
    FfonElement **_ffon = appRenderer->ffon;
    int _ffon_count = appRenderer->ffonCount;

    for (int i = 0; i < appRenderer->currentId.depth - 1; i++) {
        int idx = appRenderer->currentId.ids[i];
        if (idx < 0 || idx >= _ffon_count || _ffon[idx]->type != FFON_OBJECT) {
            return;
        }
        FfonObject *obj = _ffon[idx]->data.object;
        _ffon = obj->elements;
        _ffon_count = obj->count;
    }

    int currentIdx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (currentIdx < 0 || currentIdx >= _ffon_count) return;

    FfonElement *elem = _ffon[currentIdx];

    if (appRenderer->clipboard) {
        ffonElementDestroy(appRenderer->clipboard);
    }

    appRenderer->clipboard = ffonElementClone(elem);

    appRenderer->needsRedraw = true;
}

static void handleCtrlV_element(AppRenderer *appRenderer) {
    if (!appRenderer->clipboard) return;

    FfonElement **_ffon = appRenderer->ffon;
    int _ffon_count = appRenderer->ffonCount;

    for (int i = 0; i < appRenderer->currentId.depth - 1; i++) {
        int idx = appRenderer->currentId.ids[i];
        if (idx < 0 || idx >= _ffon_count || _ffon[idx]->type != FFON_OBJECT) {
            return;
        }
        FfonObject *obj = _ffon[idx]->data.object;
        _ffon = obj->elements;
        _ffon_count = obj->count;
    }

    int currentIdx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (currentIdx < 0 || currentIdx >= _ffon_count) return;

    FfonElement *newElem = ffonElementClone(appRenderer->clipboard);
    if (newElem) {
        ffonElementDestroy(_ffon[currentIdx]);
        _ffon[currentIdx] = newElem;
    }

    appRenderer->needsRedraw = true;
}

// Text-mode clipboard functions (matching the text path in update.c)
static void handleCtrlX_text(AppRenderer *appRenderer) {
    if (!hasSelection(appRenderer)) return;

    int start, end;
    getSelectionRange(appRenderer, &start, &end);

    int len = end - start;
    char *selectedText = malloc(len + 1);
    if (!selectedText) return;
    memcpy(selectedText, &appRenderer->inputBuffer[start], len);
    selectedText[len] = '\0';

    SDL_SetClipboardText(selectedText);
    free(selectedText);

    deleteSelection(appRenderer);
    appRenderer->needsRedraw = true;
}

static void handleCtrlC_text(AppRenderer *appRenderer) {
    if (!hasSelection(appRenderer)) return;

    int start, end;
    getSelectionRange(appRenderer, &start, &end);

    int len = end - start;
    char *selectedText = malloc(len + 1);
    if (!selectedText) return;
    memcpy(selectedText, &appRenderer->inputBuffer[start], len);
    selectedText[len] = '\0';

    SDL_SetClipboardText(selectedText);
    free(selectedText);

    appRenderer->needsRedraw = true;
}

static void handleCtrlV_text(AppRenderer *appRenderer) {
    if (!SDL_HasClipboardText()) return;

    char *text = SDL_GetClipboardText();
    if (!text || text[0] == '\0') {
        SDL_free(text);
        return;
    }

    if (hasSelection(appRenderer)) {
        deleteSelection(appRenderer);
    }

    int len = strlen(text);
    while (appRenderer->inputBufferSize + len >= appRenderer->inputBufferCapacity) {
        int newCapacity = appRenderer->inputBufferCapacity * 2;
        char *newBuffer = realloc(appRenderer->inputBuffer, newCapacity);
        if (!newBuffer) {
            SDL_free(text);
            return;
        }
        appRenderer->inputBuffer = newBuffer;
        appRenderer->inputBufferCapacity = newCapacity;
    }

    memmove(&appRenderer->inputBuffer[appRenderer->cursorPosition + len],
            &appRenderer->inputBuffer[appRenderer->cursorPosition],
            appRenderer->inputBufferSize - appRenderer->cursorPosition + 1);
    memcpy(&appRenderer->inputBuffer[appRenderer->cursorPosition], text, len);
    appRenderer->inputBufferSize += len;
    appRenderer->cursorPosition += len;

    SDL_free(text);
    appRenderer->needsRedraw = true;
}

// Helper to create a test AppRenderer with basic structure
static AppRenderer* createTestAppRenderer(void) {
    AppRenderer *app = calloc(1, sizeof(AppRenderer));
    app->currentId.depth = 1;
    app->currentId.ids[0] = 0;
    app->previousId.depth = 1;
    app->previousId.ids[0] = 0;
    app->currentCoordinate = COORDINATE_EDITOR_GENERAL;
    app->selectionAnchor = -1;
    return app;
}

static AppRenderer* createTestAppRendererWithTextBuffer(const char *text) {
    AppRenderer *app = createTestAppRenderer();
    app->inputBufferCapacity = 1024;
    app->inputBuffer = calloc(app->inputBufferCapacity, sizeof(char));
    if (text) {
        strncpy(app->inputBuffer, text, app->inputBufferCapacity - 1);
        app->inputBufferSize = strlen(app->inputBuffer);
    }
    return app;
}

static void destroyTestAppRenderer(AppRenderer *app) {
    for (int i = 0; i < app->ffonCount; i++) {
        ffonElementDestroy(app->ffon[i]);
    }
    free(app->ffon);
    if (app->clipboard) {
        ffonElementDestroy(app->clipboard);
    }
    free(app->inputBuffer);
    free(app);
}

/* ============================================
 * Unity Test Setup/Teardown
 * ============================================ */

void setUp(void) {
    RESET_FAKE(updateHistory);
    RESET_FAKE(SDL_SetClipboardText);
    RESET_FAKE(SDL_GetClipboardText);
    RESET_FAKE(SDL_HasClipboardText);
    RESET_FAKE(SDL_free);
    FFF_RESET_HISTORY();
}

void tearDown(void) {
}

/* ============================================
 * handleCtrlC (copy) element tests
 * ============================================ */

void test_handleCtrlC_copy_string_element(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("hello");
    app->ffon[1] = ffonElementCreateString("world");
    app->ffonCount = 2;

    app->currentId.ids[0] = 0;

    handleCtrlC_element(app);

    TEST_ASSERT_NOT_NULL(app->clipboard);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, app->clipboard->type);
    TEST_ASSERT_EQUAL_STRING("hello", app->clipboard->data.string);
    TEST_ASSERT_EQUAL_INT(2, app->ffonCount);
    TEST_ASSERT_TRUE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

void test_handleCtrlC_copy_second_element(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("first");
    app->ffon[1] = ffonElementCreateString("second");
    app->ffonCount = 2;

    app->currentId.ids[0] = 1;

    handleCtrlC_element(app);

    TEST_ASSERT_NOT_NULL(app->clipboard);
    TEST_ASSERT_EQUAL_STRING("second", app->clipboard->data.string);
    TEST_ASSERT_EQUAL_INT(2, app->ffonCount);

    destroyTestAppRenderer(app);
}

void test_handleCtrlC_copy_object_element(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("item");
    app->ffon[1] = ffonElementCreateObject("mykey");
    ffonObjectAddElement(app->ffon[1]->data.object, ffonElementCreateString("child"));
    app->ffonCount = 2;

    app->currentId.ids[0] = 1;

    handleCtrlC_element(app);

    TEST_ASSERT_NOT_NULL(app->clipboard);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, app->clipboard->type);
    TEST_ASSERT_EQUAL_STRING("mykey", app->clipboard->data.object->key);
    TEST_ASSERT_EQUAL_INT(1, app->clipboard->data.object->count);
    TEST_ASSERT_EQUAL_STRING("child", app->clipboard->data.object->elements[0]->data.string);

    destroyTestAppRenderer(app);
}

void test_handleCtrlC_copy_nested_element(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateObject("parent");
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child1"));
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child2"));
    app->ffonCount = 1;

    app->currentId.depth = 2;
    app->currentId.ids[0] = 0;
    app->currentId.ids[1] = 1;

    handleCtrlC_element(app);

    TEST_ASSERT_NOT_NULL(app->clipboard);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, app->clipboard->type);
    TEST_ASSERT_EQUAL_STRING("child2", app->clipboard->data.string);

    destroyTestAppRenderer(app);
}

void test_handleCtrlC_copy_replaces_previous_clipboard(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("first");
    app->ffon[1] = ffonElementCreateString("second");
    app->ffonCount = 2;

    app->currentId.ids[0] = 0;
    handleCtrlC_element(app);
    TEST_ASSERT_EQUAL_STRING("first", app->clipboard->data.string);

    app->currentId.ids[0] = 1;
    handleCtrlC_element(app);
    TEST_ASSERT_EQUAL_STRING("second", app->clipboard->data.string);

    destroyTestAppRenderer(app);
}

void test_handleCtrlC_copy_invalid_index_does_nothing(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateString("only");
    app->ffonCount = 1;

    app->currentId.ids[0] = 5;
    app->needsRedraw = false;

    handleCtrlC_element(app);

    TEST_ASSERT_NULL(app->clipboard);
    TEST_ASSERT_FALSE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

/* ============================================
 * handleCtrlX (cut) element tests
 * ============================================ */

void test_handleCtrlX_cut_removes_element(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 3);
    app->ffon[0] = ffonElementCreateString("first");
    app->ffon[1] = ffonElementCreateString("second");
    app->ffon[2] = ffonElementCreateString("third");
    app->ffonCount = 3;

    app->currentId.ids[0] = 1;

    handleCtrlX_element(app);

    TEST_ASSERT_NOT_NULL(app->clipboard);
    TEST_ASSERT_EQUAL_STRING("second", app->clipboard->data.string);

    TEST_ASSERT_EQUAL_INT(2, app->ffonCount);
    TEST_ASSERT_EQUAL_STRING("first", app->ffon[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("third", app->ffon[1]->data.string);

    TEST_ASSERT_EQUAL_INT(0, app->currentId.ids[0]);
    TEST_ASSERT_TRUE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

void test_handleCtrlX_cut_first_element_cursor_stays(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("first");
    app->ffon[1] = ffonElementCreateString("second");
    app->ffonCount = 2;

    app->currentId.ids[0] = 0;

    handleCtrlX_element(app);

    TEST_ASSERT_EQUAL_STRING("first", app->clipboard->data.string);
    TEST_ASSERT_EQUAL_INT(1, app->ffonCount);
    TEST_ASSERT_EQUAL_STRING("second", app->ffon[0]->data.string);
    TEST_ASSERT_EQUAL_INT(0, app->currentId.ids[0]);

    destroyTestAppRenderer(app);
}

void test_handleCtrlX_cut_last_element(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("first");
    app->ffon[1] = ffonElementCreateString("second");
    app->ffonCount = 2;

    app->currentId.ids[0] = 1;

    handleCtrlX_element(app);

    TEST_ASSERT_EQUAL_STRING("second", app->clipboard->data.string);
    TEST_ASSERT_EQUAL_INT(1, app->ffonCount);
    TEST_ASSERT_EQUAL_STRING("first", app->ffon[0]->data.string);
    TEST_ASSERT_EQUAL_INT(0, app->currentId.ids[0]);

    destroyTestAppRenderer(app);
}

void test_handleCtrlX_cut_object_element(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("string");
    app->ffon[1] = ffonElementCreateObject("myobj");
    ffonObjectAddElement(app->ffon[1]->data.object, ffonElementCreateString("child"));
    app->ffonCount = 2;

    app->currentId.ids[0] = 1;

    handleCtrlX_element(app);

    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, app->clipboard->type);
    TEST_ASSERT_EQUAL_STRING("myobj", app->clipboard->data.object->key);
    TEST_ASSERT_EQUAL_INT(1, app->ffonCount);

    destroyTestAppRenderer(app);
}

void test_handleCtrlX_cut_nested_element(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateObject("parent");
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child1"));
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child2"));
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child3"));
    app->ffonCount = 1;

    app->currentId.depth = 2;
    app->currentId.ids[0] = 0;
    app->currentId.ids[1] = 1;

    handleCtrlX_element(app);

    TEST_ASSERT_EQUAL_STRING("child2", app->clipboard->data.string);
    TEST_ASSERT_EQUAL_INT(2, app->ffon[0]->data.object->count);
    TEST_ASSERT_EQUAL_STRING("child1", app->ffon[0]->data.object->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("child3", app->ffon[0]->data.object->elements[1]->data.string);
    TEST_ASSERT_EQUAL_INT(0, app->currentId.ids[1]);

    destroyTestAppRenderer(app);
}

void test_handleCtrlX_cut_invalid_index_does_nothing(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateString("only");
    app->ffonCount = 1;

    app->currentId.ids[0] = 5;
    app->needsRedraw = false;

    handleCtrlX_element(app);

    TEST_ASSERT_NULL(app->clipboard);
    TEST_ASSERT_EQUAL_INT(1, app->ffonCount);
    TEST_ASSERT_FALSE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

/* ============================================
 * handleCtrlV (paste) element tests
 * ============================================ */

void test_handleCtrlV_paste_replaces_current_element(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("original1");
    app->ffon[1] = ffonElementCreateString("original2");
    app->ffonCount = 2;

    app->clipboard = ffonElementCreateString("pasted");
    app->currentId.ids[0] = 0;

    handleCtrlV_element(app);

    TEST_ASSERT_EQUAL_STRING("pasted", app->ffon[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("original2", app->ffon[1]->data.string);
    TEST_ASSERT_EQUAL_INT(2, app->ffonCount);
    TEST_ASSERT_TRUE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

void test_handleCtrlV_paste_object_element(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateString("to-replace");
    app->ffonCount = 1;

    app->clipboard = ffonElementCreateObject("pastedobj");
    ffonObjectAddElement(app->clipboard->data.object, ffonElementCreateString("child"));

    handleCtrlV_element(app);

    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, app->ffon[0]->type);
    TEST_ASSERT_EQUAL_STRING("pastedobj", app->ffon[0]->data.object->key);
    TEST_ASSERT_EQUAL_INT(1, app->ffon[0]->data.object->count);

    destroyTestAppRenderer(app);
}

void test_handleCtrlV_paste_without_clipboard_does_nothing(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateString("original");
    app->ffonCount = 1;
    app->clipboard = NULL;
    app->needsRedraw = false;

    handleCtrlV_element(app);

    TEST_ASSERT_EQUAL_STRING("original", app->ffon[0]->data.string);
    TEST_ASSERT_FALSE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

void test_handleCtrlV_paste_into_nested_position(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateObject("parent");
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child1"));
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child2"));
    app->ffonCount = 1;

    app->clipboard = ffonElementCreateString("pasted");

    app->currentId.depth = 2;
    app->currentId.ids[0] = 0;
    app->currentId.ids[1] = 1;

    handleCtrlV_element(app);

    TEST_ASSERT_EQUAL_STRING("child1", app->ffon[0]->data.object->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("pasted", app->ffon[0]->data.object->elements[1]->data.string);

    destroyTestAppRenderer(app);
}

void test_handleCtrlV_paste_invalid_index_does_nothing(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateString("original");
    app->ffonCount = 1;

    app->clipboard = ffonElementCreateString("pasted");
    app->currentId.ids[0] = 5;
    app->needsRedraw = false;

    handleCtrlV_element(app);

    TEST_ASSERT_EQUAL_STRING("original", app->ffon[0]->data.string);
    TEST_ASSERT_FALSE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

/* ============================================
 * Integration tests: copy + paste, cut + paste
 * ============================================ */

void test_clipboard_integration_copy_then_paste(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 3);
    app->ffon[0] = ffonElementCreateString("source");
    app->ffon[1] = ffonElementCreateString("target");
    app->ffon[2] = ffonElementCreateString("other");
    app->ffonCount = 3;

    app->currentId.ids[0] = 0;
    handleCtrlC_element(app);

    app->currentId.ids[0] = 1;
    handleCtrlV_element(app);

    TEST_ASSERT_EQUAL_STRING("source", app->ffon[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("source", app->ffon[1]->data.string);
    TEST_ASSERT_EQUAL_STRING("other", app->ffon[2]->data.string);
    TEST_ASSERT_EQUAL_INT(3, app->ffonCount);

    destroyTestAppRenderer(app);
}

void test_clipboard_integration_cut_then_paste(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 3);
    app->ffon[0] = ffonElementCreateString("to-cut");
    app->ffon[1] = ffonElementCreateString("middle");
    app->ffon[2] = ffonElementCreateString("target");
    app->ffonCount = 3;

    app->currentId.ids[0] = 0;
    handleCtrlX_element(app);

    TEST_ASSERT_EQUAL_INT(2, app->ffonCount);
    TEST_ASSERT_EQUAL_INT(0, app->currentId.ids[0]);

    app->currentId.ids[0] = 1;
    handleCtrlV_element(app);

    TEST_ASSERT_EQUAL_STRING("middle", app->ffon[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("to-cut", app->ffon[1]->data.string);
    TEST_ASSERT_EQUAL_INT(2, app->ffonCount);

    destroyTestAppRenderer(app);
}

void test_clipboard_integration_multiple_pastes(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 3);
    app->ffon[0] = ffonElementCreateString("source");
    app->ffon[1] = ffonElementCreateString("target1");
    app->ffon[2] = ffonElementCreateString("target2");
    app->ffonCount = 3;

    app->currentId.ids[0] = 0;
    handleCtrlC_element(app);

    app->currentId.ids[0] = 1;
    handleCtrlV_element(app);

    app->currentId.ids[0] = 2;
    handleCtrlV_element(app);

    TEST_ASSERT_EQUAL_STRING("source", app->ffon[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("source", app->ffon[1]->data.string);
    TEST_ASSERT_EQUAL_STRING("source", app->ffon[2]->data.string);

    destroyTestAppRenderer(app);
}

/* ============================================
 * Custom SDL fakes for text clipboard tests
 * ============================================ */

// Custom fake for SDL_SetClipboardText that captures the string content
static char captured_clipboard_text[1024] = "";

static bool custom_SDL_SetClipboardText(const char *text) {
    if (text) {
        strncpy(captured_clipboard_text, text, sizeof(captured_clipboard_text) - 1);
        captured_clipboard_text[sizeof(captured_clipboard_text) - 1] = '\0';
    }
    return true;
}

// Custom fake for SDL_GetClipboardText that returns a strdup'd string
static char *fake_clipboard_text = NULL;

static char* custom_SDL_GetClipboardText(void) {
    return fake_clipboard_text ? strdup(fake_clipboard_text) : NULL;
}

static bool custom_SDL_HasClipboardText(void) {
    return fake_clipboard_text != NULL && fake_clipboard_text[0] != '\0';
}

// Custom SDL_free that uses regular free (since our mock GetClipboardText uses strdup)
static void custom_SDL_free(void *ptr) {
    free(ptr);
}

/* ============================================
 * Text mode: handleCtrlC (copy) tests
 * ============================================ */

void test_text_handleCtrlC_copies_selection_to_system_clipboard(void) {
    AppRenderer *app = createTestAppRendererWithTextBuffer("hello world");
    app->currentCoordinate = COORDINATE_EDITOR_INSERT;
    app->selectionAnchor = 0;
    app->cursorPosition = 5;

    captured_clipboard_text[0] = '\0';
    SDL_SetClipboardText_fake.custom_fake = custom_SDL_SetClipboardText;

    handleCtrlC_text(app);

    TEST_ASSERT_EQUAL_INT(1, SDL_SetClipboardText_fake.call_count);
    TEST_ASSERT_EQUAL_STRING("hello", captured_clipboard_text);
    // Buffer unchanged
    TEST_ASSERT_EQUAL_STRING("hello world", app->inputBuffer);
    TEST_ASSERT_EQUAL_INT(11, app->inputBufferSize);
    TEST_ASSERT_TRUE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

void test_text_handleCtrlC_no_selection_does_nothing(void) {
    AppRenderer *app = createTestAppRendererWithTextBuffer("hello");
    app->currentCoordinate = COORDINATE_EDITOR_INSERT;
    app->selectionAnchor = -1;
    app->cursorPosition = 3;

    handleCtrlC_text(app);

    TEST_ASSERT_EQUAL_INT(0, SDL_SetClipboardText_fake.call_count);

    destroyTestAppRenderer(app);
}

/* ============================================
 * Text mode: handleCtrlX (cut) tests
 * ============================================ */

void test_text_handleCtrlX_cuts_selection(void) {
    AppRenderer *app = createTestAppRendererWithTextBuffer("hello world");
    app->currentCoordinate = COORDINATE_EDITOR_INSERT;
    app->selectionAnchor = 6;
    app->cursorPosition = 11;

    captured_clipboard_text[0] = '\0';
    SDL_SetClipboardText_fake.custom_fake = custom_SDL_SetClipboardText;

    handleCtrlX_text(app);

    TEST_ASSERT_EQUAL_INT(1, SDL_SetClipboardText_fake.call_count);
    TEST_ASSERT_EQUAL_STRING("world", captured_clipboard_text);
    TEST_ASSERT_EQUAL_STRING("hello ", app->inputBuffer);
    TEST_ASSERT_EQUAL_INT(6, app->inputBufferSize);
    TEST_ASSERT_EQUAL_INT(6, app->cursorPosition);
    TEST_ASSERT_TRUE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

void test_text_handleCtrlX_no_selection_does_nothing(void) {
    AppRenderer *app = createTestAppRendererWithTextBuffer("hello");
    app->currentCoordinate = COORDINATE_EDITOR_INSERT;
    app->selectionAnchor = -1;
    app->cursorPosition = 3;
    app->needsRedraw = false;

    handleCtrlX_text(app);

    TEST_ASSERT_EQUAL_INT(0, SDL_SetClipboardText_fake.call_count);
    TEST_ASSERT_EQUAL_STRING("hello", app->inputBuffer);
    TEST_ASSERT_FALSE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

void test_text_handleCtrlX_cuts_middle_selection(void) {
    AppRenderer *app = createTestAppRendererWithTextBuffer("abcdefgh");
    app->currentCoordinate = COORDINATE_EDITOR_INSERT;
    app->selectionAnchor = 2;
    app->cursorPosition = 5;

    captured_clipboard_text[0] = '\0';
    SDL_SetClipboardText_fake.custom_fake = custom_SDL_SetClipboardText;

    handleCtrlX_text(app);

    TEST_ASSERT_EQUAL_STRING("cde", captured_clipboard_text);
    TEST_ASSERT_EQUAL_STRING("abfgh", app->inputBuffer);
    TEST_ASSERT_EQUAL_INT(5, app->inputBufferSize);
    TEST_ASSERT_EQUAL_INT(2, app->cursorPosition);

    destroyTestAppRenderer(app);
}

/* ============================================
 * Text mode: handleCtrlV (paste) tests
 * ============================================ */

void test_text_handleCtrlV_pastes_at_cursor(void) {
    AppRenderer *app = createTestAppRendererWithTextBuffer("hello");
    app->currentCoordinate = COORDINATE_EDITOR_INSERT;
    app->cursorPosition = 5;

    fake_clipboard_text = " world";
    SDL_GetClipboardText_fake.custom_fake = custom_SDL_GetClipboardText;
    SDL_HasClipboardText_fake.custom_fake = custom_SDL_HasClipboardText;
    SDL_free_fake.custom_fake = custom_SDL_free;

    handleCtrlV_text(app);

    TEST_ASSERT_EQUAL_STRING("hello world", app->inputBuffer);
    TEST_ASSERT_EQUAL_INT(11, app->inputBufferSize);
    TEST_ASSERT_EQUAL_INT(11, app->cursorPosition);
    TEST_ASSERT_TRUE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

void test_text_handleCtrlV_pastes_at_beginning(void) {
    AppRenderer *app = createTestAppRendererWithTextBuffer("world");
    app->currentCoordinate = COORDINATE_EDITOR_INSERT;
    app->cursorPosition = 0;

    fake_clipboard_text = "hello ";
    SDL_GetClipboardText_fake.custom_fake = custom_SDL_GetClipboardText;
    SDL_HasClipboardText_fake.custom_fake = custom_SDL_HasClipboardText;
    SDL_free_fake.custom_fake = custom_SDL_free;

    handleCtrlV_text(app);

    TEST_ASSERT_EQUAL_STRING("hello world", app->inputBuffer);
    TEST_ASSERT_EQUAL_INT(11, app->inputBufferSize);
    TEST_ASSERT_EQUAL_INT(6, app->cursorPosition);

    destroyTestAppRenderer(app);
}

void test_text_handleCtrlV_replaces_selection(void) {
    AppRenderer *app = createTestAppRendererWithTextBuffer("hello world");
    app->currentCoordinate = COORDINATE_EDITOR_INSERT;
    app->selectionAnchor = 6;
    app->cursorPosition = 11;

    fake_clipboard_text = "earth";
    SDL_GetClipboardText_fake.custom_fake = custom_SDL_GetClipboardText;
    SDL_HasClipboardText_fake.custom_fake = custom_SDL_HasClipboardText;
    SDL_free_fake.custom_fake = custom_SDL_free;

    handleCtrlV_text(app);

    TEST_ASSERT_EQUAL_STRING("hello earth", app->inputBuffer);
    TEST_ASSERT_EQUAL_INT(11, app->inputBufferSize);

    destroyTestAppRenderer(app);
}

void test_text_handleCtrlV_no_clipboard_does_nothing(void) {
    AppRenderer *app = createTestAppRendererWithTextBuffer("hello");
    app->currentCoordinate = COORDINATE_EDITOR_INSERT;
    app->cursorPosition = 5;
    app->needsRedraw = false;

    fake_clipboard_text = NULL;
    SDL_HasClipboardText_fake.custom_fake = custom_SDL_HasClipboardText;

    handleCtrlV_text(app);

    TEST_ASSERT_EQUAL_STRING("hello", app->inputBuffer);
    TEST_ASSERT_FALSE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

/* ============================================
 * Main - Run all tests
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // handleCtrlC (copy) element tests
    RUN_TEST(test_handleCtrlC_copy_string_element);
    RUN_TEST(test_handleCtrlC_copy_second_element);
    RUN_TEST(test_handleCtrlC_copy_object_element);
    RUN_TEST(test_handleCtrlC_copy_nested_element);
    RUN_TEST(test_handleCtrlC_copy_replaces_previous_clipboard);
    RUN_TEST(test_handleCtrlC_copy_invalid_index_does_nothing);

    // handleCtrlX (cut) element tests
    RUN_TEST(test_handleCtrlX_cut_removes_element);
    RUN_TEST(test_handleCtrlX_cut_first_element_cursor_stays);
    RUN_TEST(test_handleCtrlX_cut_last_element);
    RUN_TEST(test_handleCtrlX_cut_object_element);
    RUN_TEST(test_handleCtrlX_cut_nested_element);
    RUN_TEST(test_handleCtrlX_cut_invalid_index_does_nothing);

    // handleCtrlV (paste) element tests
    RUN_TEST(test_handleCtrlV_paste_replaces_current_element);
    RUN_TEST(test_handleCtrlV_paste_object_element);
    RUN_TEST(test_handleCtrlV_paste_without_clipboard_does_nothing);
    RUN_TEST(test_handleCtrlV_paste_into_nested_position);
    RUN_TEST(test_handleCtrlV_paste_invalid_index_does_nothing);

    // Integration tests
    RUN_TEST(test_clipboard_integration_copy_then_paste);
    RUN_TEST(test_clipboard_integration_cut_then_paste);
    RUN_TEST(test_clipboard_integration_multiple_pastes);

    // Text mode: copy tests
    RUN_TEST(test_text_handleCtrlC_copies_selection_to_system_clipboard);
    RUN_TEST(test_text_handleCtrlC_no_selection_does_nothing);

    // Text mode: cut tests
    RUN_TEST(test_text_handleCtrlX_cuts_selection);
    RUN_TEST(test_text_handleCtrlX_no_selection_does_nothing);
    RUN_TEST(test_text_handleCtrlX_cuts_middle_selection);

    // Text mode: paste tests
    RUN_TEST(test_text_handleCtrlV_pastes_at_cursor);
    RUN_TEST(test_text_handleCtrlV_pastes_at_beginning);
    RUN_TEST(test_text_handleCtrlV_replaces_selection);
    RUN_TEST(test_text_handleCtrlV_no_clipboard_does_nothing);

    return UNITY_END();
}

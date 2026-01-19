/*
 * Tests for clipboard operations: handleCtrlX (cut), handleCtrlC (copy), handleCtrlV (paste)
 */

#include <tau/tau.h>
#include <fff/fff.h>
#include <stdlib.h>
#include <string.h>

DEFINE_FFF_GLOBALS;

// Mock updateHistory since we don't want to test history tracking here
FAKE_VOID_FUNC(updateHistory, void*, int, bool, const char*, int);

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

typedef struct {
    IdArray id;
    Task task;
    char *line;
    bool isKey;
} UndoEntry;

typedef struct {
    FfonElement **ffon;
    int ffonCount;
    int ffonCapacity;

    IdArray currentId;
    IdArray previousId;

    FfonElement *clipboard;

    UndoEntry *undoHistory;
    int undoHistoryCount;
    int undoPosition;

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

// Implementation of the functions under test (copied from update.c)
static void handleCtrlX(AppRenderer *appRenderer) {
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

static void handleCtrlC(AppRenderer *appRenderer) {
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

static void handleCtrlV(AppRenderer *appRenderer) {
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

// Helper to create a test AppRenderer with basic structure
static AppRenderer* createTestAppRenderer(void) {
    AppRenderer *app = calloc(1, sizeof(AppRenderer));
    app->currentId.depth = 1;
    app->currentId.ids[0] = 0;
    app->previousId.depth = 1;
    app->previousId.ids[0] = 0;
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
    free(app);
}

TAU_MAIN()

// ============================================
// handleCtrlC (copy) tests
// ============================================

TEST(handleCtrlC, copy_string_element) {
    AppRenderer *app = createTestAppRenderer();

    // Create a simple list: ["hello", "world"]
    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("hello");
    app->ffon[1] = ffonElementCreateString("world");
    app->ffonCount = 2;

    app->currentId.ids[0] = 0;

    handleCtrlC(app);

    CHECK(app->clipboard != NULL);
    CHECK_EQ(app->clipboard->type, FFON_STRING);
    CHECK_STREQ(app->clipboard->data.string, "hello");
    CHECK_EQ(app->ffonCount, 2);  // Original not modified
    CHECK(app->needsRedraw);

    destroyTestAppRenderer(app);
}

TEST(handleCtrlC, copy_second_element) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("first");
    app->ffon[1] = ffonElementCreateString("second");
    app->ffonCount = 2;

    app->currentId.ids[0] = 1;

    handleCtrlC(app);

    CHECK(app->clipboard != NULL);
    CHECK_STREQ(app->clipboard->data.string, "second");
    CHECK_EQ(app->ffonCount, 2);

    destroyTestAppRenderer(app);
}

TEST(handleCtrlC, copy_object_element) {
    AppRenderer *app = createTestAppRenderer();

    // Create: ["item", {key: ["child"]}]
    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("item");
    app->ffon[1] = ffonElementCreateObject("mykey");
    ffonObjectAddElement(app->ffon[1]->data.object, ffonElementCreateString("child"));
    app->ffonCount = 2;

    app->currentId.ids[0] = 1;

    handleCtrlC(app);

    CHECK(app->clipboard != NULL);
    CHECK_EQ(app->clipboard->type, FFON_OBJECT);
    CHECK_STREQ(app->clipboard->data.object->key, "mykey");
    CHECK_EQ(app->clipboard->data.object->count, 1);
    CHECK_STREQ(app->clipboard->data.object->elements[0]->data.string, "child");

    destroyTestAppRenderer(app);
}

TEST(handleCtrlC, copy_nested_element) {
    AppRenderer *app = createTestAppRenderer();

    // Create: [{parent: ["child1", "child2"]}]
    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateObject("parent");
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child1"));
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child2"));
    app->ffonCount = 1;

    // Navigate to child2 (id = [0, 1])
    app->currentId.depth = 2;
    app->currentId.ids[0] = 0;
    app->currentId.ids[1] = 1;

    handleCtrlC(app);

    CHECK(app->clipboard != NULL);
    CHECK_EQ(app->clipboard->type, FFON_STRING);
    CHECK_STREQ(app->clipboard->data.string, "child2");

    destroyTestAppRenderer(app);
}

TEST(handleCtrlC, copy_replaces_previous_clipboard) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("first");
    app->ffon[1] = ffonElementCreateString("second");
    app->ffonCount = 2;

    // First copy
    app->currentId.ids[0] = 0;
    handleCtrlC(app);
    CHECK_STREQ(app->clipboard->data.string, "first");

    // Second copy should replace
    app->currentId.ids[0] = 1;
    handleCtrlC(app);
    CHECK_STREQ(app->clipboard->data.string, "second");

    destroyTestAppRenderer(app);
}

TEST(handleCtrlC, copy_invalid_index_does_nothing) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateString("only");
    app->ffonCount = 1;

    app->currentId.ids[0] = 5;  // Out of bounds
    app->needsRedraw = false;

    handleCtrlC(app);

    CHECK(app->clipboard == NULL);
    CHECK(!app->needsRedraw);

    destroyTestAppRenderer(app);
}

// ============================================
// handleCtrlX (cut) tests
// ============================================

TEST(handleCtrlX, cut_removes_element) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 3);
    app->ffon[0] = ffonElementCreateString("first");
    app->ffon[1] = ffonElementCreateString("second");
    app->ffon[2] = ffonElementCreateString("third");
    app->ffonCount = 3;

    app->currentId.ids[0] = 1;

    handleCtrlX(app);

    // Check clipboard has the cut element
    CHECK(app->clipboard != NULL);
    CHECK_STREQ(app->clipboard->data.string, "second");

    // Check element was removed
    CHECK_EQ(app->ffonCount, 2);
    CHECK_STREQ(app->ffon[0]->data.string, "first");
    CHECK_STREQ(app->ffon[1]->data.string, "third");

    // Cursor moved back
    CHECK_EQ(app->currentId.ids[0], 0);
    CHECK(app->needsRedraw);

    destroyTestAppRenderer(app);
}

TEST(handleCtrlX, cut_first_element_cursor_stays) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("first");
    app->ffon[1] = ffonElementCreateString("second");
    app->ffonCount = 2;

    app->currentId.ids[0] = 0;

    handleCtrlX(app);

    CHECK_STREQ(app->clipboard->data.string, "first");
    CHECK_EQ(app->ffonCount, 1);
    CHECK_STREQ(app->ffon[0]->data.string, "second");
    CHECK_EQ(app->currentId.ids[0], 0);  // Stays at 0

    destroyTestAppRenderer(app);
}

TEST(handleCtrlX, cut_last_element) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("first");
    app->ffon[1] = ffonElementCreateString("second");
    app->ffonCount = 2;

    app->currentId.ids[0] = 1;

    handleCtrlX(app);

    CHECK_STREQ(app->clipboard->data.string, "second");
    CHECK_EQ(app->ffonCount, 1);
    CHECK_STREQ(app->ffon[0]->data.string, "first");
    CHECK_EQ(app->currentId.ids[0], 0);  // Moved back

    destroyTestAppRenderer(app);
}

TEST(handleCtrlX, cut_object_element) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("string");
    app->ffon[1] = ffonElementCreateObject("myobj");
    ffonObjectAddElement(app->ffon[1]->data.object, ffonElementCreateString("child"));
    app->ffonCount = 2;

    app->currentId.ids[0] = 1;

    handleCtrlX(app);

    CHECK_EQ(app->clipboard->type, FFON_OBJECT);
    CHECK_STREQ(app->clipboard->data.object->key, "myobj");
    CHECK_EQ(app->ffonCount, 1);

    destroyTestAppRenderer(app);
}

TEST(handleCtrlX, cut_nested_element) {
    AppRenderer *app = createTestAppRenderer();

    // Create: [{parent: ["child1", "child2", "child3"]}]
    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateObject("parent");
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child1"));
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child2"));
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child3"));
    app->ffonCount = 1;

    // Navigate to child2 (id = [0, 1])
    app->currentId.depth = 2;
    app->currentId.ids[0] = 0;
    app->currentId.ids[1] = 1;

    handleCtrlX(app);

    CHECK_STREQ(app->clipboard->data.string, "child2");
    CHECK_EQ(app->ffon[0]->data.object->count, 2);
    CHECK_STREQ(app->ffon[0]->data.object->elements[0]->data.string, "child1");
    CHECK_STREQ(app->ffon[0]->data.object->elements[1]->data.string, "child3");
    CHECK_EQ(app->currentId.ids[1], 0);  // Cursor moved back

    destroyTestAppRenderer(app);
}

TEST(handleCtrlX, cut_invalid_index_does_nothing) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateString("only");
    app->ffonCount = 1;

    app->currentId.ids[0] = 5;  // Out of bounds
    app->needsRedraw = false;

    handleCtrlX(app);

    CHECK(app->clipboard == NULL);
    CHECK_EQ(app->ffonCount, 1);
    CHECK(!app->needsRedraw);

    destroyTestAppRenderer(app);
}

// ============================================
// handleCtrlV (paste) tests
// ============================================

TEST(handleCtrlV, paste_replaces_current_element) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("original1");
    app->ffon[1] = ffonElementCreateString("original2");
    app->ffonCount = 2;

    app->clipboard = ffonElementCreateString("pasted");
    app->currentId.ids[0] = 0;

    handleCtrlV(app);

    CHECK_STREQ(app->ffon[0]->data.string, "pasted");
    CHECK_STREQ(app->ffon[1]->data.string, "original2");
    CHECK_EQ(app->ffonCount, 2);
    CHECK(app->needsRedraw);

    destroyTestAppRenderer(app);
}

TEST(handleCtrlV, paste_object_element) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateString("to-replace");
    app->ffonCount = 1;

    app->clipboard = ffonElementCreateObject("pastedobj");
    ffonObjectAddElement(app->clipboard->data.object, ffonElementCreateString("child"));

    handleCtrlV(app);

    CHECK_EQ(app->ffon[0]->type, FFON_OBJECT);
    CHECK_STREQ(app->ffon[0]->data.object->key, "pastedobj");
    CHECK_EQ(app->ffon[0]->data.object->count, 1);

    destroyTestAppRenderer(app);
}

TEST(handleCtrlV, paste_without_clipboard_does_nothing) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateString("original");
    app->ffonCount = 1;
    app->clipboard = NULL;
    app->needsRedraw = false;

    handleCtrlV(app);

    CHECK_STREQ(app->ffon[0]->data.string, "original");
    CHECK(!app->needsRedraw);

    destroyTestAppRenderer(app);
}

TEST(handleCtrlV, paste_into_nested_position) {
    AppRenderer *app = createTestAppRenderer();

    // Create: [{parent: ["child1", "child2"]}]
    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateObject("parent");
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child1"));
    ffonObjectAddElement(app->ffon[0]->data.object, ffonElementCreateString("child2"));
    app->ffonCount = 1;

    app->clipboard = ffonElementCreateString("pasted");

    // Navigate to child2 (id = [0, 1])
    app->currentId.depth = 2;
    app->currentId.ids[0] = 0;
    app->currentId.ids[1] = 1;

    handleCtrlV(app);

    CHECK_STREQ(app->ffon[0]->data.object->elements[0]->data.string, "child1");
    CHECK_STREQ(app->ffon[0]->data.object->elements[1]->data.string, "pasted");

    destroyTestAppRenderer(app);
}

TEST(handleCtrlV, paste_invalid_index_does_nothing) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateString("original");
    app->ffonCount = 1;

    app->clipboard = ffonElementCreateString("pasted");
    app->currentId.ids[0] = 5;  // Out of bounds
    app->needsRedraw = false;

    handleCtrlV(app);

    CHECK_STREQ(app->ffon[0]->data.string, "original");
    CHECK(!app->needsRedraw);

    destroyTestAppRenderer(app);
}

// ============================================
// Integration tests: copy + paste, cut + paste
// ============================================

TEST(clipboard_integration, copy_then_paste) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 3);
    app->ffon[0] = ffonElementCreateString("source");
    app->ffon[1] = ffonElementCreateString("target");
    app->ffon[2] = ffonElementCreateString("other");
    app->ffonCount = 3;

    // Copy from index 0
    app->currentId.ids[0] = 0;
    handleCtrlC(app);

    // Paste to index 1
    app->currentId.ids[0] = 1;
    handleCtrlV(app);

    CHECK_STREQ(app->ffon[0]->data.string, "source");
    CHECK_STREQ(app->ffon[1]->data.string, "source");  // Pasted
    CHECK_STREQ(app->ffon[2]->data.string, "other");
    CHECK_EQ(app->ffonCount, 3);

    destroyTestAppRenderer(app);
}

TEST(clipboard_integration, cut_then_paste) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 3);
    app->ffon[0] = ffonElementCreateString("to-cut");
    app->ffon[1] = ffonElementCreateString("middle");
    app->ffon[2] = ffonElementCreateString("target");
    app->ffonCount = 3;

    // Cut from index 0
    app->currentId.ids[0] = 0;
    handleCtrlX(app);

    // Now we have ["middle", "target"], cursor at 0
    CHECK_EQ(app->ffonCount, 2);
    CHECK_EQ(app->currentId.ids[0], 0);

    // Paste to index 1
    app->currentId.ids[0] = 1;
    handleCtrlV(app);

    CHECK_STREQ(app->ffon[0]->data.string, "middle");
    CHECK_STREQ(app->ffon[1]->data.string, "to-cut");  // Pasted
    CHECK_EQ(app->ffonCount, 2);

    destroyTestAppRenderer(app);
}

TEST(clipboard_integration, multiple_pastes) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 3);
    app->ffon[0] = ffonElementCreateString("source");
    app->ffon[1] = ffonElementCreateString("target1");
    app->ffon[2] = ffonElementCreateString("target2");
    app->ffonCount = 3;

    // Copy from index 0
    app->currentId.ids[0] = 0;
    handleCtrlC(app);

    // Paste to index 1
    app->currentId.ids[0] = 1;
    handleCtrlV(app);

    // Paste to index 2
    app->currentId.ids[0] = 2;
    handleCtrlV(app);

    CHECK_STREQ(app->ffon[0]->data.string, "source");
    CHECK_STREQ(app->ffon[1]->data.string, "source");
    CHECK_STREQ(app->ffon[2]->data.string, "source");

    destroyTestAppRenderer(app);
}

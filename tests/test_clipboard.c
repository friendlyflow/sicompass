/*
 * Tests for clipboard operations: handleCtrlX (cut), handleCtrlC (copy), handleCtrlV (paste)
 */

#include <unity.h>
#include <fff/fff.h>
#include <stdbool.h>
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

/* ============================================
 * Unity Test Setup/Teardown
 * ============================================ */

void setUp(void) {
    RESET_FAKE(updateHistory);
    FFF_RESET_HISTORY();
}

void tearDown(void) {
}

/* ============================================
 * handleCtrlC (copy) tests
 * ============================================ */

void test_handleCtrlC_copy_string_element(void) {
    AppRenderer *app = createTestAppRenderer();

    // Create a simple list: ["hello", "world"]
    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("hello");
    app->ffon[1] = ffonElementCreateString("world");
    app->ffonCount = 2;

    app->currentId.ids[0] = 0;

    handleCtrlC(app);

    TEST_ASSERT_NOT_NULL(app->clipboard);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, app->clipboard->type);
    TEST_ASSERT_EQUAL_STRING("hello", app->clipboard->data.string);
    TEST_ASSERT_EQUAL_INT(2, app->ffonCount);  // Original not modified
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

    handleCtrlC(app);

    TEST_ASSERT_NOT_NULL(app->clipboard);
    TEST_ASSERT_EQUAL_STRING("second", app->clipboard->data.string);
    TEST_ASSERT_EQUAL_INT(2, app->ffonCount);

    destroyTestAppRenderer(app);
}

void test_handleCtrlC_copy_object_element(void) {
    AppRenderer *app = createTestAppRenderer();

    // Create: ["item", {key: ["child"]}]
    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("item");
    app->ffon[1] = ffonElementCreateObject("mykey");
    ffonObjectAddElement(app->ffon[1]->data.object, ffonElementCreateString("child"));
    app->ffonCount = 2;

    app->currentId.ids[0] = 1;

    handleCtrlC(app);

    TEST_ASSERT_NOT_NULL(app->clipboard);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, app->clipboard->type);
    TEST_ASSERT_EQUAL_STRING("mykey", app->clipboard->data.object->key);
    TEST_ASSERT_EQUAL_INT(1, app->clipboard->data.object->count);
    TEST_ASSERT_EQUAL_STRING("child", app->clipboard->data.object->elements[0]->data.string);

    destroyTestAppRenderer(app);
}

void test_handleCtrlC_copy_nested_element(void) {
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

    // First copy
    app->currentId.ids[0] = 0;
    handleCtrlC(app);
    TEST_ASSERT_EQUAL_STRING("first", app->clipboard->data.string);

    // Second copy should replace
    app->currentId.ids[0] = 1;
    handleCtrlC(app);
    TEST_ASSERT_EQUAL_STRING("second", app->clipboard->data.string);

    destroyTestAppRenderer(app);
}

void test_handleCtrlC_copy_invalid_index_does_nothing(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateString("only");
    app->ffonCount = 1;

    app->currentId.ids[0] = 5;  // Out of bounds
    app->needsRedraw = false;

    handleCtrlC(app);

    TEST_ASSERT_NULL(app->clipboard);
    TEST_ASSERT_FALSE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

/* ============================================
 * handleCtrlX (cut) tests
 * ============================================ */

void test_handleCtrlX_cut_removes_element(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 3);
    app->ffon[0] = ffonElementCreateString("first");
    app->ffon[1] = ffonElementCreateString("second");
    app->ffon[2] = ffonElementCreateString("third");
    app->ffonCount = 3;

    app->currentId.ids[0] = 1;

    handleCtrlX(app);

    // Check clipboard has the cut element
    TEST_ASSERT_NOT_NULL(app->clipboard);
    TEST_ASSERT_EQUAL_STRING("second", app->clipboard->data.string);

    // Check element was removed
    TEST_ASSERT_EQUAL_INT(2, app->ffonCount);
    TEST_ASSERT_EQUAL_STRING("first", app->ffon[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("third", app->ffon[1]->data.string);

    // Cursor moved back
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

    handleCtrlX(app);

    TEST_ASSERT_EQUAL_STRING("first", app->clipboard->data.string);
    TEST_ASSERT_EQUAL_INT(1, app->ffonCount);
    TEST_ASSERT_EQUAL_STRING("second", app->ffon[0]->data.string);
    TEST_ASSERT_EQUAL_INT(0, app->currentId.ids[0]);  // Stays at 0

    destroyTestAppRenderer(app);
}

void test_handleCtrlX_cut_last_element(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("first");
    app->ffon[1] = ffonElementCreateString("second");
    app->ffonCount = 2;

    app->currentId.ids[0] = 1;

    handleCtrlX(app);

    TEST_ASSERT_EQUAL_STRING("second", app->clipboard->data.string);
    TEST_ASSERT_EQUAL_INT(1, app->ffonCount);
    TEST_ASSERT_EQUAL_STRING("first", app->ffon[0]->data.string);
    TEST_ASSERT_EQUAL_INT(0, app->currentId.ids[0]);  // Moved back

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

    handleCtrlX(app);

    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, app->clipboard->type);
    TEST_ASSERT_EQUAL_STRING("myobj", app->clipboard->data.object->key);
    TEST_ASSERT_EQUAL_INT(1, app->ffonCount);

    destroyTestAppRenderer(app);
}

void test_handleCtrlX_cut_nested_element(void) {
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

    TEST_ASSERT_EQUAL_STRING("child2", app->clipboard->data.string);
    TEST_ASSERT_EQUAL_INT(2, app->ffon[0]->data.object->count);
    TEST_ASSERT_EQUAL_STRING("child1", app->ffon[0]->data.object->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("child3", app->ffon[0]->data.object->elements[1]->data.string);
    TEST_ASSERT_EQUAL_INT(0, app->currentId.ids[1]);  // Cursor moved back

    destroyTestAppRenderer(app);
}

void test_handleCtrlX_cut_invalid_index_does_nothing(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*));
    app->ffon[0] = ffonElementCreateString("only");
    app->ffonCount = 1;

    app->currentId.ids[0] = 5;  // Out of bounds
    app->needsRedraw = false;

    handleCtrlX(app);

    TEST_ASSERT_NULL(app->clipboard);
    TEST_ASSERT_EQUAL_INT(1, app->ffonCount);
    TEST_ASSERT_FALSE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

/* ============================================
 * handleCtrlV (paste) tests
 * ============================================ */

void test_handleCtrlV_paste_replaces_current_element(void) {
    AppRenderer *app = createTestAppRenderer();

    app->ffon = malloc(sizeof(FfonElement*) * 2);
    app->ffon[0] = ffonElementCreateString("original1");
    app->ffon[1] = ffonElementCreateString("original2");
    app->ffonCount = 2;

    app->clipboard = ffonElementCreateString("pasted");
    app->currentId.ids[0] = 0;

    handleCtrlV(app);

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

    handleCtrlV(app);

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

    handleCtrlV(app);

    TEST_ASSERT_EQUAL_STRING("original", app->ffon[0]->data.string);
    TEST_ASSERT_FALSE(app->needsRedraw);

    destroyTestAppRenderer(app);
}

void test_handleCtrlV_paste_into_nested_position(void) {
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
    app->currentId.ids[0] = 5;  // Out of bounds
    app->needsRedraw = false;

    handleCtrlV(app);

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

    // Copy from index 0
    app->currentId.ids[0] = 0;
    handleCtrlC(app);

    // Paste to index 1
    app->currentId.ids[0] = 1;
    handleCtrlV(app);

    TEST_ASSERT_EQUAL_STRING("source", app->ffon[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("source", app->ffon[1]->data.string);  // Pasted
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

    // Cut from index 0
    app->currentId.ids[0] = 0;
    handleCtrlX(app);

    // Now we have ["middle", "target"], cursor at 0
    TEST_ASSERT_EQUAL_INT(2, app->ffonCount);
    TEST_ASSERT_EQUAL_INT(0, app->currentId.ids[0]);

    // Paste to index 1
    app->currentId.ids[0] = 1;
    handleCtrlV(app);

    TEST_ASSERT_EQUAL_STRING("middle", app->ffon[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("to-cut", app->ffon[1]->data.string);  // Pasted
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

    // Copy from index 0
    app->currentId.ids[0] = 0;
    handleCtrlC(app);

    // Paste to index 1
    app->currentId.ids[0] = 1;
    handleCtrlV(app);

    // Paste to index 2
    app->currentId.ids[0] = 2;
    handleCtrlV(app);

    TEST_ASSERT_EQUAL_STRING("source", app->ffon[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("source", app->ffon[1]->data.string);
    TEST_ASSERT_EQUAL_STRING("source", app->ffon[2]->data.string);

    destroyTestAppRenderer(app);
}

/* ============================================
 * Main - Run all tests
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // handleCtrlC (copy) tests
    RUN_TEST(test_handleCtrlC_copy_string_element);
    RUN_TEST(test_handleCtrlC_copy_second_element);
    RUN_TEST(test_handleCtrlC_copy_object_element);
    RUN_TEST(test_handleCtrlC_copy_nested_element);
    RUN_TEST(test_handleCtrlC_copy_replaces_previous_clipboard);
    RUN_TEST(test_handleCtrlC_copy_invalid_index_does_nothing);

    // handleCtrlX (cut) tests
    RUN_TEST(test_handleCtrlX_cut_removes_element);
    RUN_TEST(test_handleCtrlX_cut_first_element_cursor_stays);
    RUN_TEST(test_handleCtrlX_cut_last_element);
    RUN_TEST(test_handleCtrlX_cut_object_element);
    RUN_TEST(test_handleCtrlX_cut_nested_element);
    RUN_TEST(test_handleCtrlX_cut_invalid_index_does_nothing);

    // handleCtrlV (paste) tests
    RUN_TEST(test_handleCtrlV_paste_replaces_current_element);
    RUN_TEST(test_handleCtrlV_paste_object_element);
    RUN_TEST(test_handleCtrlV_paste_without_clipboard_does_nothing);
    RUN_TEST(test_handleCtrlV_paste_into_nested_position);
    RUN_TEST(test_handleCtrlV_paste_invalid_index_does_nothing);

    // Integration tests
    RUN_TEST(test_clipboard_integration_copy_then_paste);
    RUN_TEST(test_clipboard_integration_cut_then_paste);
    RUN_TEST(test_clipboard_integration_multiple_pastes);

    return UNITY_END();
}

/*
 * Tests for update.c functions:
 * - updateHistory (undo history management)
 * - updateFfon (FFON tree modification)
 * - updateIds (ID array navigation updates)
 * - stripTrailingColon helper
 */

#include <unity.h>
#include <fff/fff.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

DEFINE_FFF_GLOBALS;

/* ============================================
 * Type definitions
 * ============================================ */

#define MAX_ID_DEPTH 32
#define MAX_LINE_LENGTH 4096
#define UNDO_HISTORY_SIZE 500

typedef enum { FFON_STRING, FFON_OBJECT } FfonType;

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
    HISTORY_NONE,
    HISTORY_UNDO,
    HISTORY_REDO
} History;

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
    IdArray currentInsertId;

    UndoEntry *undoHistory;
    int undoHistoryCount;
    int undoPosition;

    char *inputBuffer;
    int inputBufferSize;
    int inputBufferCapacity;
    int cursorPosition;

    bool needsRedraw;
} AppRenderer;

/* ============================================
 * FFON helpers (from test_clipboard pattern)
 * ============================================ */

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

static void ffonObjectInsertElement(FfonObject *obj, FfonElement *elem, int index) {
    if (obj->count >= obj->capacity) {
        obj->capacity = obj->capacity == 0 ? 4 : obj->capacity * 2;
        obj->elements = realloc(obj->elements, sizeof(FfonElement*) * obj->capacity);
    }
    for (int i = obj->count; i > index; i--) {
        obj->elements[i] = obj->elements[i - 1];
    }
    obj->elements[index] = elem;
    obj->count++;
}

/* ============================================
 * IdArray helpers
 * ============================================ */

static void idArrayInit(IdArray *arr) {
    memset(arr, 0, sizeof(IdArray));
}

static void idArrayCopy(IdArray *dst, const IdArray *src) {
    memcpy(dst, src, sizeof(IdArray));
}

static bool idArrayEqual(const IdArray *a, const IdArray *b) {
    if (a->depth != b->depth) return false;
    for (int i = 0; i < a->depth; i++) {
        if (a->ids[i] != b->ids[i]) return false;
    }
    return true;
}

static void idArrayPush(IdArray *arr, int val) {
    if (arr->depth < MAX_ID_DEPTH) {
        arr->ids[arr->depth++] = val;
    }
}

/* ============================================
 * getFfonAtId (from lib_ffon)
 * ============================================ */

static FfonElement** getFfonAtId(FfonElement **ffon, int ffonCount, const IdArray *id, int *outCount) {
    *outCount = 0;
    if (!ffon || !id || id->depth < 1) return NULL;

    FfonElement **current = ffon;
    int currentCount = ffonCount;

    for (int i = 0; i < id->depth - 1; i++) {
        int idx = id->ids[i];
        if (idx < 0 || idx >= currentCount) return NULL;
        if (current[idx]->type != FFON_OBJECT) return NULL;
        FfonObject *obj = current[idx]->data.object;
        current = obj->elements;
        currentCount = obj->count;
    }

    *outCount = currentCount;
    return current;
}

/* ============================================
 * Functions under test (from update.c)
 * ============================================ */

static char* stripTrailingColon(const char *line) {
    if (!line) return strdup("");

    size_t len = strlen(line);
    if (len > 0 && line[len - 1] == ':') {
        char *result = malloc(len);
        if (result) {
            strncpy(result, line, len - 1);
            result[len - 1] = '\0';
        }
        return result;
    }
    return strdup(line);
}

void updateHistory(AppRenderer *appRenderer, Task task, const IdArray *id,
                   FfonElement *prevElement, FfonElement *newElement, History history) {
    if (history != HISTORY_NONE) return;
    if (task == TASK_NONE || task == TASK_K_ARROW_UP || task == TASK_J_ARROW_DOWN ||
        task == TASK_H_ARROW_LEFT || task == TASK_L_ARROW_RIGHT ||
        task == TASK_COPY) return;

    // Trim redo entries beyond current position
    for (int i = appRenderer->undoPosition; i < appRenderer->undoHistoryCount; i++) {
        if (appRenderer->undoHistory[i].prevElement)
            ffonElementDestroy(appRenderer->undoHistory[i].prevElement);
        if (appRenderer->undoHistory[i].newElement)
            ffonElementDestroy(appRenderer->undoHistory[i].newElement);
    }
    appRenderer->undoHistoryCount = appRenderer->undoPosition;

    if (appRenderer->undoHistoryCount >= UNDO_HISTORY_SIZE) {
        // Shift history
        if (appRenderer->undoHistory[0].prevElement)
            ffonElementDestroy(appRenderer->undoHistory[0].prevElement);
        if (appRenderer->undoHistory[0].newElement)
            ffonElementDestroy(appRenderer->undoHistory[0].newElement);
        memmove(&appRenderer->undoHistory[0], &appRenderer->undoHistory[1],
                sizeof(UndoEntry) * (UNDO_HISTORY_SIZE - 1));
        appRenderer->undoHistoryCount--;
    }

    int pos = appRenderer->undoHistoryCount;
    idArrayCopy(&appRenderer->undoHistory[pos].id, id);
    appRenderer->undoHistory[pos].task = task;
    appRenderer->undoHistory[pos].prevElement = prevElement ? ffonElementClone(prevElement) : NULL;
    appRenderer->undoHistory[pos].newElement = newElement ? ffonElementClone(newElement) : NULL;

    appRenderer->undoHistoryCount++;
    appRenderer->undoPosition = appRenderer->undoHistoryCount;
}

void updateFfon(AppRenderer *appRenderer, const char *line, bool isKey, Task task, History history) {
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                     &appRenderer->currentId, &count);
    if (!arr || count == 0) return;

    int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (idx < 0 || idx >= count) return;

    FfonElement *elem = arr[idx];

    if (task == TASK_INPUT) {
        if (isKey) {
            if (elem->type == FFON_OBJECT) {
                char *newKey = malloc(strlen(line) + 2);
                sprintf(newKey, "%s:", line);
                free(elem->data.object->key);
                elem->data.object->key = newKey;
            }
        } else {
            if (elem->type == FFON_STRING) {
                free(elem->data.string);
                elem->data.string = strdup(line);
            }
        }
    }
}

void updateIds(AppRenderer *appRenderer, bool isKey, Task task, History history) {
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                     &appRenderer->currentId, &count);
    if (!arr) return;

    int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];

    if (task == TASK_K_ARROW_UP) {
        if (idx > 0) {
            appRenderer->currentId.ids[appRenderer->currentId.depth - 1]--;
        }
    } else if (task == TASK_J_ARROW_DOWN) {
        if (idx < count - 1) {
            appRenderer->currentId.ids[appRenderer->currentId.depth - 1]++;
        }
    }
}

/* ============================================
 * Test helpers
 * ============================================ */

static AppRenderer* createTestApp(void) {
    AppRenderer *app = calloc(1, sizeof(AppRenderer));
    app->ffonCapacity = 10;
    app->ffon = calloc(app->ffonCapacity, sizeof(FfonElement*));
    app->undoHistory = calloc(UNDO_HISTORY_SIZE, sizeof(UndoEntry));
    app->inputBufferCapacity = 1024;
    app->inputBuffer = calloc(app->inputBufferCapacity, 1);
    idArrayInit(&app->currentId);
    idArrayInit(&app->previousId);
    idArrayInit(&app->currentInsertId);
    return app;
}

static void destroyTestApp(AppRenderer *app) {
    for (int i = 0; i < app->ffonCount; i++) {
        ffonElementDestroy(app->ffon[i]);
    }
    free(app->ffon);
    for (int i = 0; i < app->undoHistoryCount; i++) {
        ffonElementDestroy(app->undoHistory[i].prevElement);
        ffonElementDestroy(app->undoHistory[i].newElement);
    }
    free(app->undoHistory);
    free(app->inputBuffer);
    free(app);
}

/* ============================================
 * Unity Setup/Teardown
 * ============================================ */

void setUp(void) {
    FFF_RESET_HISTORY();
}

void tearDown(void) {}

/* ============================================
 * stripTrailingColon tests
 * ============================================ */

void test_stripTrailingColon_null(void) {
    char *result = stripTrailingColon(NULL);
    TEST_ASSERT_EQUAL_STRING("", result);
    free(result);
}

void test_stripTrailingColon_with_colon(void) {
    char *result = stripTrailingColon("key:");
    TEST_ASSERT_EQUAL_STRING("key", result);
    free(result);
}

void test_stripTrailingColon_without_colon(void) {
    char *result = stripTrailingColon("value");
    TEST_ASSERT_EQUAL_STRING("value", result);
    free(result);
}

void test_stripTrailingColon_empty(void) {
    char *result = stripTrailingColon("");
    TEST_ASSERT_EQUAL_STRING("", result);
    free(result);
}

void test_stripTrailingColon_just_colon(void) {
    char *result = stripTrailingColon(":");
    TEST_ASSERT_EQUAL_STRING("", result);
    free(result);
}

/* ============================================
 * updateHistory tests
 * ============================================ */

void test_updateHistory_skips_undo_mode(void) {
    AppRenderer *app = createTestApp();
    IdArray id = {.ids = {0}, .depth = 1};
    FfonElement *elem = ffonElementCreateString("test");

    updateHistory(app, TASK_INPUT, &id, elem, elem, HISTORY_UNDO);
    TEST_ASSERT_EQUAL_INT(0, app->undoHistoryCount);

    ffonElementDestroy(elem);
    destroyTestApp(app);
}

void test_updateHistory_skips_navigation_tasks(void) {
    AppRenderer *app = createTestApp();
    IdArray id = {.ids = {0}, .depth = 1};

    updateHistory(app, TASK_K_ARROW_UP, &id, NULL, NULL, HISTORY_NONE);
    TEST_ASSERT_EQUAL_INT(0, app->undoHistoryCount);

    updateHistory(app, TASK_J_ARROW_DOWN, &id, NULL, NULL, HISTORY_NONE);
    TEST_ASSERT_EQUAL_INT(0, app->undoHistoryCount);

    updateHistory(app, TASK_COPY, &id, NULL, NULL, HISTORY_NONE);
    TEST_ASSERT_EQUAL_INT(0, app->undoHistoryCount);

    destroyTestApp(app);
}

void test_updateHistory_adds_entry(void) {
    AppRenderer *app = createTestApp();
    IdArray id = {.ids = {0, 2}, .depth = 2};
    FfonElement *prev = ffonElementCreateString("old");
    FfonElement *new = ffonElementCreateString("new");

    updateHistory(app, TASK_INPUT, &id, prev, new, HISTORY_NONE);
    TEST_ASSERT_EQUAL_INT(1, app->undoHistoryCount);
    TEST_ASSERT_EQUAL_INT(1, app->undoPosition);
    TEST_ASSERT_EQUAL_INT(TASK_INPUT, app->undoHistory[0].task);
    TEST_ASSERT_TRUE(idArrayEqual(&id, &app->undoHistory[0].id));

    ffonElementDestroy(prev);
    ffonElementDestroy(new);
    destroyTestApp(app);
}

void test_updateHistory_multiple_entries(void) {
    AppRenderer *app = createTestApp();
    IdArray id = {.ids = {0}, .depth = 1};

    for (int i = 0; i < 5; i++) {
        FfonElement *elem = ffonElementCreateString("x");
        updateHistory(app, TASK_INPUT, &id, elem, elem, HISTORY_NONE);
        ffonElementDestroy(elem);
    }

    TEST_ASSERT_EQUAL_INT(5, app->undoHistoryCount);
    TEST_ASSERT_EQUAL_INT(5, app->undoPosition);

    destroyTestApp(app);
}

void test_updateHistory_null_elements(void) {
    AppRenderer *app = createTestApp();
    IdArray id = {.ids = {0}, .depth = 1};

    updateHistory(app, TASK_DELETE, &id, NULL, NULL, HISTORY_NONE);
    TEST_ASSERT_EQUAL_INT(1, app->undoHistoryCount);
    TEST_ASSERT_NULL(app->undoHistory[0].prevElement);
    TEST_ASSERT_NULL(app->undoHistory[0].newElement);

    destroyTestApp(app);
}

/* ============================================
 * updateFfon tests
 * ============================================ */

void test_updateFfon_input_string(void) {
    AppRenderer *app = createTestApp();

    FfonElement *root = ffonElementCreateObject("root:");
    ffonObjectAddElement(root->data.object, ffonElementCreateString("old value"));
    app->ffon[0] = root;
    app->ffonCount = 1;

    app->currentId.depth = 2;
    app->currentId.ids[0] = 0;
    app->currentId.ids[1] = 0;

    updateFfon(app, "new value", false, TASK_INPUT, HISTORY_NONE);

    int count;
    FfonElement **arr = getFfonAtId(app->ffon, app->ffonCount, &app->currentId, &count);
    TEST_ASSERT_EQUAL_STRING("new value", arr[0]->data.string);

    destroyTestApp(app);
}

void test_updateFfon_input_key(void) {
    AppRenderer *app = createTestApp();

    FfonElement *root = ffonElementCreateObject("root:");
    FfonElement *child = ffonElementCreateObject("old key:");
    ffonObjectAddElement(child->data.object, ffonElementCreateString("child value"));
    ffonObjectAddElement(root->data.object, child);
    app->ffon[0] = root;
    app->ffonCount = 1;

    app->currentId.depth = 2;
    app->currentId.ids[0] = 0;
    app->currentId.ids[1] = 0;

    updateFfon(app, "new key", true, TASK_INPUT, HISTORY_NONE);

    int count;
    FfonElement **arr = getFfonAtId(app->ffon, app->ffonCount, &app->currentId, &count);
    TEST_ASSERT_EQUAL_STRING("new key:", arr[0]->data.object->key);

    destroyTestApp(app);
}

/* ============================================
 * updateIds tests
 * ============================================ */

void test_updateIds_move_up(void) {
    AppRenderer *app = createTestApp();

    FfonElement *root = ffonElementCreateObject("root:");
    ffonObjectAddElement(root->data.object, ffonElementCreateString("a"));
    ffonObjectAddElement(root->data.object, ffonElementCreateString("b"));
    ffonObjectAddElement(root->data.object, ffonElementCreateString("c"));
    app->ffon[0] = root;
    app->ffonCount = 1;

    app->currentId.depth = 2;
    app->currentId.ids[0] = 0;
    app->currentId.ids[1] = 2;

    updateIds(app, false, TASK_K_ARROW_UP, HISTORY_NONE);
    TEST_ASSERT_EQUAL_INT(1, app->currentId.ids[1]);

    destroyTestApp(app);
}

void test_updateIds_move_down(void) {
    AppRenderer *app = createTestApp();

    FfonElement *root = ffonElementCreateObject("root:");
    ffonObjectAddElement(root->data.object, ffonElementCreateString("a"));
    ffonObjectAddElement(root->data.object, ffonElementCreateString("b"));
    ffonObjectAddElement(root->data.object, ffonElementCreateString("c"));
    app->ffon[0] = root;
    app->ffonCount = 1;

    app->currentId.depth = 2;
    app->currentId.ids[0] = 0;
    app->currentId.ids[1] = 0;

    updateIds(app, false, TASK_J_ARROW_DOWN, HISTORY_NONE);
    TEST_ASSERT_EQUAL_INT(1, app->currentId.ids[1]);

    destroyTestApp(app);
}

void test_updateIds_move_up_at_top(void) {
    AppRenderer *app = createTestApp();

    FfonElement *root = ffonElementCreateObject("root:");
    ffonObjectAddElement(root->data.object, ffonElementCreateString("a"));
    app->ffon[0] = root;
    app->ffonCount = 1;

    app->currentId.depth = 2;
    app->currentId.ids[0] = 0;
    app->currentId.ids[1] = 0;

    updateIds(app, false, TASK_K_ARROW_UP, HISTORY_NONE);
    TEST_ASSERT_EQUAL_INT(0, app->currentId.ids[1]); // stays at 0

    destroyTestApp(app);
}

void test_updateIds_move_down_at_bottom(void) {
    AppRenderer *app = createTestApp();

    FfonElement *root = ffonElementCreateObject("root:");
    ffonObjectAddElement(root->data.object, ffonElementCreateString("a"));
    ffonObjectAddElement(root->data.object, ffonElementCreateString("b"));
    app->ffon[0] = root;
    app->ffonCount = 1;

    app->currentId.depth = 2;
    app->currentId.ids[0] = 0;
    app->currentId.ids[1] = 1;

    updateIds(app, false, TASK_J_ARROW_DOWN, HISTORY_NONE);
    TEST_ASSERT_EQUAL_INT(1, app->currentId.ids[1]); // stays at 1

    destroyTestApp(app);
}

/* ============================================
 * Main
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // stripTrailingColon
    RUN_TEST(test_stripTrailingColon_null);
    RUN_TEST(test_stripTrailingColon_with_colon);
    RUN_TEST(test_stripTrailingColon_without_colon);
    RUN_TEST(test_stripTrailingColon_empty);
    RUN_TEST(test_stripTrailingColon_just_colon);

    // updateHistory
    RUN_TEST(test_updateHistory_skips_undo_mode);
    RUN_TEST(test_updateHistory_skips_navigation_tasks);
    RUN_TEST(test_updateHistory_adds_entry);
    RUN_TEST(test_updateHistory_multiple_entries);
    RUN_TEST(test_updateHistory_null_elements);

    // updateFfon
    RUN_TEST(test_updateFfon_input_string);
    RUN_TEST(test_updateFfon_input_key);

    // updateIds
    RUN_TEST(test_updateIds_move_up);
    RUN_TEST(test_updateIds_move_down);
    RUN_TEST(test_updateIds_move_up_at_top);
    RUN_TEST(test_updateIds_move_down_at_bottom);

    return UNITY_END();
}

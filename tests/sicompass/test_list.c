/*
 * Tests for list.c functions:
 * - clearListCurrentLayer
 * - populateListCurrentLayer (filtering logic)
 */

#include <unity.h>
#include <fff/fff.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

DEFINE_FFF_GLOBALS;

/* ============================================
 * Type definitions
 * ============================================ */

#define MAX_ID_DEPTH 32

typedef struct {
    int ids[MAX_ID_DEPTH];
    int depth;
} IdArray;

typedef struct {
    IdArray id;
    char *label;
    char *data;
    char *navPath;
} ListItem;

// Mock utf8_stristr (case-insensitive search used by populateListCurrentLayer)
// Use a real implementation for testing
static const char* utf8_stristr(const char* haystack, const char* needle) {
    if (!haystack || !needle) return NULL;
    if (*needle == '\0') return haystack;

    size_t needleLen = strlen(needle);
    size_t haystackLen = strlen(haystack);
    if (needleLen > haystackLen) return NULL;

    for (size_t i = 0; i <= haystackLen - needleLen; i++) {
        bool match = true;
        for (size_t j = 0; j < needleLen; j++) {
            char h = haystack[i + j];
            char n = needle[j];
            // Simple ASCII case-insensitive
            if (h >= 'A' && h <= 'Z') h += 32;
            if (n >= 'A' && n <= 'Z') n += 32;
            if (h != n) {
                match = false;
                break;
            }
        }
        if (match) return haystack + i;
    }
    return NULL;
}

static void idArrayCopy(IdArray *dst, const IdArray *src) {
    memcpy(dst, src, sizeof(IdArray));
}

// Minimal AppRenderer for list tests
typedef struct {
    ListItem *totalListCurrentLayer;
    int totalListCount;
    ListItem *filteredListCurrentLayer;
    int filteredListCount;
    int listIndex;
} AppRenderer;

/* ============================================
 * Functions under test (from list.c)
 * ============================================ */

void clearListCurrentLayer(AppRenderer *appRenderer) {
    if (appRenderer->totalListCurrentLayer) {
        for (int i = 0; i < appRenderer->totalListCount; i++) {
            free(appRenderer->totalListCurrentLayer[i].label);
            free(appRenderer->totalListCurrentLayer[i].data);
            free(appRenderer->totalListCurrentLayer[i].navPath);
        }
        free(appRenderer->totalListCurrentLayer);
        appRenderer->totalListCurrentLayer = NULL;
        appRenderer->totalListCount = 0;
    }

    if (appRenderer->filteredListCurrentLayer) {
        free(appRenderer->filteredListCurrentLayer);
        appRenderer->filteredListCurrentLayer = NULL;
        appRenderer->filteredListCount = 0;
    }

    appRenderer->listIndex = 0;
}

void populateListCurrentLayer(AppRenderer *appRenderer, const char *searchString) {
    if (!searchString || strlen(searchString) == 0) {
        if (appRenderer->filteredListCurrentLayer) {
            free(appRenderer->filteredListCurrentLayer);
        }
        appRenderer->filteredListCurrentLayer = NULL;
        appRenderer->filteredListCount = 0;
        appRenderer->listIndex = 0;
        return;
    }

    if (appRenderer->filteredListCurrentLayer) {
        free(appRenderer->filteredListCurrentLayer);
    }

    appRenderer->filteredListCurrentLayer = calloc(appRenderer->totalListCount, sizeof(ListItem));
    if (!appRenderer->filteredListCurrentLayer) return;

    appRenderer->filteredListCount = 0;

    for (int i = 0; i < appRenderer->totalListCount; i++) {
        const char *curLabel = appRenderer->totalListCurrentLayer[i].label;
        bool matches;
        if (appRenderer->totalListCurrentLayer[i].navPath) {
            const char *bareName = (curLabel[0] != '\0' && curLabel[1] == ' ') ? curLabel + 2 : curLabel;
            const char *found = utf8_stristr(bareName, searchString);
            matches = (found == bareName);
        } else {
            matches = (utf8_stristr(curLabel, searchString) != NULL);
        }
        if (matches) {
            idArrayCopy(&appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].id,
                         &appRenderer->totalListCurrentLayer[i].id);
            appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].label =
                appRenderer->totalListCurrentLayer[i].label;
            appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].data =
                appRenderer->totalListCurrentLayer[i].data;
            appRenderer->filteredListCurrentLayer[appRenderer->filteredListCount].navPath =
                appRenderer->totalListCurrentLayer[i].navPath;
            appRenderer->filteredListCount++;
        }
    }

    if (appRenderer->listIndex >= appRenderer->filteredListCount) {
        appRenderer->listIndex = appRenderer->filteredListCount > 0 ? appRenderer->filteredListCount - 1 : 0;
    }
}

/* ============================================
 * Test helpers
 * ============================================ */

static AppRenderer createTestApp(void) {
    AppRenderer app = {0};
    return app;
}

static void addTestItem(AppRenderer *app, const char *label, const char *data, const char *navPath) {
    app->totalListCount++;
    app->totalListCurrentLayer = realloc(app->totalListCurrentLayer,
                                          app->totalListCount * sizeof(ListItem));
    ListItem *item = &app->totalListCurrentLayer[app->totalListCount - 1];
    memset(item, 0, sizeof(ListItem));
    item->label = label ? strdup(label) : NULL;
    item->data = data ? strdup(data) : NULL;
    item->navPath = navPath ? strdup(navPath) : NULL;
    item->id.depth = 1;
    item->id.ids[0] = app->totalListCount - 1;
}

/* ============================================
 * Unity Setup/Teardown
 * ============================================ */

void setUp(void) {
    FFF_RESET_HISTORY();
}

void tearDown(void) {}

/* ============================================
 * clearListCurrentLayer tests
 * ============================================ */

void test_clearList_empty(void) {
    AppRenderer app = createTestApp();
    clearListCurrentLayer(&app); // Should not crash
    TEST_ASSERT_NULL(app.totalListCurrentLayer);
    TEST_ASSERT_EQUAL_INT(0, app.totalListCount);
}

void test_clearList_with_items(void) {
    AppRenderer app = createTestApp();
    addTestItem(&app, "item1", NULL, NULL);
    addTestItem(&app, "item2", "data2", NULL);
    TEST_ASSERT_EQUAL_INT(2, app.totalListCount);

    clearListCurrentLayer(&app);
    TEST_ASSERT_NULL(app.totalListCurrentLayer);
    TEST_ASSERT_EQUAL_INT(0, app.totalListCount);
    TEST_ASSERT_EQUAL_INT(0, app.listIndex);
}

void test_clearList_with_filtered(void) {
    AppRenderer app = createTestApp();
    addTestItem(&app, "item1", NULL, NULL);
    app.filteredListCurrentLayer = calloc(1, sizeof(ListItem));
    app.filteredListCount = 1;

    clearListCurrentLayer(&app);
    TEST_ASSERT_NULL(app.filteredListCurrentLayer);
    TEST_ASSERT_EQUAL_INT(0, app.filteredListCount);
}

void test_clearList_resets_listIndex(void) {
    AppRenderer app = createTestApp();
    addTestItem(&app, "item1", NULL, NULL);
    app.listIndex = 5;

    clearListCurrentLayer(&app);
    TEST_ASSERT_EQUAL_INT(0, app.listIndex);
}

/* ============================================
 * populateListCurrentLayer tests
 * ============================================ */

void test_populate_null_search(void) {
    AppRenderer app = createTestApp();
    addTestItem(&app, "- hello", NULL, NULL);
    addTestItem(&app, "- world", NULL, NULL);

    populateListCurrentLayer(&app, NULL);
    TEST_ASSERT_NULL(app.filteredListCurrentLayer);
    TEST_ASSERT_EQUAL_INT(0, app.filteredListCount);

    clearListCurrentLayer(&app);
}

void test_populate_empty_search(void) {
    AppRenderer app = createTestApp();
    addTestItem(&app, "- hello", NULL, NULL);

    populateListCurrentLayer(&app, "");
    TEST_ASSERT_NULL(app.filteredListCurrentLayer);
    TEST_ASSERT_EQUAL_INT(0, app.filteredListCount);

    clearListCurrentLayer(&app);
}

void test_populate_matches_all(void) {
    AppRenderer app = createTestApp();
    addTestItem(&app, "- hello", NULL, NULL);
    addTestItem(&app, "- help", NULL, NULL);
    addTestItem(&app, "- world", NULL, NULL);

    populateListCurrentLayer(&app, "hel");
    TEST_ASSERT_EQUAL_INT(2, app.filteredListCount);
    TEST_ASSERT_EQUAL_STRING("- hello", app.filteredListCurrentLayer[0].label);
    TEST_ASSERT_EQUAL_STRING("- help", app.filteredListCurrentLayer[1].label);

    free(app.filteredListCurrentLayer);
    app.filteredListCurrentLayer = NULL;
    clearListCurrentLayer(&app);
}

void test_populate_no_matches(void) {
    AppRenderer app = createTestApp();
    addTestItem(&app, "- hello", NULL, NULL);
    addTestItem(&app, "- world", NULL, NULL);

    populateListCurrentLayer(&app, "xyz");
    TEST_ASSERT_EQUAL_INT(0, app.filteredListCount);

    free(app.filteredListCurrentLayer);
    app.filteredListCurrentLayer = NULL;
    clearListCurrentLayer(&app);
}

void test_populate_case_insensitive(void) {
    AppRenderer app = createTestApp();
    addTestItem(&app, "- Hello", NULL, NULL);
    addTestItem(&app, "- WORLD", NULL, NULL);

    populateListCurrentLayer(&app, "hello");
    TEST_ASSERT_EQUAL_INT(1, app.filteredListCount);
    TEST_ASSERT_EQUAL_STRING("- Hello", app.filteredListCurrentLayer[0].label);

    free(app.filteredListCurrentLayer);
    app.filteredListCurrentLayer = NULL;
    clearListCurrentLayer(&app);
}

void test_populate_navPath_prefix_match(void) {
    AppRenderer app = createTestApp();
    // navPath items use prefix matching on bare name (skip "- " prefix)
    addTestItem(&app, "- report.pdf", NULL, "/home/docs/report.pdf");
    addTestItem(&app, "- readme.md", NULL, "/home/docs/readme.md");
    addTestItem(&app, "- notes.txt", NULL, "/home/docs/notes.txt");

    // "rep" should prefix-match "report.pdf" but not others
    populateListCurrentLayer(&app, "rep");
    TEST_ASSERT_EQUAL_INT(1, app.filteredListCount);
    TEST_ASSERT_EQUAL_STRING("- report.pdf", app.filteredListCurrentLayer[0].label);

    free(app.filteredListCurrentLayer);
    app.filteredListCurrentLayer = NULL;
    clearListCurrentLayer(&app);
}

void test_populate_navPath_no_substring_match(void) {
    AppRenderer app = createTestApp();
    // navPath items only do prefix matching, not substring
    addTestItem(&app, "- report.pdf", NULL, "/home/docs/report.pdf");

    populateListCurrentLayer(&app, "port"); // substring but not prefix
    TEST_ASSERT_EQUAL_INT(0, app.filteredListCount);

    free(app.filteredListCurrentLayer);
    app.filteredListCurrentLayer = NULL;
    clearListCurrentLayer(&app);
}

void test_populate_clamps_listIndex(void) {
    AppRenderer app = createTestApp();
    addTestItem(&app, "- hello", NULL, NULL);
    addTestItem(&app, "- world", NULL, NULL);
    app.listIndex = 5; // Out of range

    populateListCurrentLayer(&app, "hello");
    // Only 1 match, listIndex should be clamped to 0
    TEST_ASSERT_EQUAL_INT(0, app.listIndex);

    free(app.filteredListCurrentLayer);
    app.filteredListCurrentLayer = NULL;
    clearListCurrentLayer(&app);
}

void test_populate_replaces_previous_filter(void) {
    AppRenderer app = createTestApp();
    addTestItem(&app, "- hello", NULL, NULL);
    addTestItem(&app, "- world", NULL, NULL);
    addTestItem(&app, "- help", NULL, NULL);

    // First filter
    populateListCurrentLayer(&app, "hel");
    TEST_ASSERT_EQUAL_INT(2, app.filteredListCount);

    // Second filter (narrower)
    populateListCurrentLayer(&app, "hello");
    TEST_ASSERT_EQUAL_INT(1, app.filteredListCount);

    free(app.filteredListCurrentLayer);
    app.filteredListCurrentLayer = NULL;
    clearListCurrentLayer(&app);
}

/* ============================================
 * Main
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // clearListCurrentLayer
    RUN_TEST(test_clearList_empty);
    RUN_TEST(test_clearList_with_items);
    RUN_TEST(test_clearList_with_filtered);
    RUN_TEST(test_clearList_resets_listIndex);

    // populateListCurrentLayer
    RUN_TEST(test_populate_null_search);
    RUN_TEST(test_populate_empty_search);
    RUN_TEST(test_populate_matches_all);
    RUN_TEST(test_populate_no_matches);
    RUN_TEST(test_populate_case_insensitive);
    RUN_TEST(test_populate_navPath_prefix_match);
    RUN_TEST(test_populate_navPath_no_substring_match);
    RUN_TEST(test_populate_clamps_listIndex);
    RUN_TEST(test_populate_replaces_previous_filter);

    return UNITY_END();
}

/*
 * Tests for state.c utility functions:
 * - coordinateToString
 * - taskToString
 * - isLineKey
 * - setErrorMessage
 */

#include <unity.h>
#include <string.h>
#include <stdio.h>
#include <stdbool.h>
#include <stdlib.h>

// Type definitions (from view.h)
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

// Minimal AppRenderer for setErrorMessage
typedef struct {
    char errorMessage[256];
} AppRenderer;

/* ============================================
 * Functions under test (from state.c)
 * ============================================ */

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

/* ============================================
 * Unity Setup/Teardown
 * ============================================ */

void setUp(void) {}
void tearDown(void) {}

/* ============================================
 * coordinateToString tests
 * ============================================ */

void test_coordinateToString_operator_general(void) {
    TEST_ASSERT_EQUAL_STRING("operator mode", coordinateToString(COORDINATE_OPERATOR_GENERAL));
}

void test_coordinateToString_operator_insert(void) {
    TEST_ASSERT_EQUAL_STRING("operator insert", coordinateToString(COORDINATE_OPERATOR_INSERT));
}

void test_coordinateToString_editor_general(void) {
    TEST_ASSERT_EQUAL_STRING("editor mode", coordinateToString(COORDINATE_EDITOR_GENERAL));
}

void test_coordinateToString_editor_insert(void) {
    TEST_ASSERT_EQUAL_STRING("editor insert", coordinateToString(COORDINATE_EDITOR_INSERT));
}

void test_coordinateToString_editor_normal(void) {
    TEST_ASSERT_EQUAL_STRING("editor normal", coordinateToString(COORDINATE_EDITOR_NORMAL));
}

void test_coordinateToString_editor_visual(void) {
    TEST_ASSERT_EQUAL_STRING("editor visual", coordinateToString(COORDINATE_EDITOR_VISUAL));
}

void test_coordinateToString_simple_search(void) {
    TEST_ASSERT_EQUAL_STRING("search", coordinateToString(COORDINATE_SIMPLE_SEARCH));
}

void test_coordinateToString_extended_search(void) {
    TEST_ASSERT_EQUAL_STRING("ext search", coordinateToString(COORDINATE_EXTENDED_SEARCH));
}

void test_coordinateToString_command(void) {
    TEST_ASSERT_EQUAL_STRING("run command", coordinateToString(COORDINATE_COMMAND));
}

void test_coordinateToString_scroll(void) {
    TEST_ASSERT_EQUAL_STRING("scroll mode", coordinateToString(COORDINATE_SCROLL));
}

void test_coordinateToString_scroll_search(void) {
    TEST_ASSERT_EQUAL_STRING("scroll search", coordinateToString(COORDINATE_SCROLL_SEARCH));
}

void test_coordinateToString_dashboard(void) {
    TEST_ASSERT_EQUAL_STRING("dashboard", coordinateToString(COORDINATE_DASHBOARD));
}

void test_coordinateToString_unknown(void) {
    TEST_ASSERT_EQUAL_STRING("unknown", coordinateToString((Coordinate)999));
}

/* ============================================
 * taskToString tests
 * ============================================ */

void test_taskToString_none(void) {
    TEST_ASSERT_EQUAL_STRING("none", taskToString(TASK_NONE));
}

void test_taskToString_input(void) {
    TEST_ASSERT_EQUAL_STRING("input", taskToString(TASK_INPUT));
}

void test_taskToString_append(void) {
    TEST_ASSERT_EQUAL_STRING("append", taskToString(TASK_APPEND));
}

void test_taskToString_append_append(void) {
    TEST_ASSERT_EQUAL_STRING("append append", taskToString(TASK_APPEND_APPEND));
}

void test_taskToString_insert(void) {
    TEST_ASSERT_EQUAL_STRING("insert", taskToString(TASK_INSERT));
}

void test_taskToString_insert_insert(void) {
    TEST_ASSERT_EQUAL_STRING("insert insert", taskToString(TASK_INSERT_INSERT));
}

void test_taskToString_delete(void) {
    TEST_ASSERT_EQUAL_STRING("delete", taskToString(TASK_DELETE));
}

void test_taskToString_up(void) {
    TEST_ASSERT_EQUAL_STRING("up", taskToString(TASK_K_ARROW_UP));
}

void test_taskToString_down(void) {
    TEST_ASSERT_EQUAL_STRING("down", taskToString(TASK_J_ARROW_DOWN));
}

void test_taskToString_left(void) {
    TEST_ASSERT_EQUAL_STRING("left", taskToString(TASK_H_ARROW_LEFT));
}

void test_taskToString_right(void) {
    TEST_ASSERT_EQUAL_STRING("right", taskToString(TASK_L_ARROW_RIGHT));
}

void test_taskToString_cut(void) {
    TEST_ASSERT_EQUAL_STRING("cut", taskToString(TASK_CUT));
}

void test_taskToString_copy(void) {
    TEST_ASSERT_EQUAL_STRING("copy", taskToString(TASK_COPY));
}

void test_taskToString_paste(void) {
    TEST_ASSERT_EQUAL_STRING("paste", taskToString(TASK_PASTE));
}

void test_taskToString_unknown(void) {
    TEST_ASSERT_EQUAL_STRING("unknown", taskToString((Task)999));
}

/* ============================================
 * isLineKey tests
 * ============================================ */

void test_isLineKey_null(void) {
    TEST_ASSERT_FALSE(isLineKey(NULL));
}

void test_isLineKey_empty(void) {
    TEST_ASSERT_FALSE(isLineKey(""));
}

void test_isLineKey_with_colon(void) {
    TEST_ASSERT_TRUE(isLineKey("key:"));
}

void test_isLineKey_without_colon(void) {
    TEST_ASSERT_FALSE(isLineKey("value"));
}

void test_isLineKey_just_colon(void) {
    TEST_ASSERT_TRUE(isLineKey(":"));
}

void test_isLineKey_colon_in_middle(void) {
    TEST_ASSERT_FALSE(isLineKey("key:value"));
}

void test_isLineKey_multiple_colons(void) {
    TEST_ASSERT_TRUE(isLineKey("a:b:c:"));
}

void test_isLineKey_spaces_before_colon(void) {
    TEST_ASSERT_TRUE(isLineKey("my key :"));
}

/* ============================================
 * setErrorMessage tests
 * ============================================ */

void test_setErrorMessage_simple(void) {
    AppRenderer app = {0};
    setErrorMessage(&app, "test error");
    TEST_ASSERT_EQUAL_STRING("test error", app.errorMessage);
}

void test_setErrorMessage_empty(void) {
    AppRenderer app = {0};
    setErrorMessage(&app, "");
    TEST_ASSERT_EQUAL_STRING("", app.errorMessage);
}

void test_setErrorMessage_overwrites(void) {
    AppRenderer app = {0};
    setErrorMessage(&app, "first");
    setErrorMessage(&app, "second");
    TEST_ASSERT_EQUAL_STRING("second", app.errorMessage);
}

void test_setErrorMessage_truncates_long_message(void) {
    AppRenderer app = {0};
    char longMsg[512];
    memset(longMsg, 'A', sizeof(longMsg) - 1);
    longMsg[sizeof(longMsg) - 1] = '\0';

    setErrorMessage(&app, longMsg);
    TEST_ASSERT_EQUAL_INT(255, strlen(app.errorMessage));
}

/* ============================================
 * Main
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // coordinateToString
    RUN_TEST(test_coordinateToString_operator_general);
    RUN_TEST(test_coordinateToString_operator_insert);
    RUN_TEST(test_coordinateToString_editor_general);
    RUN_TEST(test_coordinateToString_editor_insert);
    RUN_TEST(test_coordinateToString_editor_normal);
    RUN_TEST(test_coordinateToString_editor_visual);
    RUN_TEST(test_coordinateToString_simple_search);
    RUN_TEST(test_coordinateToString_extended_search);
    RUN_TEST(test_coordinateToString_command);
    RUN_TEST(test_coordinateToString_scroll);
    RUN_TEST(test_coordinateToString_scroll_search);
    RUN_TEST(test_coordinateToString_dashboard);
    RUN_TEST(test_coordinateToString_unknown);

    // taskToString
    RUN_TEST(test_taskToString_none);
    RUN_TEST(test_taskToString_input);
    RUN_TEST(test_taskToString_append);
    RUN_TEST(test_taskToString_append_append);
    RUN_TEST(test_taskToString_insert);
    RUN_TEST(test_taskToString_insert_insert);
    RUN_TEST(test_taskToString_delete);
    RUN_TEST(test_taskToString_up);
    RUN_TEST(test_taskToString_down);
    RUN_TEST(test_taskToString_left);
    RUN_TEST(test_taskToString_right);
    RUN_TEST(test_taskToString_cut);
    RUN_TEST(test_taskToString_copy);
    RUN_TEST(test_taskToString_paste);
    RUN_TEST(test_taskToString_unknown);

    // isLineKey
    RUN_TEST(test_isLineKey_null);
    RUN_TEST(test_isLineKey_empty);
    RUN_TEST(test_isLineKey_with_colon);
    RUN_TEST(test_isLineKey_without_colon);
    RUN_TEST(test_isLineKey_just_colon);
    RUN_TEST(test_isLineKey_colon_in_middle);
    RUN_TEST(test_isLineKey_multiple_colons);
    RUN_TEST(test_isLineKey_spaces_before_colon);

    // setErrorMessage
    RUN_TEST(test_setErrorMessage_simple);
    RUN_TEST(test_setErrorMessage_empty);
    RUN_TEST(test_setErrorMessage_overwrites);
    RUN_TEST(test_setErrorMessage_truncates_long_message);

    return UNITY_END();
}

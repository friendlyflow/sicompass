/*
 * Tests for handlers.c functions:
 * - UTF-8 helpers: utf8_char_length, utf8_move_backward, utf8_move_forward
 * - Selection helpers: hasSelection, clearSelection, getSelectionRange, deleteSelection
 * - Selection-extending: handleShiftHome, handleShiftEnd, handleSelectAll
 */

#include <unity.h>
#include <fff/fff.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

DEFINE_FFF_GLOBALS;

// Mock SDL_GetTicks and caretReset (used by shift handlers)
FAKE_VALUE_FUNC(uint64_t, SDL_GetTicks);
FAKE_VOID_FUNC(caretReset, void*, uint64_t);

#define MAX_LINE_LENGTH 4096

// Minimal types
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

typedef struct {
    char *inputBuffer;
    int inputBufferSize;
    int inputBufferCapacity;
    int cursorPosition;
    int selectionAnchor;
    Coordinate currentCoordinate;
    bool needsRedraw;
    void *caretState; // opaque for tests
} AppRenderer;

/* ============================================
 * UTF-8 helpers (static in handlers.c, copied here)
 * ============================================ */

static int utf8_char_length(const char *str, int pos) {
    unsigned char c = (unsigned char)str[pos];

    if ((c & 0x80) == 0) {
        return 1;
    } else if ((c & 0xE0) == 0xC0) {
        return 2;
    } else if ((c & 0xF0) == 0xE0) {
        return 3;
    } else if ((c & 0xF8) == 0xF0) {
        return 4;
    }

    return 1;
}

static int utf8_move_backward(const char *str, int cursorPos) {
    if (cursorPos <= 0) {
        return 0;
    }

    int newPos = cursorPos - 1;

    while (newPos > 0 && ((unsigned char)str[newPos] & 0xC0) == 0x80) {
        newPos--;
    }

    return newPos;
}

static int utf8_move_forward(const char *str, int cursorPos, int bufferSize) {
    if (cursorPos >= bufferSize) {
        return bufferSize;
    }

    int charLen = utf8_char_length(str, cursorPos);
    int newPos = cursorPos + charLen;

    if (newPos > bufferSize) {
        newPos = bufferSize;
    }

    return newPos;
}

/* ============================================
 * Selection helpers (from handlers.c)
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
 * Selection-extending handlers (from handlers.c)
 * ============================================ */

void handleShiftLeft(AppRenderer *appRenderer) {
    if (appRenderer->cursorPosition <= 0) return;

    if (appRenderer->selectionAnchor == -1) {
        appRenderer->selectionAnchor = appRenderer->cursorPosition;
    }

    appRenderer->cursorPosition = utf8_move_backward(
        appRenderer->inputBuffer, appRenderer->cursorPosition);

    caretReset(appRenderer->caretState, SDL_GetTicks());
    appRenderer->needsRedraw = true;
}

void handleShiftRight(AppRenderer *appRenderer) {
    if (appRenderer->cursorPosition >= appRenderer->inputBufferSize) return;

    if (appRenderer->selectionAnchor == -1) {
        appRenderer->selectionAnchor = appRenderer->cursorPosition;
    }

    appRenderer->cursorPosition = utf8_move_forward(
        appRenderer->inputBuffer, appRenderer->cursorPosition,
        appRenderer->inputBufferSize);

    caretReset(appRenderer->caretState, SDL_GetTicks());
    appRenderer->needsRedraw = true;
}

void handleShiftHome(AppRenderer *appRenderer) {
    if (appRenderer->selectionAnchor == -1) {
        appRenderer->selectionAnchor = appRenderer->cursorPosition;
    }
    appRenderer->cursorPosition = 0;
    caretReset(appRenderer->caretState, SDL_GetTicks());
    appRenderer->needsRedraw = true;
}

void handleShiftEnd(AppRenderer *appRenderer) {
    if (appRenderer->selectionAnchor == -1) {
        appRenderer->selectionAnchor = appRenderer->cursorPosition;
    }
    appRenderer->cursorPosition = appRenderer->inputBufferSize;
    caretReset(appRenderer->caretState, SDL_GetTicks());
    appRenderer->needsRedraw = true;
}

void handleSelectAll(AppRenderer *appRenderer) {
    if (appRenderer->inputBufferSize == 0) return;
    appRenderer->selectionAnchor = 0;
    appRenderer->cursorPosition = appRenderer->inputBufferSize;
    caretReset(appRenderer->caretState, SDL_GetTicks());
    appRenderer->needsRedraw = true;
}

/* ============================================
 * Test helpers
 * ============================================ */

static AppRenderer createTestApp(const char *text) {
    AppRenderer app = {0};
    int len = text ? (int)strlen(text) : 0;
    app.inputBufferCapacity = len + 64;
    app.inputBuffer = calloc(app.inputBufferCapacity, 1);
    if (text) {
        memcpy(app.inputBuffer, text, len);
    }
    app.inputBufferSize = len;
    app.cursorPosition = 0;
    app.selectionAnchor = -1;
    app.caretState = NULL;
    return app;
}

static void freeTestApp(AppRenderer *app) {
    free(app->inputBuffer);
}

/* ============================================
 * Unity Setup/Teardown
 * ============================================ */

void setUp(void) {
    RESET_FAKE(SDL_GetTicks);
    RESET_FAKE(caretReset);
    FFF_RESET_HISTORY();
    SDL_GetTicks_fake.return_val = 1000;
}

void tearDown(void) {}

/* ============================================
 * utf8_char_length tests
 * ============================================ */

void test_utf8_char_length_ascii(void) {
    TEST_ASSERT_EQUAL_INT(1, utf8_char_length("A", 0));
}

void test_utf8_char_length_two_byte(void) {
    // "é" is 0xC3 0xA9
    TEST_ASSERT_EQUAL_INT(2, utf8_char_length("\xC3\xA9", 0));
}

void test_utf8_char_length_three_byte(void) {
    // "€" is 0xE2 0x82 0xAC
    TEST_ASSERT_EQUAL_INT(3, utf8_char_length("\xE2\x82\xAC", 0));
}

void test_utf8_char_length_four_byte(void) {
    // "𝄞" is 0xF0 0x9D 0x84 0x9E
    TEST_ASSERT_EQUAL_INT(4, utf8_char_length("\xF0\x9D\x84\x9E", 0));
}

void test_utf8_char_length_at_offset(void) {
    // "Aé" - A at 0, é at 1
    TEST_ASSERT_EQUAL_INT(1, utf8_char_length("A\xC3\xA9", 0));
    TEST_ASSERT_EQUAL_INT(2, utf8_char_length("A\xC3\xA9", 1));
}

/* ============================================
 * utf8_move_backward tests
 * ============================================ */

void test_utf8_move_backward_at_start(void) {
    TEST_ASSERT_EQUAL_INT(0, utf8_move_backward("hello", 0));
}

void test_utf8_move_backward_ascii(void) {
    TEST_ASSERT_EQUAL_INT(2, utf8_move_backward("hello", 3));
}

void test_utf8_move_backward_two_byte_char(void) {
    // "Aé" = A(1 byte) + é(2 bytes) = 3 bytes total
    // From byte 3 (end), should go back to byte 1 (start of é)
    TEST_ASSERT_EQUAL_INT(1, utf8_move_backward("A\xC3\xA9", 3));
}

void test_utf8_move_backward_three_byte_char(void) {
    // "A€" = A(1) + €(3) = 4 bytes
    // From byte 4 (end), back to byte 1 (start of €)
    TEST_ASSERT_EQUAL_INT(1, utf8_move_backward("A\xE2\x82\xAC", 4));
}

void test_utf8_move_backward_four_byte_char(void) {
    // "A𝄞" = A(1) + 𝄞(4) = 5 bytes
    // From byte 5 (end), back to byte 1
    TEST_ASSERT_EQUAL_INT(1, utf8_move_backward("A\xF0\x9D\x84\x9E", 5));
}

/* ============================================
 * utf8_move_forward tests
 * ============================================ */

void test_utf8_move_forward_ascii(void) {
    TEST_ASSERT_EQUAL_INT(1, utf8_move_forward("hello", 0, 5));
}

void test_utf8_move_forward_at_end(void) {
    TEST_ASSERT_EQUAL_INT(5, utf8_move_forward("hello", 5, 5));
}

void test_utf8_move_forward_two_byte_char(void) {
    // "é" is 2 bytes
    TEST_ASSERT_EQUAL_INT(2, utf8_move_forward("\xC3\xA9", 0, 2));
}

void test_utf8_move_forward_three_byte_char(void) {
    // "€" is 3 bytes
    TEST_ASSERT_EQUAL_INT(3, utf8_move_forward("\xE2\x82\xAC", 0, 3));
}

void test_utf8_move_forward_past_ascii_then_multibyte(void) {
    // "Aé" = A(1) + é(2) = 3 bytes
    // From 0, move forward = 1 (past A)
    TEST_ASSERT_EQUAL_INT(1, utf8_move_forward("A\xC3\xA9", 0, 3));
    // From 1, move forward = 3 (past é)
    TEST_ASSERT_EQUAL_INT(3, utf8_move_forward("A\xC3\xA9", 1, 3));
}

/* ============================================
 * hasSelection tests
 * ============================================ */

void test_hasSelection_no_selection(void) {
    AppRenderer app = createTestApp("hello");
    TEST_ASSERT_FALSE(hasSelection(&app));
    freeTestApp(&app);
}

void test_hasSelection_anchor_equals_cursor(void) {
    AppRenderer app = createTestApp("hello");
    app.selectionAnchor = 3;
    app.cursorPosition = 3;
    TEST_ASSERT_FALSE(hasSelection(&app));
    freeTestApp(&app);
}

void test_hasSelection_with_selection(void) {
    AppRenderer app = createTestApp("hello");
    app.selectionAnchor = 1;
    app.cursorPosition = 4;
    TEST_ASSERT_TRUE(hasSelection(&app));
    freeTestApp(&app);
}

void test_hasSelection_reverse_selection(void) {
    AppRenderer app = createTestApp("hello");
    app.selectionAnchor = 4;
    app.cursorPosition = 1;
    TEST_ASSERT_TRUE(hasSelection(&app));
    freeTestApp(&app);
}

/* ============================================
 * clearSelection tests
 * ============================================ */

void test_clearSelection_resets_anchor(void) {
    AppRenderer app = createTestApp("hello");
    app.selectionAnchor = 3;
    clearSelection(&app);
    TEST_ASSERT_EQUAL_INT(-1, app.selectionAnchor);
    freeTestApp(&app);
}

/* ============================================
 * getSelectionRange tests
 * ============================================ */

void test_getSelectionRange_forward(void) {
    AppRenderer app = createTestApp("hello");
    app.selectionAnchor = 1;
    app.cursorPosition = 4;
    int start, end;
    getSelectionRange(&app, &start, &end);
    TEST_ASSERT_EQUAL_INT(1, start);
    TEST_ASSERT_EQUAL_INT(4, end);
    freeTestApp(&app);
}

void test_getSelectionRange_backward(void) {
    AppRenderer app = createTestApp("hello");
    app.selectionAnchor = 4;
    app.cursorPosition = 1;
    int start, end;
    getSelectionRange(&app, &start, &end);
    TEST_ASSERT_EQUAL_INT(1, start);
    TEST_ASSERT_EQUAL_INT(4, end);
    freeTestApp(&app);
}

/* ============================================
 * deleteSelection tests
 * ============================================ */

void test_deleteSelection_no_selection(void) {
    AppRenderer app = createTestApp("hello");
    app.cursorPosition = 3;
    deleteSelection(&app);
    TEST_ASSERT_EQUAL_STRING("hello", app.inputBuffer);
    TEST_ASSERT_EQUAL_INT(5, app.inputBufferSize);
    freeTestApp(&app);
}

void test_deleteSelection_middle(void) {
    AppRenderer app = createTestApp("hello");
    app.selectionAnchor = 1;
    app.cursorPosition = 4;
    deleteSelection(&app);
    TEST_ASSERT_EQUAL_STRING("ho", app.inputBuffer);
    TEST_ASSERT_EQUAL_INT(2, app.inputBufferSize);
    TEST_ASSERT_EQUAL_INT(1, app.cursorPosition);
    TEST_ASSERT_EQUAL_INT(-1, app.selectionAnchor);
    freeTestApp(&app);
}

void test_deleteSelection_reverse(void) {
    AppRenderer app = createTestApp("hello");
    app.selectionAnchor = 4;
    app.cursorPosition = 1;
    deleteSelection(&app);
    TEST_ASSERT_EQUAL_STRING("ho", app.inputBuffer);
    TEST_ASSERT_EQUAL_INT(2, app.inputBufferSize);
    TEST_ASSERT_EQUAL_INT(1, app.cursorPosition);
    freeTestApp(&app);
}

void test_deleteSelection_entire_string(void) {
    AppRenderer app = createTestApp("hello");
    app.selectionAnchor = 0;
    app.cursorPosition = 5;
    deleteSelection(&app);
    TEST_ASSERT_EQUAL_STRING("", app.inputBuffer);
    TEST_ASSERT_EQUAL_INT(0, app.inputBufferSize);
    TEST_ASSERT_EQUAL_INT(0, app.cursorPosition);
    freeTestApp(&app);
}

void test_deleteSelection_single_char(void) {
    AppRenderer app = createTestApp("hello");
    app.selectionAnchor = 2;
    app.cursorPosition = 3;
    deleteSelection(&app);
    TEST_ASSERT_EQUAL_STRING("helo", app.inputBuffer);
    TEST_ASSERT_EQUAL_INT(4, app.inputBufferSize);
    freeTestApp(&app);
}

/* ============================================
 * handleShiftLeft tests
 * ============================================ */

void test_handleShiftLeft_starts_selection(void) {
    AppRenderer app = createTestApp("hello");
    app.cursorPosition = 3;
    handleShiftLeft(&app);
    TEST_ASSERT_EQUAL_INT(3, app.selectionAnchor);
    TEST_ASSERT_EQUAL_INT(2, app.cursorPosition);
    freeTestApp(&app);
}

void test_handleShiftLeft_extends_selection(void) {
    AppRenderer app = createTestApp("hello");
    app.cursorPosition = 3;
    handleShiftLeft(&app);
    handleShiftLeft(&app);
    TEST_ASSERT_EQUAL_INT(3, app.selectionAnchor);
    TEST_ASSERT_EQUAL_INT(1, app.cursorPosition);
    freeTestApp(&app);
}

void test_handleShiftLeft_at_start_noop(void) {
    AppRenderer app = createTestApp("hello");
    app.cursorPosition = 0;
    handleShiftLeft(&app);
    TEST_ASSERT_EQUAL_INT(-1, app.selectionAnchor);
    TEST_ASSERT_EQUAL_INT(0, app.cursorPosition);
    freeTestApp(&app);
}

void test_handleShiftLeft_utf8(void) {
    // "Aé" = A(1) + é(2) = 3 bytes
    AppRenderer app = createTestApp("A\xC3\xA9");
    app.cursorPosition = 3; // end
    handleShiftLeft(&app);
    TEST_ASSERT_EQUAL_INT(3, app.selectionAnchor);
    TEST_ASSERT_EQUAL_INT(1, app.cursorPosition); // start of é
    freeTestApp(&app);
}

/* ============================================
 * handleShiftRight tests
 * ============================================ */

void test_handleShiftRight_starts_selection(void) {
    AppRenderer app = createTestApp("hello");
    app.cursorPosition = 1;
    handleShiftRight(&app);
    TEST_ASSERT_EQUAL_INT(1, app.selectionAnchor);
    TEST_ASSERT_EQUAL_INT(2, app.cursorPosition);
    freeTestApp(&app);
}

void test_handleShiftRight_at_end_noop(void) {
    AppRenderer app = createTestApp("hello");
    app.cursorPosition = 5;
    handleShiftRight(&app);
    TEST_ASSERT_EQUAL_INT(-1, app.selectionAnchor);
    TEST_ASSERT_EQUAL_INT(5, app.cursorPosition);
    freeTestApp(&app);
}

void test_handleShiftRight_utf8(void) {
    // "éB" = é(2) + B(1) = 3 bytes
    AppRenderer app = createTestApp("\xC3\xA9" "B");
    app.cursorPosition = 0;
    handleShiftRight(&app);
    TEST_ASSERT_EQUAL_INT(0, app.selectionAnchor);
    TEST_ASSERT_EQUAL_INT(2, app.cursorPosition); // past é
    freeTestApp(&app);
}

/* ============================================
 * handleShiftHome tests
 * ============================================ */

void test_handleShiftHome_from_middle(void) {
    AppRenderer app = createTestApp("hello");
    app.cursorPosition = 3;
    handleShiftHome(&app);
    TEST_ASSERT_EQUAL_INT(3, app.selectionAnchor);
    TEST_ASSERT_EQUAL_INT(0, app.cursorPosition);
    freeTestApp(&app);
}

void test_handleShiftHome_preserves_existing_anchor(void) {
    AppRenderer app = createTestApp("hello");
    app.cursorPosition = 3;
    app.selectionAnchor = 4;
    handleShiftHome(&app);
    TEST_ASSERT_EQUAL_INT(4, app.selectionAnchor);
    TEST_ASSERT_EQUAL_INT(0, app.cursorPosition);
    freeTestApp(&app);
}

/* ============================================
 * handleShiftEnd tests
 * ============================================ */

void test_handleShiftEnd_from_middle(void) {
    AppRenderer app = createTestApp("hello");
    app.cursorPosition = 2;
    handleShiftEnd(&app);
    TEST_ASSERT_EQUAL_INT(2, app.selectionAnchor);
    TEST_ASSERT_EQUAL_INT(5, app.cursorPosition);
    freeTestApp(&app);
}

/* ============================================
 * handleSelectAll tests
 * ============================================ */

void test_handleSelectAll_selects_everything(void) {
    AppRenderer app = createTestApp("hello");
    app.cursorPosition = 2;
    handleSelectAll(&app);
    TEST_ASSERT_EQUAL_INT(0, app.selectionAnchor);
    TEST_ASSERT_EQUAL_INT(5, app.cursorPosition);
    freeTestApp(&app);
}

void test_handleSelectAll_empty_buffer_noop(void) {
    AppRenderer app = createTestApp("");
    handleSelectAll(&app);
    TEST_ASSERT_EQUAL_INT(-1, app.selectionAnchor);
    TEST_ASSERT_EQUAL_INT(0, app.cursorPosition);
    freeTestApp(&app);
}

/* ============================================
 * Main
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // utf8_char_length
    RUN_TEST(test_utf8_char_length_ascii);
    RUN_TEST(test_utf8_char_length_two_byte);
    RUN_TEST(test_utf8_char_length_three_byte);
    RUN_TEST(test_utf8_char_length_four_byte);
    RUN_TEST(test_utf8_char_length_at_offset);

    // utf8_move_backward
    RUN_TEST(test_utf8_move_backward_at_start);
    RUN_TEST(test_utf8_move_backward_ascii);
    RUN_TEST(test_utf8_move_backward_two_byte_char);
    RUN_TEST(test_utf8_move_backward_three_byte_char);
    RUN_TEST(test_utf8_move_backward_four_byte_char);

    // utf8_move_forward
    RUN_TEST(test_utf8_move_forward_ascii);
    RUN_TEST(test_utf8_move_forward_at_end);
    RUN_TEST(test_utf8_move_forward_two_byte_char);
    RUN_TEST(test_utf8_move_forward_three_byte_char);
    RUN_TEST(test_utf8_move_forward_past_ascii_then_multibyte);

    // hasSelection
    RUN_TEST(test_hasSelection_no_selection);
    RUN_TEST(test_hasSelection_anchor_equals_cursor);
    RUN_TEST(test_hasSelection_with_selection);
    RUN_TEST(test_hasSelection_reverse_selection);

    // clearSelection
    RUN_TEST(test_clearSelection_resets_anchor);

    // getSelectionRange
    RUN_TEST(test_getSelectionRange_forward);
    RUN_TEST(test_getSelectionRange_backward);

    // deleteSelection
    RUN_TEST(test_deleteSelection_no_selection);
    RUN_TEST(test_deleteSelection_middle);
    RUN_TEST(test_deleteSelection_reverse);
    RUN_TEST(test_deleteSelection_entire_string);
    RUN_TEST(test_deleteSelection_single_char);

    // handleShiftLeft
    RUN_TEST(test_handleShiftLeft_starts_selection);
    RUN_TEST(test_handleShiftLeft_extends_selection);
    RUN_TEST(test_handleShiftLeft_at_start_noop);
    RUN_TEST(test_handleShiftLeft_utf8);

    // handleShiftRight
    RUN_TEST(test_handleShiftRight_starts_selection);
    RUN_TEST(test_handleShiftRight_at_end_noop);
    RUN_TEST(test_handleShiftRight_utf8);

    // handleShiftHome / handleShiftEnd
    RUN_TEST(test_handleShiftHome_from_middle);
    RUN_TEST(test_handleShiftHome_preserves_existing_anchor);
    RUN_TEST(test_handleShiftEnd_from_middle);

    // handleSelectAll
    RUN_TEST(test_handleSelectAll_selects_everything);
    RUN_TEST(test_handleSelectAll_empty_buffer_noop);

    return UNITY_END();
}

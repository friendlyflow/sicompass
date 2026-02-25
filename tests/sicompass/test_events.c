/*
 * Tests for events.c functions:
 * - handleKeys (key dispatch routing)
 * - handleInput (text input insertion)
 */

#include <unity.h>
#include <fff/fff.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

DEFINE_FFF_GLOBALS;

/* ============================================
 * SDL type stubs
 * ============================================ */

typedef uint32_t SDL_Keycode;
typedef uint16_t SDL_Keymod;
typedef uint32_t Uint32;
typedef uint64_t SDL_WindowID;

#define SDL_KMOD_CTRL  0x0040
#define SDL_KMOD_SHIFT 0x0001
#define SDL_KMOD_ALT   0x0100

#define SDLK_TAB       '\t'
#define SDLK_RETURN    '\r'
#define SDLK_ESCAPE    0x1B
#define SDLK_A         'a'
#define SDLK_C         'c'
#define SDLK_D         'd'
#define SDLK_E         'e'
#define SDLK_F         'f'
#define SDLK_H         'h'
#define SDLK_I         'i'
#define SDLK_J         'j'
#define SDLK_K         'k'
#define SDLK_L         'l'
#define SDLK_O         'o'
#define SDLK_V         'v'
#define SDLK_X         'x'
#define SDLK_Z         'z'
#define SDLK_UP        0x40000052
#define SDLK_DOWN      0x40000051
#define SDLK_LEFT      0x40000050
#define SDLK_RIGHT     0x4000004F
#define SDLK_HOME      0x4000004A
#define SDLK_END       0x4000004D
#define SDLK_PAGEUP    0x4000004B
#define SDLK_PAGEDOWN  0x4000004E
#define SDLK_DELETE     0x0000007F
#define SDLK_BACKSPACE  0x00000008
#define SDLK_COLON     ':'

typedef struct {
    struct {
        SDL_Keycode key;
        SDL_Keymod mod;
    } key;
} SDL_Event;

/* ============================================
 * Coordinate / type stubs
 * ============================================ */

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
    COORDINATE_SCROLL_SEARCH
} Coordinate;

typedef enum {
    HISTORY_NONE,
    HISTORY_UNDO,
    HISTORY_REDO
} History;

typedef enum {
    COMMAND_NONE,
    COMMAND_EDITOR_MODE,
    COMMAND_OPERATOR_MODE,
    COMMAND_PROVIDER,
} Command;

typedef struct {
    Coordinate currentCoordinate;
    Command currentCommand;
    char *inputBuffer;
    int inputBufferSize;
    int inputBufferCapacity;
    int cursorPosition;
    int selectionAnchor;
    bool needsRedraw;
    void *caretState;
    int filteredListCount;
    int totalListCount;
} AppRenderer;

/* ============================================
 * Mock all handler functions called by handleKeys
 * ============================================ */

FAKE_VOID_FUNC(handleTab, AppRenderer*);
FAKE_VOID_FUNC(handleSelectAll, AppRenderer*);
FAKE_VOID_FUNC(handleCtrlA, AppRenderer*, History);
FAKE_VOID_FUNC(handleEnter, AppRenderer*, History);
FAKE_VOID_FUNC(handleCtrlEnter, AppRenderer*, History);
FAKE_VOID_FUNC(handleCtrlI, AppRenderer*, History);
FAKE_VOID_FUNC(handleCtrlIOperator, AppRenderer*);
FAKE_VOID_FUNC(handleCtrlAOperator, AppRenderer*);
FAKE_VOID_FUNC(handleDelete, AppRenderer*, History);
FAKE_VOID_FUNC(handleFileDelete, AppRenderer*);
FAKE_VOID_FUNC(handleColon, AppRenderer*);
FAKE_VOID_FUNC(handleUp, AppRenderer*);
FAKE_VOID_FUNC(handleDown, AppRenderer*);
FAKE_VOID_FUNC(handlePageUp, AppRenderer*);
FAKE_VOID_FUNC(handlePageDown, AppRenderer*);
FAKE_VOID_FUNC(handleLeft, AppRenderer*);
FAKE_VOID_FUNC(handleRight, AppRenderer*);
FAKE_VOID_FUNC(handleShiftLeft, AppRenderer*);
FAKE_VOID_FUNC(handleShiftRight, AppRenderer*);
FAKE_VOID_FUNC(handleHome, AppRenderer*);
FAKE_VOID_FUNC(handleEnd, AppRenderer*);
FAKE_VOID_FUNC(handleCtrlHome, AppRenderer*);
FAKE_VOID_FUNC(handleCtrlEnd, AppRenderer*);
FAKE_VOID_FUNC(handleShiftHome, AppRenderer*);
FAKE_VOID_FUNC(handleShiftEnd, AppRenderer*);
FAKE_VOID_FUNC(handleI, AppRenderer*);
FAKE_VOID_FUNC(handleA, AppRenderer*);
FAKE_VOID_FUNC(handleHistoryAction, AppRenderer*, History);
FAKE_VOID_FUNC(handleCtrlX, AppRenderer*);
FAKE_VOID_FUNC(handleCtrlC, AppRenderer*);
FAKE_VOID_FUNC(handleCtrlV, AppRenderer*);
FAKE_VOID_FUNC(handleCtrlF, AppRenderer*);
FAKE_VOID_FUNC(handleEscape, AppRenderer*);
FAKE_VOID_FUNC(handleCommand, AppRenderer*);

// Mocks for backspace/delete inline code
FAKE_VALUE_FUNC(bool, hasSelection, AppRenderer*);
FAKE_VOID_FUNC(deleteSelection, AppRenderer*);
FAKE_VOID_FUNC(caretReset, void*, uint64_t);
FAKE_VALUE_FUNC(uint64_t, SDL_GetTicks);
FAKE_VOID_FUNC(populateListCurrentLayer, AppRenderer*, const char*);
FAKE_VOID_FUNC(clearSelection, AppRenderer*);

/* ============================================
 * Function under test (from events.c)
 * ============================================ */

void handleKeys(AppRenderer *appRenderer, SDL_Event *event) {
    SDL_Keycode key = event->key.key;
    SDL_Keymod mod = event->key.mod;

    bool ctrl = (mod & SDL_KMOD_CTRL) != 0;
    bool shift = (mod & SDL_KMOD_SHIFT) != 0;
    bool alt = (mod & SDL_KMOD_ALT) != 0;

    if (!ctrl && !shift && !alt && key == SDLK_TAB) {
        handleTab(appRenderer);
    }
    else if (ctrl && !shift && !alt && key == SDLK_A &&
             (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH)) {
        handleSelectAll(appRenderer);
    }
    else if (((ctrl && !shift && !alt && key == SDLK_A) ||
              (!ctrl && !shift && !alt && key == SDLK_RETURN)) &&
             appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL) {
        handleCtrlA(appRenderer, HISTORY_NONE);
    }
    else if (ctrl && shift && !alt && key == SDLK_A &&
             appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT) {
        handleEscape(appRenderer);
        handleCtrlA(appRenderer, HISTORY_NONE);
        handleA(appRenderer);
    }
    else if (!ctrl && !shift && !alt && key == SDLK_RETURN) {
        handleEnter(appRenderer, HISTORY_NONE);
    }
    else if (ctrl && !shift && !alt && key == SDLK_RETURN) {
        handleCtrlEnter(appRenderer, HISTORY_NONE);
    }
    else if (ctrl && !shift && !alt && key == SDLK_I &&
             appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {
        handleCtrlIOperator(appRenderer);
    }
    else if (ctrl && !shift && !alt && key == SDLK_I &&
             appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL) {
        handleCtrlI(appRenderer, HISTORY_NONE);
    }
    else if (ctrl && shift && !alt && key == SDLK_I &&
             appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT) {
        handleEscape(appRenderer);
        handleCtrlI(appRenderer, HISTORY_NONE);
        handleI(appRenderer);
    }
    else if (ctrl && !shift && !alt && key == SDLK_A &&
             appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {
        handleCtrlAOperator(appRenderer);
    }
    else if (ctrl && !shift && !alt && key == SDLK_D &&
             appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {
        handleFileDelete(appRenderer);
    }
    else if (ctrl && !shift && !alt && key == SDLK_D &&
             appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL) {
        handleDelete(appRenderer, HISTORY_NONE);
    }
    else if (!ctrl && !shift && !alt && key == SDLK_DELETE &&
             appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {
        handleFileDelete(appRenderer);
    }
    else if (!ctrl && !shift && !alt && key == SDLK_COLON &&
             appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT &&
             appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT) {
        handleColon(appRenderer);
    }
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_K && (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
                                 appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL)) ||
              (key == SDLK_UP &&
               appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT &&
               appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT))) {
        handleUp(appRenderer);
    }
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_J && (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
                                 appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL)) ||
              (key == SDLK_DOWN &&
               appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT &&
               appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT))) {
        handleDown(appRenderer);
    }
    else if (!ctrl && !shift && !alt && key == SDLK_PAGEUP &&
             appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT &&
             appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT) {
        handlePageUp(appRenderer);
    }
    else if (!ctrl && !shift && !alt && key == SDLK_PAGEDOWN &&
             appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT &&
             appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT) {
        handlePageDown(appRenderer);
    }
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_H && (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
                                 appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL)) ||
              key == SDLK_LEFT)) {
        handleLeft(appRenderer);
    }
    else if (!ctrl && shift && !alt && key == SDLK_LEFT &&
             (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH)) {
        handleShiftLeft(appRenderer);
    }
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_L && (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
                                 appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL)) ||
              key == SDLK_RIGHT)) {
        handleRight(appRenderer);
    }
    else if (!ctrl && shift && !alt && key == SDLK_RIGHT &&
             (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH)) {
        handleShiftRight(appRenderer);
    }
    else if (!ctrl && !shift && !alt && key == SDLK_HOME &&
             (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
              appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
              appRenderer->currentCoordinate == COORDINATE_SCROLL)) {
        handleHome(appRenderer);
    }
    else if (!ctrl && !shift && !alt && key == SDLK_END &&
             (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
              appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
              appRenderer->currentCoordinate == COORDINATE_SCROLL)) {
        handleEnd(appRenderer);
    }
    else if (ctrl && !shift && !alt && key == SDLK_HOME &&
             (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH)) {
        handleCtrlHome(appRenderer);
    }
    else if (ctrl && !shift && !alt && key == SDLK_END &&
             (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH)) {
        handleCtrlEnd(appRenderer);
    }
    else if (!ctrl && !shift && !alt && key == SDLK_I) {
        handleI(appRenderer);
    }
    else if (!ctrl && !shift && !alt && key == SDLK_A) {
        handleA(appRenderer);
    }
    else if (ctrl && !shift && !alt && key == SDLK_Z) {
        handleHistoryAction(appRenderer, HISTORY_UNDO);
    }
    else if (ctrl && shift && !alt && key == SDLK_Z) {
        handleHistoryAction(appRenderer, HISTORY_REDO);
    }
    else if (ctrl && !shift && !alt && key == SDLK_X) {
        handleCtrlX(appRenderer);
    }
    else if (ctrl && !shift && !alt && key == SDLK_C) {
        handleCtrlC(appRenderer);
    }
    else if (ctrl && !shift && !alt && key == SDLK_V) {
        handleCtrlV(appRenderer);
    }
    else if (ctrl && !shift && !alt && key == SDLK_F) {
        handleCtrlF(appRenderer);
    }
    else if (!ctrl && !shift && !alt && key == SDLK_ESCAPE) {
        handleEscape(appRenderer);
    }
    else if (!ctrl && !shift && !alt && key == SDLK_E &&
             (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
              appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL)) {
        appRenderer->currentCommand = COMMAND_EDITOR_MODE;
        handleCommand(appRenderer);
    }
    else if (!ctrl && !shift && !alt && key == SDLK_O &&
             (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
              appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL)) {
        appRenderer->currentCommand = COMMAND_OPERATOR_MODE;
        handleCommand(appRenderer);
    }
}

/* handleInput (from events.c) */
void handleInput(AppRenderer *appRenderer, const char *text) {
    if (!text) return;

    if (appRenderer->currentCoordinate == COORDINATE_COMMAND &&
        appRenderer->inputBufferSize == 0 &&
        strcmp(text, ":") == 0) {
        return;
    }

    if (hasSelection(appRenderer)) {
        deleteSelection(appRenderer);
    }

    int len = strlen(text);
    if (appRenderer->inputBufferSize + len >= appRenderer->inputBufferCapacity) {
        int newCapacity = appRenderer->inputBufferCapacity * 2;
        char *newBuffer = realloc(appRenderer->inputBuffer, newCapacity);
        if (!newBuffer) return;
        appRenderer->inputBuffer = newBuffer;
        appRenderer->inputBufferCapacity = newCapacity;
    }

    memmove(&appRenderer->inputBuffer[appRenderer->cursorPosition + len],
           &appRenderer->inputBuffer[appRenderer->cursorPosition],
           appRenderer->inputBufferSize - appRenderer->cursorPosition + 1);
    memcpy(&appRenderer->inputBuffer[appRenderer->cursorPosition], text, len);
    appRenderer->inputBufferSize += len;
    appRenderer->cursorPosition += len;

    uint64_t currentTime = SDL_GetTicks();
    caretReset(appRenderer->caretState, currentTime);

    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        populateListCurrentLayer(appRenderer, appRenderer->inputBuffer);
    }

    appRenderer->needsRedraw = true;
}

/* ============================================
 * Test helpers
 * ============================================ */

static SDL_Event makeKeyEvent(SDL_Keycode key, SDL_Keymod mod) {
    SDL_Event e = {0};
    e.key.key = key;
    e.key.mod = mod;
    return e;
}

static AppRenderer createTestApp(Coordinate coord) {
    AppRenderer app = {0};
    app.currentCoordinate = coord;
    app.inputBufferCapacity = 256;
    app.inputBuffer = calloc(app.inputBufferCapacity, 1);
    app.selectionAnchor = -1;
    return app;
}

static void freeTestApp(AppRenderer *app) {
    free(app->inputBuffer);
}

/* ============================================
 * Unity Setup/Teardown
 * ============================================ */

void setUp(void) {
    RESET_FAKE(handleTab); RESET_FAKE(handleSelectAll);
    RESET_FAKE(handleCtrlA); RESET_FAKE(handleEnter);
    RESET_FAKE(handleCtrlEnter); RESET_FAKE(handleCtrlI);
    RESET_FAKE(handleCtrlIOperator); RESET_FAKE(handleCtrlAOperator);
    RESET_FAKE(handleDelete); RESET_FAKE(handleFileDelete);
    RESET_FAKE(handleColon); RESET_FAKE(handleUp); RESET_FAKE(handleDown);
    RESET_FAKE(handlePageUp); RESET_FAKE(handlePageDown);
    RESET_FAKE(handleLeft); RESET_FAKE(handleRight);
    RESET_FAKE(handleShiftLeft); RESET_FAKE(handleShiftRight);
    RESET_FAKE(handleHome); RESET_FAKE(handleEnd);
    RESET_FAKE(handleCtrlHome); RESET_FAKE(handleCtrlEnd);
    RESET_FAKE(handleShiftHome); RESET_FAKE(handleShiftEnd);
    RESET_FAKE(handleI); RESET_FAKE(handleA);
    RESET_FAKE(handleHistoryAction);
    RESET_FAKE(handleCtrlX); RESET_FAKE(handleCtrlC); RESET_FAKE(handleCtrlV);
    RESET_FAKE(handleCtrlF); RESET_FAKE(handleEscape); RESET_FAKE(handleCommand);
    RESET_FAKE(hasSelection); RESET_FAKE(deleteSelection);
    RESET_FAKE(caretReset); RESET_FAKE(SDL_GetTicks);
    RESET_FAKE(populateListCurrentLayer); RESET_FAKE(clearSelection);
    FFF_RESET_HISTORY();
    SDL_GetTicks_fake.return_val = 1000;
    hasSelection_fake.return_val = false;
}

void tearDown(void) {}

/* ============================================
 * handleKeys dispatch tests
 * ============================================ */

void test_handleKeys_tab(void) {
    AppRenderer app = createTestApp(COORDINATE_OPERATOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_TAB, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleTab_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_a_in_editor_insert_selects_all(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_INSERT);
    SDL_Event e = makeKeyEvent(SDLK_A, SDL_KMOD_CTRL);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleSelectAll_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_a_in_editor_general_appends(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_A, SDL_KMOD_CTRL);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleCtrlA_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_enter_in_editor_general_appends(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_RETURN, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleCtrlA_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_enter_in_operator_general(void) {
    AppRenderer app = createTestApp(COORDINATE_OPERATOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_RETURN, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleEnter_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_enter(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_INSERT);
    SDL_Event e = makeKeyEvent(SDLK_RETURN, SDL_KMOD_CTRL);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleCtrlEnter_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_i_operator_general(void) {
    AppRenderer app = createTestApp(COORDINATE_OPERATOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_I, SDL_KMOD_CTRL);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleCtrlIOperator_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_i_editor_general(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_I, SDL_KMOD_CTRL);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleCtrlI_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_d_operator_deletes_file(void) {
    AppRenderer app = createTestApp(COORDINATE_OPERATOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_D, SDL_KMOD_CTRL);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleFileDelete_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_d_editor_deletes_element(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_D, SDL_KMOD_CTRL);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleDelete_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_delete_key_operator(void) {
    AppRenderer app = createTestApp(COORDINATE_OPERATOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_DELETE, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleFileDelete_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_colon_in_general_modes(void) {
    AppRenderer app = createTestApp(COORDINATE_OPERATOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_COLON, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleColon_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_colon_blocked_in_insert(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_INSERT);
    SDL_Event e = makeKeyEvent(SDLK_COLON, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(0, handleColon_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_k_moves_up_in_operator(void) {
    AppRenderer app = createTestApp(COORDINATE_OPERATOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_K, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleUp_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_j_moves_down_in_editor(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_J, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleDown_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_up_arrow_in_search(void) {
    AppRenderer app = createTestApp(COORDINATE_SIMPLE_SEARCH);
    SDL_Event e = makeKeyEvent(SDLK_UP, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleUp_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_h_moves_left_in_operator(void) {
    AppRenderer app = createTestApp(COORDINATE_OPERATOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_H, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleLeft_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_l_moves_right_in_editor(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_L, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleRight_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_shift_left_in_insert(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_INSERT);
    SDL_Event e = makeKeyEvent(SDLK_LEFT, SDL_KMOD_SHIFT);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleShiftLeft_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_shift_right_in_search(void) {
    AppRenderer app = createTestApp(COORDINATE_SIMPLE_SEARCH);
    SDL_Event e = makeKeyEvent(SDLK_RIGHT, SDL_KMOD_SHIFT);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleShiftRight_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_z_undo(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_Z, SDL_KMOD_CTRL);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleHistoryAction_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_shift_z_redo(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_Z, SDL_KMOD_CTRL | SDL_KMOD_SHIFT);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleHistoryAction_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_x_cut(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_X, SDL_KMOD_CTRL);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleCtrlX_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_c_copy(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_C, SDL_KMOD_CTRL);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleCtrlC_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_v_paste(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_V, SDL_KMOD_CTRL);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleCtrlV_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_f_find(void) {
    AppRenderer app = createTestApp(COORDINATE_OPERATOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_F, SDL_KMOD_CTRL);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleCtrlF_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_escape(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_INSERT);
    SDL_Event e = makeKeyEvent(SDLK_ESCAPE, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleEscape_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_e_editor_mode(void) {
    AppRenderer app = createTestApp(COORDINATE_OPERATOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_E, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(COMMAND_EDITOR_MODE, app.currentCommand);
    TEST_ASSERT_EQUAL_INT(1, handleCommand_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_o_operator_mode(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_O, 0);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(COMMAND_OPERATOR_MODE, app.currentCommand);
    TEST_ASSERT_EQUAL_INT(1, handleCommand_fake.call_count);
    freeTestApp(&app);
}

void test_handleKeys_ctrl_a_operator_appends(void) {
    AppRenderer app = createTestApp(COORDINATE_OPERATOR_GENERAL);
    SDL_Event e = makeKeyEvent(SDLK_A, SDL_KMOD_CTRL);
    handleKeys(&app, &e);
    TEST_ASSERT_EQUAL_INT(1, handleCtrlAOperator_fake.call_count);
    freeTestApp(&app);
}

/* ============================================
 * handleInput tests
 * ============================================ */

void test_handleInput_null_text(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_INSERT);
    handleInput(&app, NULL);
    TEST_ASSERT_EQUAL_INT(0, app.inputBufferSize);
    freeTestApp(&app);
}

void test_handleInput_basic_insert(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_INSERT);
    handleInput(&app, "abc");
    TEST_ASSERT_EQUAL_STRING("abc", app.inputBuffer);
    TEST_ASSERT_EQUAL_INT(3, app.inputBufferSize);
    TEST_ASSERT_EQUAL_INT(3, app.cursorPosition);
    freeTestApp(&app);
}

void test_handleInput_insert_at_cursor(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_INSERT);
    handleInput(&app, "hello");
    app.cursorPosition = 2; // between "he" and "llo"
    handleInput(&app, "X");
    TEST_ASSERT_EQUAL_STRING("heXllo", app.inputBuffer);
    TEST_ASSERT_EQUAL_INT(6, app.inputBufferSize);
    TEST_ASSERT_EQUAL_INT(3, app.cursorPosition);
    freeTestApp(&app);
}

void test_handleInput_ignores_colon_in_empty_command_mode(void) {
    AppRenderer app = createTestApp(COORDINATE_COMMAND);
    handleInput(&app, ":");
    TEST_ASSERT_EQUAL_INT(0, app.inputBufferSize);
    freeTestApp(&app);
}

void test_handleInput_allows_colon_in_non_empty_command(void) {
    AppRenderer app = createTestApp(COORDINATE_COMMAND);
    handleInput(&app, "a");
    handleInput(&app, ":");
    TEST_ASSERT_EQUAL_STRING("a:", app.inputBuffer);
    freeTestApp(&app);
}

void test_handleInput_triggers_search_in_simple_search(void) {
    AppRenderer app = createTestApp(COORDINATE_SIMPLE_SEARCH);
    handleInput(&app, "t");
    TEST_ASSERT_EQUAL_INT(1, populateListCurrentLayer_fake.call_count);
    freeTestApp(&app);
}

void test_handleInput_no_search_in_editor_insert(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_INSERT);
    handleInput(&app, "t");
    TEST_ASSERT_EQUAL_INT(0, populateListCurrentLayer_fake.call_count);
    freeTestApp(&app);
}

void test_handleInput_sets_needsRedraw(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_INSERT);
    handleInput(&app, "x");
    TEST_ASSERT_TRUE(app.needsRedraw);
    freeTestApp(&app);
}

void test_handleInput_resets_caret(void) {
    AppRenderer app = createTestApp(COORDINATE_EDITOR_INSERT);
    handleInput(&app, "x");
    TEST_ASSERT_EQUAL_INT(1, caretReset_fake.call_count);
    freeTestApp(&app);
}

/* ============================================
 * Main
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // handleKeys dispatch
    RUN_TEST(test_handleKeys_tab);
    RUN_TEST(test_handleKeys_ctrl_a_in_editor_insert_selects_all);
    RUN_TEST(test_handleKeys_ctrl_a_in_editor_general_appends);
    RUN_TEST(test_handleKeys_enter_in_editor_general_appends);
    RUN_TEST(test_handleKeys_enter_in_operator_general);
    RUN_TEST(test_handleKeys_ctrl_enter);
    RUN_TEST(test_handleKeys_ctrl_i_operator_general);
    RUN_TEST(test_handleKeys_ctrl_i_editor_general);
    RUN_TEST(test_handleKeys_ctrl_d_operator_deletes_file);
    RUN_TEST(test_handleKeys_ctrl_d_editor_deletes_element);
    RUN_TEST(test_handleKeys_delete_key_operator);
    RUN_TEST(test_handleKeys_colon_in_general_modes);
    RUN_TEST(test_handleKeys_colon_blocked_in_insert);
    RUN_TEST(test_handleKeys_k_moves_up_in_operator);
    RUN_TEST(test_handleKeys_j_moves_down_in_editor);
    RUN_TEST(test_handleKeys_up_arrow_in_search);
    RUN_TEST(test_handleKeys_h_moves_left_in_operator);
    RUN_TEST(test_handleKeys_l_moves_right_in_editor);
    RUN_TEST(test_handleKeys_shift_left_in_insert);
    RUN_TEST(test_handleKeys_shift_right_in_search);
    RUN_TEST(test_handleKeys_ctrl_z_undo);
    RUN_TEST(test_handleKeys_ctrl_shift_z_redo);
    RUN_TEST(test_handleKeys_ctrl_x_cut);
    RUN_TEST(test_handleKeys_ctrl_c_copy);
    RUN_TEST(test_handleKeys_ctrl_v_paste);
    RUN_TEST(test_handleKeys_ctrl_f_find);
    RUN_TEST(test_handleKeys_escape);
    RUN_TEST(test_handleKeys_e_editor_mode);
    RUN_TEST(test_handleKeys_o_operator_mode);
    RUN_TEST(test_handleKeys_ctrl_a_operator_appends);

    // handleInput
    RUN_TEST(test_handleInput_null_text);
    RUN_TEST(test_handleInput_basic_insert);
    RUN_TEST(test_handleInput_insert_at_cursor);
    RUN_TEST(test_handleInput_ignores_colon_in_empty_command_mode);
    RUN_TEST(test_handleInput_allows_colon_in_non_empty_command);
    RUN_TEST(test_handleInput_triggers_search_in_simple_search);
    RUN_TEST(test_handleInput_no_search_in_editor_insert);
    RUN_TEST(test_handleInput_sets_needsRedraw);
    RUN_TEST(test_handleInput_resets_caret);

    return UNITY_END();
}

#pragma once

#include <json-c/json.h>
#include <accesskit.h>
#include <ffon.h>

#include "main.h"
#include "provider.h"
#include "accesskit_sdl.h"

// Constants
#define MAX_LINE_LENGTH 4096
#define MAX_URI_LENGTH 4096
#define MAX_FFON_ELEMENTS 10000
#define UNDO_HISTORY_SIZE 500
#define DELTA_MS 400
#define INDENT_CHARS 4
#define FONT_SIZE_PT 12.0f
#define TEXT_PADDING 4.0f

// Colors
#define COLOR_BG 0x1E1E1EFF
#define COLOR_TEXT 0xD4D4D4FF
#define COLOR_SC_HIGHLIGHT 0xC0ECB8FF
#define COLOR_BORDER 0xFFE5B4FF
#define COLOR_ORANGE 0xFFA500FF
#define COLOR_RED 0xFF0000FF
#define COLOR_LIGHT_GREEN 0xC0ECB8FF
#define COLOR_DARK_GREEN 0x1C4414FF
#define COLOR_DIMGREY 0x696969FF
#define COLOR_DARK_GREY 0x333333FF
#define COLOR_LIGHTGREY 0xD3D3D3FF

// Enums
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
    COMMAND_EDITOR_MODE,
    COMMAND_OPERATOR_MODE
} Command;

// Forward declarations
typedef struct SiCompassApplication SiCompassApplication;
typedef struct CaretState CaretState;

// Forward declaration for window_state
struct AppRenderer;

// Window state for thread-safe accessibility state management
struct windowState {
    accesskit_node_id focus;
    const char *announcement;
    SDL_Mutex *mutex;
    struct AppRenderer *appRenderer;  // Pointer to access list data for accessibility tree
};

// Action handler state for routing accessibility actions to SDL events
struct actionHandlerState {
    Uint32 eventType;
    SDL_WindowID windowId;
};

// Undo history entry
typedef struct {
    IdArray id;
    Task task;
    char *line;
    bool isKey;
} UndoEntry;

// List item for right panel
typedef struct {
    IdArray id;
    char *value;
} ListItem;

// Main application state
typedef struct AppRenderer {
    // FFON data
    FfonElement **ffon;
    int ffonCount;
    int ffonCapacity;

    // Current state
    IdArray currentId;
    IdArray previousId;
    IdArray currentInsertId;
    Coordinate currentCoordinate;
    Coordinate previousCoordinate;
    Command currentCommand;

    // UI state
    char *inputBuffer;
    int inputBufferSize;
    int inputBufferCapacity;
    int cursorPosition;
    int scrollOffset;

    // Right panel
    ListItem *totalListCurrentLayer;
    int totalListCount;
    ListItem *filteredListCurrentLayer;
    int filteredListCount;
    int listIndex;

    // History
    UndoEntry *undoHistory;
    int undoHistoryCount;
    int undoPosition;

    // Timing
    uint64_t lastKeypressTime;

    // Caret state
    CaretState *caretState;
    int currentElementX;  // X position of current element during rendering
    int currentElementY;  // Y position of current element during rendering
    bool currentElementIsObject;  // Whether current element is an object (needs colon)
    char originalKey[MAX_LINE_LENGTH];  // Original key when editing an object in insert mode

    // Cut/copy/paste buffer
    FfonElement *clipboard;

    // Flags
    bool running;
    bool needsRedraw;
    bool inputDown;

    // Error message
    char errorMessage[256];

    // Current URI for provider-based navigation
    char currentUri[MAX_URI_LENGTH];

    // AccessKit SDL adapter (cross-platform wrapper)
    struct accesskit_sdl_adapter accesskitAdapter;
    accesskit_node_id accesskitRootId;
    accesskit_node_id accesskitLiveRegionId;

    // Window state for thread-safe accessibility
    struct windowState state;

    // Action handler state for accessibility events
    struct actionHandlerState actionHandler;
} AppRenderer;

// Function declarations

// Main entry point
void mainLoop(SiCompassApplication* app);

// Initialization and cleanup
SiCompassApplication* appRendererCreate(SiCompassApplication* app);
void appRendererDestroy(AppRenderer *appRenderer);

// FFON operations
FfonElement* ffonElementCreateString(const char *str);
FfonElement* ffonElementCreateObject(const char *key);
void ffonElementDestroy(FfonElement *elem);
FfonElement* ffonElementClone(FfonElement *elem);
FfonObject* ffonObjectCreate(const char *key);
void ffonObjectDestroy(FfonObject *obj);
void ffonObjectAddElement(FfonObject *obj, FfonElement *elem);
void ffonObjectInsertElement(FfonObject *obj, FfonElement *elem, int index);

// JSON loading
bool loadJsonFile(AppRenderer *appRenderer, const char *filename);
FfonElement* parseJsonValue(json_object *jobj);

// Navigation and state updates
void updateState(AppRenderer *appRenderer, Task task, History history);
void updateIds(AppRenderer *appRenderer, bool isKey, Task task, History history);
void updateFfon(AppRenderer *appRenderer, const char *line, bool isKey, Task task, History history);
void updateHistory(AppRenderer *appRenderer, Task task, bool isKey, const char *line, History history);

// Event handling
void handleKeys(AppRenderer *appRenderer, SDL_Event *event);
void handleTab(AppRenderer *appRenderer);
void handleInput(AppRenderer *appRenderer, const char *text);
void handleCtrlA(AppRenderer *appRenderer, History history);
void handleEnter(AppRenderer *appRenderer, History history);
void handleCtrlEnter(AppRenderer *appRenderer, History history);
void handleCtrlI(AppRenderer *appRenderer, History history);
void handleDelete(AppRenderer *appRenderer, History history);
void handleColon(AppRenderer *appRenderer);
void handleUp(AppRenderer *appRenderer);
void handleDown(AppRenderer *appRenderer);
void handleLeft(AppRenderer *appRenderer);
void handleRight(AppRenderer *appRenderer);
void handleI(AppRenderer *appRenderer);
void handleA(AppRenderer *appRenderer);
void handleHistoryAction(AppRenderer *appRenderer, History history);
void handleCtrlX(AppRenderer *appRenderer);
void handleCtrlC(AppRenderer *appRenderer);
void handleCtrlV(AppRenderer *appRenderer);
void handleFind(AppRenderer *appRenderer);
void handleEscape(AppRenderer *appRenderer);
void handleCommand(AppRenderer *appRenderer);

// Right panel
void createListCurrentLayer(AppRenderer *appRenderer);
void populateListCurrentLayer(AppRenderer *appRenderer, const char *searchString);
void clearListCurrentLayer(AppRenderer *appRenderer);

// Rendering
void updateView(SiCompassApplication *app);
void renderSimpleSearch(SiCompassApplication *app);
// void renderHierarchy(SiCompassApplication *app);
void renderInteraction(SiCompassApplication *app);
void renderLine(SiCompassApplication *app, FfonElement *elem, const IdArray *id, int indent, int *yPos);
int renderText(SiCompassApplication *app, const char *text, int x, int y, uint32_t color, bool highlight);

// Caret functions
CaretState* caretCreate();
void caretDestroy(CaretState* caret);
void caretUpdate(CaretState* caret, uint64_t currentTime);
void caretReset(CaretState* caret, uint64_t currentTime);
void caretRender(SiCompassApplication* app, CaretState* caret,
                 const char* text, int x, int y, int cursorPosition,
                 uint32_t color);

// Utility functions
const char* coordinateToString(Coordinate coord);
const char* taskToString(Task task);
bool isLineKey(const char *line);
char* escapeHtmlToText(const char *html);
void setErrorMessage(AppRenderer *appRenderer, const char *message);

// AccessKit accessibility functions
void accesskitInit(SiCompassApplication *app);
void accesskitDestroy(AppRenderer *appRenderer);
void accesskitSpeak(AppRenderer *appRenderer, const char *text);
void accesskitSpeakCurrentItem(AppRenderer *appRenderer);
void accesskitSpeakModeChange(AppRenderer *appRenderer, const char *context);
void accesskitUpdateWindowFocus(AppRenderer *appRenderer, bool isFocused);

// Window state functions for thread-safe accessibility
void windowStateInit(struct windowState *state, accesskit_node_id initialFocus, AppRenderer *appRenderer);
void windowStateDestroy(struct windowState *state);
void windowStateLock(struct windowState *state);
void windowStateUnlock(struct windowState *state);
void windowStateSetFocus(struct windowState *state, struct accesskit_sdl_adapter *adapter, accesskit_node_id new_focus);

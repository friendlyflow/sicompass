#pragma once

#ifdef _WIN32
#include <win_compat.h>
#endif

#include <json-c/json.h>
#include <accesskit.h>
#include <ffon.h>
#include <SDL3/SDL.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

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

// Runtime color palette (theme-aware)
typedef struct {
    uint32_t background;
    uint32_t text;
    uint32_t headerseparator;
    uint32_t selected;
    uint32_t extsearch;
    uint32_t scrollsearch;
    uint32_t error;
} ColorPalette;

extern const ColorPalette PALETTE_DARK;
extern const ColorPalette PALETTE_LIGHT;

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
    FfonElement *prevElement;  // Element state before operation (NULL for insert/append)
    FfonElement *newElement;   // Element state after operation (NULL for delete/cut)
} UndoEntry;

// List item for right panel
typedef struct {
    IdArray id;
    char *label;
    char *data;      // breadcrumb for display (extended search)
    char *navPath;   // non-NULL = path-based navigation (deep search items not in FFON tree)
} ListItem;

// Main application state
typedef struct AppRenderer {
    // FFON data
    FfonElement **ffon;
    Provider **providers;  // parallel to ffon: providers[i] owns ffon[i]
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
    int selectionAnchor;  // -1 = no selection; byte offset of anchor otherwise
    char inputPrefix[MAX_LINE_LENGTH];   // non-editable text before <input>
    char inputSuffix[MAX_LINE_LENGTH];   // non-editable text after </input>
    int scrollOffset;
    int textScrollOffset;      // Line offset for scroll mode text viewing
    int textScrollLineCount;   // Total wrapped lines from last renderScroll (for clamping)
    int renderClipTopY;        // Skip rendering lines with Y < this value (0 = no clipping)
    int scrollSearchMatchCount;    // Total number of search matches in scroll text
    int scrollSearchCurrentMatch;  // Index of currently-focused match (0-based)

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
    int currentElementX;  // X position of current element during rendering (after prefix)
    int currentElementBaseX;  // X position before prefix (for multi-line newline rendering)
    int currentElementY;  // Y position of current element during rendering
    bool currentElementIsObject;  // Whether current element is an object (needs colon)
    char originalKey[MAX_LINE_LENGTH];  // Original key when editing an object in insert mode

    // Cut/copy/paste buffer (FFON element level)
    FfonElement *clipboard;

    // File clipboard for file browser cut/copy/paste (filesystem level)
    // fileClipboardPath is empty string when no file is in the clipboard.
    // For cut: full path to the cached copy in the clipboard cache dir.
    // For copy: full path to the original source file/dir.
    char fileClipboardPath[MAX_URI_LENGTH];
    bool fileClipboardIsCut;

    // Flags
    bool running;
    bool needsRedraw;
    bool inputDown;
    bool prefixedInsertMode;  // true when in OPERATOR_INSERT via Ctrl+I/Ctrl+A (prefix-based creation)
    bool pendingSaveAs;       // true when in COMMAND mode for "save as" filename input
    bool pendingFileBrowserSaveAs; // true when in file browser for save-as flow
    bool pendingFileBrowserOpen;   // true when in file browser for open/load flow

    // Application pointer for accessing window dimensions and font metrics
    SiCompassApplication *app;

    // Cached layout metrics (updated by render loop, used by handlers for scroll/page calculations)
    int windowHeight;
    int cachedLineHeight;

    // Error message
    char errorMessage[256];

    // Current URI for provider-based navigation
    char currentUri[MAX_URI_LENGTH];

    // Active provider command name (when currentCommand == COMMAND_PROVIDER)
    char providerCommandName[64];

    // AccessKit SDL adapter (cross-platform wrapper)
    struct accesskit_sdl_adapter accesskitAdapter;
    accesskit_node_id accesskitRootId;
    accesskit_node_id accesskitElementId;

    // Window state for thread-safe accessibility
    struct windowState state;

    // Action handler state for accessibility events
    struct actionHandlerState actionHandler;

    // Active color palette (points to PALETTE_DARK or PALETTE_LIGHT)
    const ColorPalette *palette;

    // Dashboard image path (copied from active provider when entering dashboard mode)
    char dashboardImagePath[MAX_URI_LENGTH];

    // Configurable save/load folder path (resolved absolute path, set from settings)
    char saveFolderPath[4096];

    // Remembered save file path (empty = no file loaded/saved yet)
    char currentSavePath[MAX_URI_LENGTH];
    // "Save as via file browser" state
    int saveAsSourceRootIdx;              // Source provider index to save data from
    IdArray saveAsReturnId;               // Navigation state to restore on return
} AppRenderer;

// Function declarations

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
void updateHistory(AppRenderer *appRenderer, Task task, const IdArray *id, FfonElement *prevElement, FfonElement *newElement, History history);

// Event handling
void handleKeys(AppRenderer *appRenderer, SDL_Event *event);
void handleTab(AppRenderer *appRenderer);
void handleInput(AppRenderer *appRenderer, const char *text);
void handleCtrlA(AppRenderer *appRenderer, History history);
void handleEnter(AppRenderer *appRenderer, History history);
void handleCtrlEnter(AppRenderer *appRenderer, History history);
void handleCtrlI(AppRenderer *appRenderer, History history);
void handleCtrlIOperator(AppRenderer *appRenderer);
void handleCtrlAOperator(AppRenderer *appRenderer);
void handleDelete(AppRenderer *appRenderer, History history);
void handleFileDelete(AppRenderer *appRenderer);
void handleFileCut(AppRenderer *appRenderer);
void handleFileCopy(AppRenderer *appRenderer);
void handleFilePaste(AppRenderer *appRenderer);
void handleColon(AppRenderer *appRenderer);
void handleUp(AppRenderer *appRenderer);
void handleDown(AppRenderer *appRenderer);
void handleUpInsert(AppRenderer *appRenderer);
void handleDownInsert(AppRenderer *appRenderer);
void handleShiftUpInsert(AppRenderer *appRenderer);
void handleShiftDownInsert(AppRenderer *appRenderer);
void handlePageUp(AppRenderer *appRenderer);
void handlePageDown(AppRenderer *appRenderer);
void handleLeft(AppRenderer *appRenderer);
void handleRight(AppRenderer *appRenderer);
void handleI(AppRenderer *appRenderer);
void handleA(AppRenderer *appRenderer);
void handleHistoryAction(AppRenderer *appRenderer, History history);
void handleCtrlX(AppRenderer *appRenderer);
void handleCtrlC(AppRenderer *appRenderer);
void handleCtrlV(AppRenderer *appRenderer);
void handleCtrlF(AppRenderer *appRenderer);
void handleEscape(AppRenderer *appRenderer);
void handleCommand(AppRenderer *appRenderer);
void handleHome(AppRenderer *appRenderer);
void handleEnd(AppRenderer *appRenderer);
void handleCtrlHome(AppRenderer *appRenderer);
void handleCtrlEnd(AppRenderer *appRenderer);
void handleShiftHome(AppRenderer *appRenderer);
void handleShiftEnd(AppRenderer *appRenderer);
void handleSelectAll(AppRenderer *appRenderer);
void handleShiftLeft(AppRenderer *appRenderer);
void handleShiftRight(AppRenderer *appRenderer);
void handleDashboard(AppRenderer *appRenderer);
void handleF5(AppRenderer *appRenderer);
void handleSaveProviderConfig(AppRenderer *appRenderer);
void handleLoadProviderConfig(AppRenderer *appRenderer);
void handleSaveAsProviderConfig(AppRenderer *appRenderer);
bool hasSelection(AppRenderer *appRenderer);
void clearSelection(AppRenderer *appRenderer);
void getSelectionRange(AppRenderer *appRenderer, int *start, int *end);
void deleteSelection(AppRenderer *appRenderer);

// Right panel
void createListCurrentLayer(AppRenderer *appRenderer);
void createListExtendedSearch(AppRenderer *appRenderer);
void populateListCurrentLayer(AppRenderer *appRenderer, const char *searchString);
void clearListCurrentLayer(AppRenderer *appRenderer);

// Caret functions
CaretState* caretCreate();
void caretDestroy(CaretState* caret);
void caretUpdate(CaretState* caret, uint64_t currentTime);
void caretReset(CaretState* caret, uint64_t currentTime);

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
void accesskitSpeakCurrentElement(AppRenderer *appRenderer);
void accesskitSpeakModeChange(AppRenderer *appRenderer, const char *context);
void accesskitUpdateWindowFocus(AppRenderer *appRenderer, bool isFocused);

// Window state functions for thread-safe accessibility
void windowStateInit(struct windowState *state, accesskit_node_id initialFocus, AppRenderer *appRenderer);
void windowStateDestroy(struct windowState *state);
void windowStateLock(struct windowState *state);
void windowStateUnlock(struct windowState *state);

#pragma once

#include <json-c/json.h>

#include "main.h"

// Constants
#define MAX_ID_DEPTH 32
#define MAX_LINE_LENGTH 4096
#define MAX_FFON_ELEMENTS 10000
#define UNDO_HISTORY_SIZE 500
#define DELTA_MS 400
#define INDENT_CHARS 4

// Colors
#define COLOR_BG 0x1E1E1EFF
#define COLOR_TEXT 0xD4D4D4FF
#define COLOR_HIGHLIGHT 0xC0ECB8FF
#define COLOR_BORDER 0xFFE5B4FF
#define COLOR_ORANGE 0xFFA500FF
#define COLOR_RED 0xFF0000FF
#define COLOR_GREEN 0xC0ECB8FF

// Enums
typedef enum {
    COORDINATE_LEFT_VISITOR_GENERAL,
    COORDINATE_LEFT_VISITOR_INSERT,
    COORDINATE_LEFT_EDITOR_GENERAL,
    COORDINATE_LEFT_EDITOR_INSERT,
    COORDINATE_LEFT_EDITOR_NORMAL,
    COORDINATE_LEFT_EDITOR_VISUAL,
    COORDINATE_RIGHT_INFO,
    COORDINATE_RIGHT_COMMAND,
    COORDINATE_RIGHT_FIND
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
    COMMAND_VISITOR_MODE
} Command;

// Forward declarations
typedef struct SiCompassApplication SiCompassApplication;
typedef struct FfonElement FfonElement;
typedef struct FfonObject FfonObject;

// FFON data structures
struct FfonElement {
    enum { FFON_STRING, FFON_OBJECT } type;
    union {
        char *string;
        FfonObject *object;
    } data;
};

struct FfonObject {
    char *key;
    FfonElement **elements;
    int count;
    int capacity;
};

// ID array structure
typedef struct {
    int ids[MAX_ID_DEPTH];
    int depth;
} IdArray;

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
    ListItem *totalListRight;
    int totalListCount;
    ListItem *filteredListRight;
    int filteredListCount;
    int listIndex;

    // History
    UndoEntry *undoHistory;
    int undoHistoryCount;
    int undoPosition;

    // Timing
    uint64_t lastKeypressTime;

    // Cut/copy/paste buffer
    FfonElement *clipboard;

    // Flags
    bool running;
    bool needsRedraw;
    bool inputDown;

    // Error message
    char errorMessage[256];
} AppRenderer;

// Function declarations

// Main entry point
void startApp(SiCompassApplication* app);

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

// JSON loading
bool loadJsonFile(AppRenderer *appRenderer, const char *filename);
FfonElement* parseJsonValue(json_object *jobj);

// ID array operations
void idArrayInit(IdArray *arr);
void idArrayCopy(IdArray *dst, const IdArray *src);
bool idArrayEqual(const IdArray *a, const IdArray *b);
void idArrayPush(IdArray *arr, int val);
int idArrayPop(IdArray *arr);
char* idArrayToString(const IdArray *arr);

// Navigation and state updates
void updateState(AppRenderer *appRenderer, Task task, History history);
void updateIds(AppRenderer *appRenderer, bool isKey, Task task, History history);
void updateFfon(AppRenderer *appRenderer, const char *line, bool isKey, Task task, History history);
void updateHistory(AppRenderer *appRenderer, Task task, bool isKey, const char *line, History history);

// Navigation helpers
bool nextLayerExists(AppRenderer *appRenderer);
int getMaxIdInCurrent(AppRenderer *appRenderer);
FfonElement** getFfonAtId(AppRenderer *appRenderer, const IdArray *id, int *outCount);

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
void handleCcp(AppRenderer *appRenderer, Task task);
void handleFind(AppRenderer *appRenderer);
void handleEscape(AppRenderer *appRenderer);
void handleCommand(AppRenderer *appRenderer);

// Right panel
void createListRight(AppRenderer *appRenderer);
void populateListRight(AppRenderer *appRenderer, const char *searchString);
void clearListRight(AppRenderer *appRenderer);

// Rendering
void updateView(AppRenderer *appRenderer);
void renderLeftPanel(AppRenderer *appRenderer);
void renderRightPanel(AppRenderer *appRenderer);
void renderLine(AppRenderer *appRenderer, FfonElement *elem, const IdArray *id, int indent, int *yPos);
void renderText(AppRenderer *appRenderer, const char *text, int x, int y, uint32_t color, bool highlight);

// Utility functions
const char* coordinateToString(Coordinate coord);
const char* taskToString(Task task);
bool isLineKey(const char *line);
char* escapeHtmlToText(const char *html);
void setErrorMessage(AppRenderer *appRenderer, const char *message);

#pragma once

#include <json-c/json.h>

#include "main.h"

// Constants
#define MAX_ID_DEPTH 32
#define MAX_LINE_LENGTH 4096
#define MAX_SFON_ELEMENTS 10000
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
typedef struct SfonElement SfonElement;
typedef struct SfonObject SfonObject;

// SFON data structures
struct SfonElement {
    enum { SFON_STRING, SFON_OBJECT } type;
    union {
        char *string;
        SfonObject *object;
    } data;
};

struct SfonObject {
    char *key;
    SfonElement **elements;
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
    // SFON data
    SfonElement **sfon;
    int sfonCount;
    int sfonCapacity;

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
    SfonElement *clipboard;

    // Flags
    bool running;
    bool needsRedraw;
    bool inputDown;

    // Error message
    char errorMessage[256];
} AppRenderer;

// Function declarations

// Main entry point
void updateView(SiCompassApplication* app);

// Initialization and cleanup
SiCompassApplication* editorStateCreate(void);
void editorStateDestroy(SiCompassApplication *state);
bool initSdl(SiCompassApplication *state);
void cleanupSdl(SiCompassApplication *state);

// SFON operations
SfonElement* sfonElementCreateString(const char *str);
SfonElement* sfonElementCreateObject(const char *key);
void sfonElementDestroy(SfonElement *elem);
SfonElement* sfonElementClone(SfonElement *elem);
SfonObject* sfonObjectCreate(const char *key);
void sfonObjectDestroy(SfonObject *obj);
void sfonObjectAddElement(SfonObject *obj, SfonElement *elem);

// JSON loading
bool loadJsonFile(SiCompassApplication *state, const char *filename);
SfonElement* parseJsonValue(json_object *jobj);

// ID array operations
void idArrayInit(IdArray *arr);
void idArrayCopy(IdArray *dst, const IdArray *src);
bool idArrayEqual(const IdArray *a, const IdArray *b);
void idArrayPush(IdArray *arr, int val);
int idArrayPop(IdArray *arr);
char* idArrayToString(const IdArray *arr);

// Navigation and state updates
void updateState(SiCompassApplication *state, Task task, History history);
void updateIds(SiCompassApplication *state, bool isKey, Task task, History history);
void updateSfon(SiCompassApplication *state, const char *line, bool isKey, Task task, History history);
void updateHistory(SiCompassApplication *state, Task task, bool isKey, const char *line, History history);

// Navigation helpers
bool nextLayerExists(SiCompassApplication *state);
int getMaxIdInCurrent(SiCompassApplication *state);
SfonElement** getSfonAtId(SiCompassApplication *state, const IdArray *id, int *outCount);

// Event handling
void handleKeys(SiCompassApplication *state, SDL_Event *event);
void handleTab(SiCompassApplication *state);
void handleInput(SiCompassApplication *state, const char *text);
void handleCtrlA(SiCompassApplication *state, History history);
void handleEnter(SiCompassApplication *state, History history);
void handleCtrlEnter(SiCompassApplication *state, History history);
void handleCtrlI(SiCompassApplication *state, History history);
void handleDelete(SiCompassApplication *state, History history);
void handleColon(SiCompassApplication *state);
void handleUp(SiCompassApplication *state);
void handleDown(SiCompassApplication *state);
void handleLeft(SiCompassApplication *state);
void handleRight(SiCompassApplication *state);
void handleI(SiCompassApplication *state);
void handleA(SiCompassApplication *state);
void handleHistoryAction(SiCompassApplication *state, History history);
void handleCcp(SiCompassApplication *state, Task task);
void handleFind(SiCompassApplication *state);
void handleEscape(SiCompassApplication *state);
void handleCommand(SiCompassApplication *state);

// Right panel
void createListRight(SiCompassApplication *state);
void populateListRight(SiCompassApplication *state, const char *searchString);
void clearListRight(SiCompassApplication *state);

// Rendering
void updateView(SiCompassApplication *state);
void renderLeftPanel(SiCompassApplication *state);
void renderRightPanel(SiCompassApplication *state);
void renderLine(SiCompassApplication *state, SfonElement *elem, const IdArray *id, int indent, int *yPos);
void renderText(SiCompassApplication *state, const char *text, int x, int y, uint32_t color, bool highlight);

// Utility functions
const char* coordinateToString(Coordinate coord);
const char* taskToString(Task task);
bool isLineKey(const char *line);
char* escapeHtmlToText(const char *html);
void setErrorMessage(SiCompassApplication *state, const char *message);

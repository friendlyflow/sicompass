#ifndef FFON_EDITOR_H
#define FFON_EDITOR_H

#include <SDL3/SDL.h>
#include <SDL3_ttf/SDL_ttf.h>
#include <json-c/json.h>
#include <cglm/cglm.h>
#include <stdbool.h>
#include <time.h>

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
typedef struct {
    // SDL components
    SDL_Window *window;
    SDL_Renderer *renderer;
    TTF_Font *font;
    int fontHeight;
    int charWidth;

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
} EditorState;

// Function declarations

// Initialization and cleanup
EditorState* editorStateCreate(void);
void editorStateDestroy(EditorState *state);
bool initSdl(EditorState *state);
void cleanupSdl(EditorState *state);

// FFON operations
FfonElement* ffonElementCreateString(const char *str);
FfonElement* ffonElementCreateObject(const char *key);
void ffonElementDestroy(FfonElement *elem);
FfonElement* ffonElementClone(FfonElement *elem);
FfonObject* ffonObjectCreate(const char *key);
void ffonObjectDestroy(FfonObject *obj);
void ffonObjectAddElement(FfonObject *obj, FfonElement *elem);

// JSON loading
bool loadJsonFile(EditorState *state, const char *filename);
FfonElement* parseJsonValue(json_object *jobj);

// ID array operations
void idArrayInit(IdArray *arr);
void idArrayCopy(IdArray *dst, const IdArray *src);
bool idArrayEqual(const IdArray *a, const IdArray *b);
void idArrayPush(IdArray *arr, int val);
int idArrayPop(IdArray *arr);
char* idArrayToString(const IdArray *arr);

// Navigation and state updates
void updateState(EditorState *state, Task task, History history);
void updateIds(EditorState *state, bool isKey, Task task, History history);
void updateFfon(EditorState *state, const char *line, bool isKey, Task task, History history);
void updateHistory(EditorState *state, Task task, bool isKey, const char *line, History history);

// Navigation helpers
bool nextLayerExists(EditorState *state);
int getMaxIdInCurrent(EditorState *state);
FfonElement** getFfonAtId(EditorState *state, const IdArray *id, int *outCount);

// Event handling
void handleKeys(EditorState *state, SDL_Event *event);
void handleTab(EditorState *state);
void handleInput(EditorState *state, const char *text);
void handleCtrlA(EditorState *state, History history);
void handleEnter(EditorState *state, History history);
void handleCtrlEnter(EditorState *state, History history);
void handleCtrlI(EditorState *state, History history);
void handleDelete(EditorState *state, History history);
void handleColon(EditorState *state);
void handleUp(EditorState *state);
void handleDown(EditorState *state);
void handleLeft(EditorState *state);
void handleRight(EditorState *state);
void handleI(EditorState *state);
void handleA(EditorState *state);
void handleHistoryAction(EditorState *state, History history);
void handleCcp(EditorState *state, Task task);
void handleFind(EditorState *state);
void handleEscape(EditorState *state);
void handleCommand(EditorState *state);

// Right panel
void createListRight(EditorState *state);
void populateListRight(EditorState *state, const char *searchString);
void clearListRight(EditorState *state);

// Rendering
void updateView(EditorState *state);
void renderLeftPanel(EditorState *state);
void renderRightPanel(EditorState *state);
void renderLine(EditorState *state, FfonElement *elem, const IdArray *id, int indent, int *yPos);
void renderText(EditorState *state, const char *text, int x, int y, uint32_t color, bool highlight);

// Utility functions
const char* coordinateToString(Coordinate coord);
const char* taskToString(Task task);
bool isLineKey(const char *line);
char* escapeHtmlToText(const char *html);
void setErrorMessage(EditorState *state, const char *message);

#endif // FFON_EDITOR_H

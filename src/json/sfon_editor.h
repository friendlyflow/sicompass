#ifndef SFON_EDITOR_H
#define SFON_EDITOR_H

#include <SDL3/SDL.h>
#include <SDL3_ttf/SDL_ttf.h>
#include <json-c/json.h>
#include <cglm/cglm.h>
#include <stdbool.h>
#include <time.h>

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
    bool is_key;
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
    int font_height;
    int char_width;
    
    // SFON data
    SfonElement **sfon;
    int sfon_count;
    int sfon_capacity;
    
    // Current state
    IdArray current_id;
    IdArray previous_id;
    IdArray current_insert_id;
    Coordinate current_coordinate;
    Coordinate previous_coordinate;
    Command current_command;
    
    // UI state
    char *input_buffer;
    int input_buffer_size;
    int input_buffer_capacity;
    int cursor_position;
    int scroll_offset;
    
    // Right panel
    ListItem *total_list_right;
    int total_list_count;
    ListItem *filtered_list_right;
    int filtered_list_count;
    int list_index;
    
    // History
    UndoEntry *undo_history;
    int undo_history_count;
    int undo_position;
    
    // Timing
    uint64_t last_keypress_time;
    
    // Cut/copy/paste buffer
    SfonElement *clipboard;
    
    // Flags
    bool running;
    bool needs_redraw;
    bool input_down;
    
    // Error message
    char error_message[256];
} EditorState;

// Function declarations

// Initialization and cleanup
EditorState* editor_state_create(void);
void editor_state_destroy(EditorState *state);
bool init_sdl(EditorState *state);
void cleanup_sdl(EditorState *state);

// SFON operations
SfonElement* sfon_element_create_string(const char *str);
SfonElement* sfon_element_create_object(const char *key);
void sfon_element_destroy(SfonElement *elem);
SfonElement* sfon_element_clone(SfonElement *elem);
SfonObject* sfon_object_create(const char *key);
void sfon_object_destroy(SfonObject *obj);
void sfon_object_add_element(SfonObject *obj, SfonElement *elem);

// JSON loading
bool load_json_file(EditorState *state, const char *filename);
SfonElement* parse_json_value(json_object *jobj);

// ID array operations
void id_array_init(IdArray *arr);
void id_array_copy(IdArray *dst, const IdArray *src);
bool id_array_equal(const IdArray *a, const IdArray *b);
void id_array_push(IdArray *arr, int val);
int id_array_pop(IdArray *arr);
char* id_array_to_string(const IdArray *arr);

// Navigation and state updates
void update_state(EditorState *state, Task task, History history);
void update_ids(EditorState *state, bool is_key, Task task, History history);
void update_sfon(EditorState *state, const char *line, bool is_key, Task task, History history);
void update_history(EditorState *state, Task task, bool is_key, const char *line, History history);

// Navigation helpers
bool next_layer_exists(EditorState *state);
int get_max_id_in_current(EditorState *state);
SfonElement** get_sfon_at_id(EditorState *state, const IdArray *id, int *out_count);

// Event handling
void handle_keys(EditorState *state, SDL_Event *event);
void handle_tab(EditorState *state);
void handle_input(EditorState *state, const char *text);
void handle_ctrl_a(EditorState *state, History history);
void handle_enter(EditorState *state, History history);
void handle_ctrl_enter(EditorState *state, History history);
void handle_ctrl_i(EditorState *state, History history);
void handle_delete(EditorState *state, History history);
void handle_colon(EditorState *state);
void handle_up(EditorState *state);
void handle_down(EditorState *state);
void handle_left(EditorState *state);
void handle_right(EditorState *state);
void handle_i(EditorState *state);
void handle_a(EditorState *state);
void handle_history_action(EditorState *state, History history);
void handle_ccp(EditorState *state, Task task);
void handle_find(EditorState *state);
void handle_escape(EditorState *state);
void handle_command(EditorState *state);

// Right panel
void create_list_right(EditorState *state);
void populate_list_right(EditorState *state, const char *search_string);
void clear_list_right(EditorState *state);

// Rendering
void update_view(EditorState *state);
void render_left_panel(EditorState *state);
void render_right_panel(EditorState *state);
void render_line(EditorState *state, SfonElement *elem, const IdArray *id, int indent, int *y_pos);
void render_text(EditorState *state, const char *text, int x, int y, uint32_t color, bool highlight);

// Utility functions
const char* coordinate_to_string(Coordinate coord);
const char* task_to_string(Task task);
bool is_line_key(const char *line);
char* escape_html_to_text(const char *html);
void set_error_message(EditorState *state, const char *message);

#endif // SFON_EDITOR_H

#include "sfon_editor.h"
#include <string.h>

void handle_keys(EditorState *state, SDL_Event *event) {
    SDL_Keycode key = event->key.key;
    SDL_Keymod mod = event->key.mod;
    
    bool ctrl = (mod & SDL_KMOD_CTRL) != 0;
    bool shift = (mod & SDL_KMOD_SHIFT) != 0;
    bool alt = (mod & SDL_KMOD_ALT) != 0;
    
    // Tab
    if (!ctrl && !shift && !alt && key == SDLK_TAB) {
        handle_tab(state);
    }
    // Ctrl+A or Enter in editor general mode
    else if (((ctrl && !shift && !alt && key == SDLK_A) ||
              (!ctrl && !shift && !alt && key == SDLK_RETURN)) &&
             state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        handle_ctrl_a(state, HISTORY_NONE);
    }
    // Ctrl+Shift+A in editor insert mode
    else if (ctrl && shift && !alt && key == SDLK_A &&
             state->current_coordinate == COORDINATE_LEFT_EDITOR_INSERT) {
        handle_escape(state);
        handle_ctrl_a(state, HISTORY_NONE);
        handle_a(state);
    }
    // Enter
    else if (!ctrl && !shift && !alt && key == SDLK_RETURN) {
        handle_enter(state, HISTORY_NONE);
    }
    // Ctrl+Enter
    else if (ctrl && !shift && !alt && key == SDLK_RETURN) {
        handle_ctrl_enter(state, HISTORY_NONE);
    }
    // Ctrl+I in editor general mode
    else if (ctrl && !shift && !alt && key == SDLK_I &&
             state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        handle_ctrl_i(state, HISTORY_NONE);
    }
    // Ctrl+Shift+I in editor insert mode
    else if (ctrl && shift && !alt && key == SDLK_I &&
             state->current_coordinate == COORDINATE_LEFT_EDITOR_INSERT) {
        handle_escape(state);
        handle_ctrl_i(state, HISTORY_NONE);
        handle_i(state);
    }
    // Ctrl+D (delete)
    else if (ctrl && !shift && !alt && key == SDLK_D &&
             state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        handle_delete(state, HISTORY_NONE);
    }
    // Colon (command mode)
    else if (!ctrl && !shift && !alt && key == SDLK_SEMICOLON &&
             (shift || event->key.key == SDLK_COLON) &&
             state->current_coordinate != COORDINATE_LEFT_VISITOR_INSERT &&
             state->current_coordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        handle_colon(state);
    }
    // K or Up arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_K && (state->current_coordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
                                 state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL)) ||
              (key == SDLK_UP &&
               state->current_coordinate != COORDINATE_LEFT_VISITOR_INSERT &&
               state->current_coordinate != COORDINATE_LEFT_EDITOR_INSERT))) {
        handle_up(state);
    }
    // J or Down arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_J && (state->current_coordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
                                 state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL)) ||
              (key == SDLK_DOWN &&
               state->current_coordinate != COORDINATE_LEFT_VISITOR_INSERT &&
               state->current_coordinate != COORDINATE_LEFT_EDITOR_INSERT))) {
        handle_down(state);
    }
    // H or Left arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_H && (state->current_coordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
                                 state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL)) ||
              key == SDLK_LEFT)) {
        handle_left(state);
    }
    // L or Right arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_L && (state->current_coordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
                                 state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL)) ||
              key == SDLK_RIGHT)) {
        handle_right(state);
    }
    // I (insert mode)
    else if (!ctrl && !shift && !alt && key == SDLK_I) {
        handle_i(state);
    }
    // A (append mode)
    else if (!ctrl && !shift && !alt && key == SDLK_A) {
        handle_a(state);
    }
    // Ctrl+Z (undo)
    else if (ctrl && !shift && !alt && key == SDLK_Z) {
        handle_history_action(state, HISTORY_UNDO);
    }
    // Ctrl+Shift+Z (redo)
    else if (ctrl && shift && !alt && key == SDLK_Z) {
        handle_history_action(state, HISTORY_REDO);
    }
    // Ctrl+X (cut)
    else if (ctrl && !shift && !alt && key == SDLK_X &&
             state->current_coordinate != COORDINATE_LEFT_EDITOR_INSERT &&
             state->current_coordinate != COORDINATE_LEFT_VISITOR_INSERT &&
             state->current_coordinate != COORDINATE_LEFT_VISITOR_GENERAL) {
        handle_ccp(state, TASK_CUT);
    }
    // Ctrl+C (copy)
    else if (ctrl && !shift && !alt && key == SDLK_C &&
             state->current_coordinate != COORDINATE_LEFT_EDITOR_INSERT &&
             state->current_coordinate != COORDINATE_LEFT_VISITOR_INSERT &&
             state->current_coordinate != COORDINATE_LEFT_VISITOR_GENERAL) {
        handle_ccp(state, TASK_COPY);
    }
    // Ctrl+V (paste)
    else if (ctrl && !shift && !alt && key == SDLK_V &&
             state->current_coordinate != COORDINATE_LEFT_EDITOR_INSERT &&
             state->current_coordinate != COORDINATE_LEFT_VISITOR_INSERT &&
             state->current_coordinate != COORDINATE_LEFT_VISITOR_GENERAL) {
        handle_ccp(state, TASK_PASTE);
    }
    // Ctrl+F (find)
    else if (ctrl && !shift && !alt && key == SDLK_F) {
        handle_find(state);
    }
    // Escape
    else if (!ctrl && !shift && !alt && key == SDLK_ESCAPE) {
        handle_escape(state);
    }
    // E (editor mode)
    else if (!ctrl && !shift && !alt && key == SDLK_E &&
             (state->current_coordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
              state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL)) {
        state->current_command = COMMAND_EDITOR_MODE;
        handle_command(state);
    }
    // V (visitor mode)
    else if (!ctrl && !shift && !alt && key == SDLK_V &&
             (state->current_coordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
              state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL)) {
        state->current_command = COMMAND_VISITOR_MODE;
        handle_command(state);
    }
    // Backspace in insert modes
    else if (!ctrl && !shift && !alt && key == SDLK_BACKSPACE &&
             (state->current_coordinate == COORDINATE_LEFT_EDITOR_INSERT ||
              state->current_coordinate == COORDINATE_LEFT_VISITOR_INSERT ||
              state->current_coordinate == COORDINATE_RIGHT_INFO ||
              state->current_coordinate == COORDINATE_RIGHT_COMMAND ||
              state->current_coordinate == COORDINATE_RIGHT_FIND)) {
        if (state->input_buffer_size > 0) {
            state->input_buffer[--state->input_buffer_size] = '\0';
            if (state->cursor_position > 0) state->cursor_position--;
            state->needs_redraw = true;
        }
    }
}

void handle_input(EditorState *state, const char *text) {
    if (!text) return;
    
    int len = strlen(text);
    if (state->input_buffer_size + len >= state->input_buffer_capacity) {
        // Resize buffer
        int new_capacity = state->input_buffer_capacity * 2;
        char *new_buffer = realloc(state->input_buffer, new_capacity);
        if (!new_buffer) return;
        
        state->input_buffer = new_buffer;
        state->input_buffer_capacity = new_capacity;
    }
    
    strcat(state->input_buffer, text);
    state->input_buffer_size += len;
    state->cursor_position += len;
    state->needs_redraw = true;
}

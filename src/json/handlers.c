#include "sfon_editor.h"
#include <string.h>
#include <SDL3/SDL.h>

void handle_tab(EditorState *state) {
    state->previous_coordinate = state->current_coordinate;
    state->current_coordinate = COORDINATE_RIGHT_INFO;
    
    create_list_right(state);
    state->needs_redraw = true;
}

void handle_ctrl_a(EditorState *state, History history) {
    uint64_t now = SDL_GetTicks();
    
    if (now - state->last_keypress_time <= DELTA_MS) {
        state->last_keypress_time = 0;
        handle_history_action(state, HISTORY_UNDO);
        update_state(state, TASK_APPEND_APPEND, HISTORY_NONE);
    } else {
        update_state(state, TASK_APPEND, history);
    }
    
    state->last_keypress_time = now;
    state->needs_redraw = true;
}

void handle_enter(EditorState *state, History history) {
    uint64_t now = SDL_GetTicks();
    
    if (state->current_coordinate == COORDINATE_RIGHT_INFO) {
        // Get selected item from list
        if (state->list_index >= 0 && state->list_index < state->filtered_list_count) {
            id_array_copy(&state->current_id, &state->filtered_list_right[state->list_index].id);
        }
        state->current_coordinate = state->previous_coordinate;
        state->needs_redraw = true;
    } else if (state->current_coordinate == COORDINATE_RIGHT_COMMAND) {
        // Execute selected command
        if (state->list_index >= 0 && state->list_index < state->filtered_list_count) {
            const char *cmd = state->filtered_list_right[state->list_index].value;
            if (strcmp(cmd, "editor mode") == 0) {
                state->current_command = COMMAND_EDITOR_MODE;
            } else if (strcmp(cmd, "visitor mode") == 0) {
                state->current_command = COMMAND_VISITOR_MODE;
            }
            handle_command(state);
        }
    }
    
    state->last_keypress_time = now;
}

void handle_ctrl_enter(EditorState *state, History history) {
    if (state->current_coordinate == COORDINATE_LEFT_EDITOR_INSERT) {
        update_state(state, TASK_INPUT, HISTORY_NONE);
        state->current_coordinate = COORDINATE_LEFT_EDITOR_GENERAL;
        handle_right(state);
        handle_a(state);
    }
}

void handle_ctrl_i(EditorState *state, History history) {
    uint64_t now = SDL_GetTicks();
    
    if (now - state->last_keypress_time <= DELTA_MS) {
        state->last_keypress_time = 0;
        handle_history_action(state, HISTORY_UNDO);
        update_state(state, TASK_INSERT_INSERT, HISTORY_NONE);
    } else {
        update_state(state, TASK_INSERT, history);
    }
    
    state->last_keypress_time = now;
    state->needs_redraw = true;
}

void handle_delete(EditorState *state, History history) {
    update_state(state, TASK_DELETE, history);
    state->needs_redraw = true;
}

void handle_colon(EditorState *state) {
    state->previous_coordinate = state->current_coordinate;
    state->current_coordinate = COORDINATE_RIGHT_COMMAND;
    
    create_list_right(state);
    state->needs_redraw = true;
}

void handle_up(EditorState *state) {
    if (state->current_coordinate == COORDINATE_RIGHT_INFO ||
        state->current_coordinate == COORDINATE_RIGHT_COMMAND ||
        state->current_coordinate == COORDINATE_RIGHT_FIND) {
        if (state->list_index > 0) {
            state->list_index--;
        }
    } else if (state->current_coordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        update_state(state, TASK_K_ARROW_UP, HISTORY_NONE);
    }
    state->needs_redraw = true;
}

void handle_down(EditorState *state) {
    if (state->current_coordinate == COORDINATE_RIGHT_INFO ||
        state->current_coordinate == COORDINATE_RIGHT_COMMAND ||
        state->current_coordinate == COORDINATE_RIGHT_FIND) {
        int max_index = (state->filtered_list_count > 0) ? 
                        state->filtered_list_count - 1 : 
                        state->total_list_count - 1;
        if (state->list_index < max_index) {
            state->list_index++;
        }
    } else if (state->current_coordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        update_state(state, TASK_J_ARROW_DOWN, HISTORY_NONE);
    }
    state->needs_redraw = true;
}

void handle_left(EditorState *state) {
    if (state->current_coordinate == COORDINATE_RIGHT_INFO ||
        state->current_coordinate == COORDINATE_RIGHT_COMMAND ||
        state->current_coordinate == COORDINATE_RIGHT_FIND) {
        // Nothing to do
    } else if (state->current_coordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        update_state(state, TASK_H_ARROW_LEFT, HISTORY_NONE);
        state->needs_redraw = true;
    }
}

void handle_right(EditorState *state) {
    if (state->current_coordinate == COORDINATE_RIGHT_INFO ||
        state->current_coordinate == COORDINATE_RIGHT_COMMAND ||
        state->current_coordinate == COORDINATE_RIGHT_FIND) {
        // Nothing to do
    } else if (state->current_coordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        update_state(state, TASK_L_ARROW_RIGHT, HISTORY_NONE);
        state->needs_redraw = true;
    }
}

void handle_i(EditorState *state) {
    if (state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        id_array_copy(&state->current_insert_id, &state->current_id);
        state->previous_coordinate = state->current_coordinate;
        state->current_coordinate = COORDINATE_LEFT_EDITOR_INSERT;
        
        // Get current line content
        int count;
        SfonElement **arr = get_sfon_at_id(state, &state->current_id, &count);
        if (arr && count > 0) {
            int idx = state->current_id.ids[state->current_id.depth - 1];
            if (idx >= 0 && idx < count) {
                SfonElement *elem = arr[idx];
                if (elem->type == SFON_STRING) {
                    strncpy(state->input_buffer, elem->data.string, 
                           state->input_buffer_capacity - 1);
                    state->input_buffer_size = strlen(state->input_buffer);
                } else {
                    strncpy(state->input_buffer, elem->data.object->key,
                           state->input_buffer_capacity - 1);
                    state->input_buffer_size = strlen(state->input_buffer);
                }
            }
        }
        
        state->cursor_position = 0;
        id_array_init(&state->current_insert_id);
        state->needs_redraw = true;
    }
}

void handle_a(EditorState *state) {
    if (state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        id_array_copy(&state->current_insert_id, &state->current_id);
        state->previous_coordinate = state->current_coordinate;
        state->current_coordinate = COORDINATE_LEFT_EDITOR_INSERT;
        
        // Get current line content
        int count;
        SfonElement **arr = get_sfon_at_id(state, &state->current_id, &count);
        if (arr && count > 0) {
            int idx = state->current_id.ids[state->current_id.depth - 1];
            if (idx >= 0 && idx < count) {
                SfonElement *elem = arr[idx];
                if (elem->type == SFON_STRING) {
                    strncpy(state->input_buffer, elem->data.string,
                           state->input_buffer_capacity - 1);
                    state->input_buffer_size = strlen(state->input_buffer);
                } else {
                    strncpy(state->input_buffer, elem->data.object->key,
                           state->input_buffer_capacity - 1);
                    state->input_buffer_size = strlen(state->input_buffer);
                }
            }
        }
        
        state->cursor_position = state->input_buffer_size;
        id_array_init(&state->current_insert_id);
        state->needs_redraw = true;
    }
}

void handle_find(EditorState *state) {
    if (state->current_coordinate != COORDINATE_RIGHT_INFO &&
        state->current_coordinate != COORDINATE_RIGHT_COMMAND) {
        state->previous_coordinate = state->current_coordinate;
        state->current_coordinate = COORDINATE_RIGHT_FIND;
        state->needs_redraw = true;
    }
}

void handle_escape(EditorState *state) {
    if (state->previous_coordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
        state->previous_coordinate == COORDINATE_LEFT_VISITOR_INSERT) {
        if (state->current_coordinate == COORDINATE_LEFT_VISITOR_INSERT) {
            update_state(state, TASK_INPUT, HISTORY_NONE);
        }
        state->current_coordinate = COORDINATE_LEFT_VISITOR_GENERAL;
    } else {
        if (state->current_coordinate == COORDINATE_LEFT_EDITOR_INSERT) {
            update_state(state, TASK_INPUT, HISTORY_NONE);
        }
        state->current_coordinate = COORDINATE_LEFT_EDITOR_GENERAL;
    }
    
    state->previous_coordinate = state->current_coordinate;
    state->needs_redraw = true;
}

void handle_command(EditorState *state) {
    switch (state->current_command) {
        case COMMAND_EDITOR_MODE:
            state->previous_coordinate = state->current_coordinate;
            state->current_coordinate = COORDINATE_LEFT_EDITOR_GENERAL;
            break;
            
        case COMMAND_VISITOR_MODE:
            state->previous_coordinate = state->current_coordinate;
            state->current_coordinate = COORDINATE_LEFT_VISITOR_GENERAL;
            break;
    }
    
    state->needs_redraw = true;
}

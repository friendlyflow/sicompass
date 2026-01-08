#include "sfon_editor.h"
#include <string.h>
#include <stdlib.h>

void update_state(EditorState *state, Task task, History history) {
    // Get current line
    char line[MAX_LINE_LENGTH] = "";
    
    if (history == HISTORY_UNDO || history == HISTORY_REDO) {
        if (state->undo_position < state->undo_history_count) {
            strncpy(line, state->undo_history[state->undo_history_count - state->undo_position].line,
                   MAX_LINE_LENGTH - 1);
        }
    } else {
        // Get line from current element or input buffer
        if (state->current_coordinate == COORDINATE_LEFT_EDITOR_INSERT ||
            state->current_coordinate == COORDINATE_LEFT_VISITOR_INSERT) {
            strncpy(line, state->input_buffer, MAX_LINE_LENGTH - 1);
        } else {
            int count;
            SfonElement **arr = get_sfon_at_id(state, &state->current_id, &count);
            if (arr && count > 0) {
                int idx = state->current_id.ids[state->current_id.depth - 1];
                if (idx >= 0 && idx < count) {
                    SfonElement *elem = arr[idx];
                    if (elem->type == SFON_STRING) {
                        strncpy(line, elem->data.string, MAX_LINE_LENGTH - 1);
                    } else {
                        strncpy(line, elem->data.object->key, MAX_LINE_LENGTH - 1);
                    }
                }
            }
        }
    }
    
    bool is_key = is_line_key(line);
    update_ids(state, is_key, task, history);
    update_sfon(state, line, is_key, task, history);
    update_history(state, task, is_key, line, history);
}

void update_ids(EditorState *state, bool is_key, Task task, History history) {
    id_array_copy(&state->previous_id, &state->current_id);
    
    if (history == HISTORY_UNDO || history == HISTORY_REDO) {
        return;
    }
    
    int max_id = get_max_id_in_current(state);
    int current_idx = state->current_id.ids[state->current_id.depth - 1];
    
    switch (task) {
        case TASK_K_ARROW_UP:
            if (current_idx > 0) {
                state->current_id.ids[state->current_id.depth - 1]--;
            }
            break;
            
        case TASK_J_ARROW_DOWN:
            if (current_idx < max_id) {
                state->current_id.ids[state->current_id.depth - 1]++;
            }
            break;
            
        case TASK_H_ARROW_LEFT:
            if (state->current_id.depth > 1) {
                id_array_pop(&state->current_id);
            }
            break;
            
        case TASK_L_ARROW_RIGHT:
            if (next_layer_exists(state)) {
                id_array_push(&state->current_id, 0);
            }
            break;
            
        case TASK_APPEND:
            if (state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL ||
                state->current_coordinate == COORDINATE_LEFT_VISITOR_GENERAL) {
                if (!is_key) {
                    state->current_id.ids[state->current_id.depth - 1]++;
                } else {
                    if (next_layer_exists(state)) {
                        state->current_id.ids[state->current_id.depth - 1]++;
                    } else {
                        id_array_push(&state->current_id, 0);
                    }
                }
            }
            break;
            
        case TASK_APPEND_APPEND:
            if (state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL ||
                state->current_coordinate == COORDINATE_LEFT_VISITOR_GENERAL) {
                state->current_id.ids[state->current_id.depth - 1] = max_id + 1;
            }
            break;
            
        case TASK_INSERT:
            // Position stays the same
            break;
            
        case TASK_INSERT_INSERT:
            if (state->current_coordinate == COORDINATE_LEFT_EDITOR_GENERAL ||
                state->current_coordinate == COORDINATE_LEFT_VISITOR_GENERAL) {
                state->current_id.ids[state->current_id.depth - 1] = 0;
            }
            break;
            
        case TASK_DELETE:
            // Position handled in update_sfon
            break;
            
        case TASK_INPUT:
            // Position stays the same
            break;
            
        default:
            break;
    }
}

void update_sfon(EditorState *state, const char *line, bool is_key, Task task, History history) {
    if (state->current_id.depth == 0) return;
    
    // Navigate to parent array
    int count;
    SfonElement **arr = get_sfon_at_id(state, &state->current_id, &count);
    if (!arr) return;
    
    int idx = state->current_id.ids[state->current_id.depth - 1];
    
    // Get parent object if we're nested
    SfonObject *parent_obj = NULL;
    if (state->current_id.depth > 1) {
        SfonElement **parent_arr = get_sfon_at_id(state, &state->current_id, &count);
        if (parent_arr) {
            int parent_idx = state->current_id.ids[state->current_id.depth - 2];
            if (parent_idx >= 0 && parent_idx < count) {
                SfonElement *parent_elem = parent_arr[parent_idx];
                if (parent_elem && parent_elem->type == SFON_OBJECT) {
                    parent_obj = parent_elem->data.object;
                }
            }
        }
    }
    
    switch (task) {
        case TASK_APPEND:
        case TASK_APPEND_APPEND:
        case TASK_INSERT:
        case TASK_INSERT_INSERT: {
            if (is_key) {
                // Convert to object or update key
                if (idx >= 0 && idx < count && arr[idx]->type == SFON_OBJECT) {
                    // Update key
                    free(arr[idx]->data.object->key);
                    arr[idx]->data.object->key = strdup(line);
                } else {
                    // Convert string to object
                    SfonElement *new_elem = sfon_element_create_object(line);
                    if (new_elem) {
                        sfon_object_add_element(new_elem->data.object, 
                                               sfon_element_create_string(""));
                        
                        if (parent_obj) {
                            // Insert in parent object
                            if (history != HISTORY_REDO) {
                                sfon_object_add_element(parent_obj, sfon_element_create_string(""));
                            }
                            if (idx >= 0 && idx < parent_obj->count) {
                                sfon_element_destroy(parent_obj->elements[idx]);
                                parent_obj->elements[idx] = new_elem;
                            }
                        }
                    }
                }
            } else {
                // Update or insert string element
                if (parent_obj) {
                    if (idx >= 0 && idx < parent_obj->count) {
                        sfon_element_destroy(parent_obj->elements[idx]);
                        parent_obj->elements[idx] = sfon_element_create_string(line);
                    }
                    if (history != HISTORY_REDO) {
                        sfon_object_add_element(parent_obj, sfon_element_create_string(""));
                    }
                }
            }
            break;
        }
        
        case TASK_DELETE: {
            if (parent_obj && idx >= 0 && idx < parent_obj->count) {
                // Remove element
                sfon_element_destroy(parent_obj->elements[idx]);
                
                // Shift elements down
                for (int i = idx; i < parent_obj->count - 1; i++) {
                    parent_obj->elements[i] = parent_obj->elements[i + 1];
                }
                parent_obj->count--;
                
                // Adjust current_id
                if (state->current_id.ids[state->current_id.depth - 1] > 0) {
                    state->current_id.ids[state->current_id.depth - 1]--;
                }
                
                // If empty, add one empty element
                if (parent_obj->count == 0) {
                    sfon_object_add_element(parent_obj, sfon_element_create_string(""));
                }
            }
            break;
        }
        
        case TASK_INPUT:
        case TASK_H_ARROW_LEFT:
        case TASK_L_ARROW_RIGHT:
        case TASK_K_ARROW_UP:
        case TASK_J_ARROW_DOWN: {
            // Update current element with line content
            if (idx >= 0 && idx < count) {
                if (arr[idx]->type == SFON_STRING) {
                    free(arr[idx]->data.string);
                    arr[idx]->data.string = strdup(line);
                } else if (arr[idx]->type == SFON_OBJECT) {
                    free(arr[idx]->data.object->key);
                    arr[idx]->data.object->key = strdup(line);
                }
            }
            break;
        }
        
        default:
            break;
    }
}

void update_history(EditorState *state, Task task, bool is_key, const char *line, History history) {
    if (history != HISTORY_NONE) return;
    
    if (task == TASK_APPEND || task == TASK_APPEND_APPEND ||
        task == TASK_INSERT || task == TASK_INSERT_INSERT ||
        task == TASK_DELETE || task == TASK_INPUT) {
        
        if (state->undo_history_count >= UNDO_HISTORY_SIZE) {
            // Remove oldest entry
            free(state->undo_history[0].line);
            memmove(&state->undo_history[0], &state->undo_history[1],
                   sizeof(UndoEntry) * (UNDO_HISTORY_SIZE - 1));
            state->undo_history_count--;
        }
        
        UndoEntry *entry = &state->undo_history[state->undo_history_count++];
        id_array_copy(&entry->id, &state->current_id);
        entry->task = task;
        entry->is_key = is_key;
        entry->line = strdup(line ? line : "");
        
        state->undo_position = 0;
    }
}

void handle_history_action(EditorState *state, History history) {
    if (state->undo_history_count == 0) {
        set_error_message(state, "No undo history");
        return;
    }
    
    if (history == HISTORY_UNDO) {
        // Save current state before undo
        int count;
        SfonElement **arr = get_sfon_at_id(state, &state->current_id, &count);
        if (arr && count > 0) {
            int idx = state->current_id.ids[state->current_id.depth - 1];
            if (idx >= 0 && idx < count) {
                SfonElement *elem = arr[idx];
                char line[MAX_LINE_LENGTH] = "";
                if (elem->type == SFON_STRING) {
                    strncpy(line, elem->data.string, MAX_LINE_LENGTH - 1);
                } else {
                    strncpy(line, elem->data.object->key, MAX_LINE_LENGTH - 1);
                }
                
                bool is_key = is_line_key(line);
                update_ids(state, is_key, TASK_NONE, HISTORY_NONE);
                update_sfon(state, line, is_key, TASK_NONE, HISTORY_NONE);
            }
        }
        
        if (state->undo_position < state->undo_history_count) {
            state->undo_position++;
        }
        
        UndoEntry *entry = &state->undo_history[state->undo_history_count - state->undo_position];
        id_array_copy(&state->current_id, &entry->id);
        
        // Reverse the operation
        switch (entry->task) {
            case TASK_APPEND:
            case TASK_APPEND_APPEND:
            case TASK_INSERT:
            case TASK_INSERT_INSERT:
                handle_delete(state, history);
                break;
                
            case TASK_DELETE:
                if (state->current_id.ids[state->current_id.depth - 1] == 0) {
                    handle_ctrl_i(state, history);
                } else {
                    handle_ctrl_a(state, history);
                }
                break;
                
            default:
                break;
        }
    } else if (history == HISTORY_REDO) {
        if (state->undo_position > 0) {
            UndoEntry *entry = &state->undo_history[state->undo_history_count - state->undo_position];
            id_array_copy(&state->current_id, &entry->id);
            
            // Redo the operation
            switch (entry->task) {
                case TASK_APPEND:
                case TASK_APPEND_APPEND:
                    handle_ctrl_a(state, history);
                    break;
                    
                case TASK_INSERT:
                case TASK_INSERT_INSERT:
                    handle_ctrl_i(state, history);
                    break;
                    
                case TASK_DELETE:
                    handle_delete(state, history);
                    break;
                    
                default:
                    break;
            }
            
            state->undo_position--;
        }
    }
    
    state->needs_redraw = true;
}

void handle_ccp(EditorState *state, Task task) {
    int count;
    SfonElement **arr = get_sfon_at_id(state, &state->current_id, &count);
    if (!arr || count == 0) return;
    
    int idx = state->current_id.ids[state->current_id.depth - 1];
    if (idx < 0 || idx >= count) return;
    
    if (task == TASK_PASTE) {
        if (state->clipboard) {
            // Insert clipboard content
            SfonElement *new_elem = sfon_element_clone(state->clipboard);
            if (new_elem) {
                // Add to parent
                SfonObject *parent_obj = NULL;
                if (state->current_id.depth > 1) {
                    int parent_count;
                    SfonElement **parent_arr = get_sfon_at_id(state, &state->current_id, &parent_count);
                    if (parent_arr) {
                        int parent_idx = state->current_id.ids[state->current_id.depth - 2];
                        if (parent_idx >= 0 && parent_idx < parent_count &&
                            parent_arr[parent_idx]->type == SFON_OBJECT) {
                            parent_obj = parent_arr[parent_idx]->data.object;
                        }
                    }
                }
                
                if (parent_obj) {
                    sfon_object_add_element(parent_obj, new_elem);
                    update_history(state, TASK_PASTE, false, "", HISTORY_NONE);
                }
            }
        }
    } else {
        // Copy or cut
        SfonElement *elem = arr[idx];
        
        if (state->clipboard) {
            sfon_element_destroy(state->clipboard);
        }
        
        if (elem->type == SFON_OBJECT && !next_layer_exists(state)) {
            // Copy the object's contents
            state->clipboard = sfon_element_clone(elem);
        } else {
            state->clipboard = sfon_element_clone(elem);
        }
        
        if (task == TASK_CUT) {
            handle_delete(state, HISTORY_NONE);
            update_history(state, TASK_CUT, false, "", HISTORY_NONE);
        }
    }
    
    state->needs_redraw = true;
}

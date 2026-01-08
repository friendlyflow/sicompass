#include "sfon_editor.h"
#include <stdlib.h>
#include <string.h>

void clear_list_right(EditorState *state) {
    if (state->total_list_right) {
        for (int i = 0; i < state->total_list_count; i++) {
            free(state->total_list_right[i].value);
        }
        free(state->total_list_right);
        state->total_list_right = NULL;
        state->total_list_count = 0;
    }
    
    if (state->filtered_list_right) {
        // Don't free values, they're shared with total_list_right
        free(state->filtered_list_right);
        state->filtered_list_right = NULL;
        state->filtered_list_count = 0;
    }
    
    state->list_index = 0;
}

void create_list_right(EditorState *state) {
    clear_list_right(state);
    
    if (state->current_coordinate == COORDINATE_RIGHT_INFO) {
        // List all elements in current layer
        int count;
        SfonElement **arr = get_sfon_at_id(state, &state->current_id, &count);
        if (!arr) return;
        
        state->total_list_right = calloc(count, sizeof(ListItem));
        if (!state->total_list_right) return;
        
        IdArray this_id;
        id_array_copy(&this_id, &state->current_id);
        this_id.ids[this_id.depth - 1] = 0;
        
        for (int i = 0; i < count; i++) {
            SfonElement *elem = arr[i];
            
            id_array_copy(&state->total_list_right[state->total_list_count].id, &this_id);
            
            if (elem->type == SFON_STRING) {
                state->total_list_right[state->total_list_count].value = 
                    strdup(elem->data.string);
            } else {
                state->total_list_right[state->total_list_count].value = 
                    strdup(elem->data.object->key);
            }
            
            state->total_list_count++;
            this_id.ids[this_id.depth - 1]++;
        }
        
    } else if (state->current_coordinate == COORDINATE_RIGHT_COMMAND) {
        // List available commands
        const char *commands[] = {
            "editor mode",
            "visitor mode"
        };
        int num_commands = sizeof(commands) / sizeof(commands[0]);
        
        state->total_list_right = calloc(num_commands, sizeof(ListItem));
        if (!state->total_list_right) return;
        
        for (int i = 0; i < num_commands; i++) {
            state->total_list_right[i].id.depth = 1;
            state->total_list_right[i].id.ids[0] = i;
            state->total_list_right[i].value = strdup(commands[i]);
            state->total_list_count++;
        }
    }
}

void populate_list_right(EditorState *state, const char *search_string) {
    if (!search_string || strlen(search_string) == 0) {
        // No filter, use all items
        if (state->filtered_list_right) {
            free(state->filtered_list_right);
        }
        state->filtered_list_right = NULL;
        state->filtered_list_count = 0;
        state->list_index = 0;
        return;
    }
    
    // Simple substring search
    if (state->filtered_list_right) {
        free(state->filtered_list_right);
    }
    
    state->filtered_list_right = calloc(state->total_list_count, sizeof(ListItem));
    if (!state->filtered_list_right) return;
    
    state->filtered_list_count = 0;
    
    for (int i = 0; i < state->total_list_count; i++) {
        if (strstr(state->total_list_right[i].value, search_string) != NULL) {
            id_array_copy(&state->filtered_list_right[state->filtered_list_count].id,
                         &state->total_list_right[i].id);
            state->filtered_list_right[state->filtered_list_count].value = 
                state->total_list_right[i].value; // Share pointer
            state->filtered_list_count++;
        }
    }
    
    // Reset list index
    if (state->list_index >= state->filtered_list_count) {
        state->list_index = state->filtered_list_count > 0 ? state->filtered_list_count - 1 : 0;
    }
}

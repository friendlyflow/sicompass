#include "sfon_editor.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

SfonElement* parse_json_value(json_object *jobj) {
    if (!jobj) return sfon_element_create_string("");
    
    enum json_type type = json_object_get_type(jobj);
    
    switch (type) {
        case json_type_string: {
            const char *str = json_object_get_string(jobj);
            return sfon_element_create_string(str);
        }
        
        case json_type_int:
        case json_type_double: {
            const char *str = json_object_to_json_string(jobj);
            return sfon_element_create_string(str);
        }
        
        case json_type_boolean: {
            bool val = json_object_get_boolean(jobj);
            return sfon_element_create_string(val ? "true" : "false");
        }
        
        case json_type_null: {
            return sfon_element_create_string("null");
        }
        
        case json_type_array: {
            int array_len = json_object_array_length(jobj);
            
            // Create a temporary object to hold array items
            SfonElement *elem = sfon_element_create_object("array");
            if (!elem) return NULL;
            
            for (int i = 0; i < array_len; i++) {
                json_object *item = json_object_array_get_idx(jobj, i);
                SfonElement *child = parse_json_value(item);
                if (child) {
                    sfon_object_add_element(elem->data.object, child);
                }
            }
            
            return elem;
        }
        
        case json_type_object: {
            // Get the first (and should be only) key-value pair
            json_object_iterator it = json_object_iter_begin(jobj);
            json_object_iterator it_end = json_object_iter_end(jobj);
            
            if (json_object_iter_equal(&it, &it_end)) {
                // Empty object
                return sfon_element_create_string("");
            }
            
            const char *key = json_object_iter_peek_name(&it);
            json_object *val = json_object_iter_peek_value(&it);
            
            // Create object element with the key
            SfonElement *elem = sfon_element_create_object(key);
            if (!elem) return NULL;
            
            // Parse the value (should be an array)
            if (json_object_is_type(val, json_type_array)) {
                int array_len = json_object_array_length(val);
                for (int i = 0; i < array_len; i++) {
                    json_object *item = json_object_array_get_idx(val, i);
                    SfonElement *child = parse_json_value(item);
                    if (child) {
                        sfon_object_add_element(elem->data.object, child);
                    }
                }
            }
            
            return elem;
        }
        
        default:
            return sfon_element_create_string("");
    }
}

bool load_json_file(EditorState *state, const char *filename) {
    // Read file
    FILE *fp = fopen(filename, "r");
    if (!fp) {
        fprintf(stderr, "Failed to open file: %s\n", filename);
        return false;
    }
    
    fseek(fp, 0, SEEK_END);
    long file_size = ftell(fp);
    fseek(fp, 0, SEEK_SET);
    
    char *json_data = malloc(file_size + 1);
    if (!json_data) {
        fclose(fp);
        return false;
    }
    
    size_t read_size = fread(json_data, 1, file_size, fp);
    json_data[read_size] = '\0';
    fclose(fp);
    
    // Parse JSON
    json_object *root = json_tokener_parse(json_data);
    free(json_data);
    
    if (!root) {
        fprintf(stderr, "Failed to parse JSON\n");
        return false;
    }
    
    // Check if root is an array
    if (!json_object_is_type(root, json_type_array)) {
        fprintf(stderr, "Root must be an array\n");
        json_object_put(root);
        return false;
    }
    
    // Clear existing SFON data
    for (int i = 0; i < state->sfon_count; i++) {
        sfon_element_destroy(state->sfon[i]);
    }
    state->sfon_count = 0;
    
    // Parse array elements
    int array_len = json_object_array_length(root);
    for (int i = 0; i < array_len; i++) {
        json_object *item = json_object_array_get_idx(root, i);
        SfonElement *elem = parse_json_value(item);
        
        if (elem) {
            // Resize if needed
            if (state->sfon_count >= state->sfon_capacity) {
                int new_capacity = state->sfon_capacity * 2;
                SfonElement **new_sfon = realloc(state->sfon, 
                                                 new_capacity * sizeof(SfonElement*));
                if (!new_sfon) {
                    sfon_element_destroy(elem);
                    json_object_put(root);
                    return false;
                }
                state->sfon = new_sfon;
                state->sfon_capacity = new_capacity;
            }
            
            state->sfon[state->sfon_count++] = elem;
        }
    }
    
    json_object_put(root);
    
    // If empty, add one empty element
    if (state->sfon_count == 0) {
        state->sfon[0] = sfon_element_create_string("");
        state->sfon_count = 1;
    }
    
    return true;
}

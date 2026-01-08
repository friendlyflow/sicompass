#include "sfon_editor.h"
#include <stdlib.h>
#include <string.h>

SfonElement* sfon_element_create_string(const char *str) {
    SfonElement *elem = malloc(sizeof(SfonElement));
    if (!elem) return NULL;
    
    elem->type = SFON_STRING;
    elem->data.string = strdup(str ? str : "");
    if (!elem->data.string) {
        free(elem);
        return NULL;
    }
    
    return elem;
}

SfonElement* sfon_element_create_object(const char *key) {
    SfonElement *elem = malloc(sizeof(SfonElement));
    if (!elem) return NULL;
    
    elem->type = SFON_OBJECT;
    elem->data.object = sfon_object_create(key);
    if (!elem->data.object) {
        free(elem);
        return NULL;
    }
    
    return elem;
}

void sfon_element_destroy(SfonElement *elem) {
    if (!elem) return;
    
    if (elem->type == SFON_STRING) {
        free(elem->data.string);
    } else if (elem->type == SFON_OBJECT) {
        sfon_object_destroy(elem->data.object);
    }
    
    free(elem);
}

SfonElement* sfon_element_clone(SfonElement *elem) {
    if (!elem) return NULL;
    
    if (elem->type == SFON_STRING) {
        return sfon_element_create_string(elem->data.string);
    } else {
        SfonElement *new_elem = sfon_element_create_object(elem->data.object->key);
        if (!new_elem) return NULL;
        
        // Clone all child elements
        for (int i = 0; i < elem->data.object->count; i++) {
            SfonElement *child = sfon_element_clone(elem->data.object->elements[i]);
            if (child) {
                sfon_object_add_element(new_elem->data.object, child);
            }
        }
        
        return new_elem;
    }
}

SfonObject* sfon_object_create(const char *key) {
    SfonObject *obj = malloc(sizeof(SfonObject));
    if (!obj) return NULL;
    
    obj->key = strdup(key ? key : "");
    if (!obj->key) {
        free(obj);
        return NULL;
    }
    
    obj->capacity = 10;
    obj->count = 0;
    obj->elements = calloc(obj->capacity, sizeof(SfonElement*));
    if (!obj->elements) {
        free(obj->key);
        free(obj);
        return NULL;
    }
    
    return obj;
}

void sfon_object_destroy(SfonObject *obj) {
    if (!obj) return;
    
    free(obj->key);
    
    for (int i = 0; i < obj->count; i++) {
        sfon_element_destroy(obj->elements[i]);
    }
    free(obj->elements);
    
    free(obj);
}

void sfon_object_add_element(SfonObject *obj, SfonElement *elem) {
    if (!obj || !elem) return;
    
    // Resize if needed
    if (obj->count >= obj->capacity) {
        int new_capacity = obj->capacity * 2;
        SfonElement **new_elements = realloc(obj->elements, 
                                             new_capacity * sizeof(SfonElement*));
        if (!new_elements) return;
        
        obj->elements = new_elements;
        obj->capacity = new_capacity;
    }
    
    obj->elements[obj->count++] = elem;
}

// Get SFON element(s) at a given ID path
SfonElement** get_sfon_at_id(EditorState *state, const IdArray *id, int *out_count) {
    if (id->depth == 0) {
        *out_count = state->sfon_count;
        return state->sfon;
    }
    
    SfonElement **current = state->sfon;
    int current_count = state->sfon_count;
    
    for (int i = 0; i < id->depth - 1; i++) {
        int idx = id->ids[i];
        if (idx < 0 || idx >= current_count) {
            *out_count = 0;
            return NULL;
        }
        
        SfonElement *elem = current[idx];
        if (elem->type != SFON_OBJECT) {
            *out_count = 0;
            return NULL;
        }
        
        current = elem->data.object->elements;
        current_count = elem->data.object->count;
    }
    
    *out_count = current_count;
    return current;
}

bool next_layer_exists(EditorState *state) {
    if (state->previous_id.depth == 0) return false;
    
    int count;
    SfonElement **arr = get_sfon_at_id(state, &state->previous_id, &count);
    if (!arr || count == 0) return false;
    
    int last_idx = state->previous_id.ids[state->previous_id.depth - 1];
    if (last_idx < 0 || last_idx >= count) return false;
    
    return arr[last_idx]->type == SFON_OBJECT;
}

int get_max_id_in_current(EditorState *state) {
    int count;
    SfonElement **arr = get_sfon_at_id(state, &state->current_id, &count);
    if (!arr || count == 0) return 0;
    
    return count - 1;
}

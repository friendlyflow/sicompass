#include "view.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

SfonElement* parseJsonValue(json_object *jobj) {
    if (!jobj) return sfonElementCreateString("");

    enum json_type type = json_object_get_type(jobj);

    switch (type) {
        case json_type_string: {
            const char *str = json_object_get_string(jobj);
            return sfonElementCreateString(str);
        }

        case json_type_int:
        case json_type_double: {
            const char *str = json_object_to_json_string(jobj);
            return sfonElementCreateString(str);
        }

        case json_type_boolean: {
            bool val = json_object_get_boolean(jobj);
            return sfonElementCreateString(val ? "true" : "false");
        }

        case json_type_null: {
            return sfonElementCreateString("null");
        }

        case json_type_array: {
            int arrayLen = json_object_array_length(jobj);

            // Create a temporary object to hold array items
            SfonElement *elem = sfonElementCreateObject("array");
            if (!elem) return NULL;

            for (int i = 0; i < arrayLen; i++) {
                json_object *item = json_object_array_get_idx(jobj, i);
                SfonElement *child = parseJsonValue(item);
                if (child) {
                    sfonObjectAddElement(elem->data.object, child);
                }
            }

            return elem;
        }

        case json_type_object: {
            // Get the first (and should be only) key-value pair
            json_object_iterator it = json_object_iter_begin(jobj);
            json_object_iterator itEnd = json_object_iter_end(jobj);

            if (json_object_iter_equal(&it, &itEnd)) {
                // Empty object
                return sfonElementCreateString("");
            }

            const char *key = json_object_iter_peek_name(&it);
            json_object *val = json_object_iter_peek_value(&it);

            // Create object element with the key
            SfonElement *elem = sfonElementCreateObject(key);
            if (!elem) return NULL;

            // Parse the value (should be an array)
            if (json_object_is_type(val, json_type_array)) {
                int arrayLen = json_object_array_length(val);
                for (int i = 0; i < arrayLen; i++) {
                    json_object *item = json_object_array_get_idx(val, i);
                    SfonElement *child = parseJsonValue(item);
                    if (child) {
                        sfonObjectAddElement(elem->data.object, child);
                    }
                }
            }

            return elem;
        }

        default:
            return sfonElementCreateString("");
    }
}

bool loadJsonFile(EditorState *state, const char *filename) {
    // Read file
    FILE *fp = fopen(filename, "r");
    if (!fp) {
        fprintf(stderr, "Failed to open file: %s\n", filename);
        return false;
    }

    fseek(fp, 0, SEEK_END);
    long fileSize = ftell(fp);
    fseek(fp, 0, SEEK_SET);

    char *jsonData = malloc(fileSize + 1);
    if (!jsonData) {
        fclose(fp);
        return false;
    }

    size_t readSize = fread(jsonData, 1, fileSize, fp);
    jsonData[readSize] = '\0';
    fclose(fp);

    // Parse JSON
    json_object *root = json_tokener_parse(jsonData);
    free(jsonData);

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
    for (int i = 0; i < state->sfonCount; i++) {
        sfonElementDestroy(state->sfon[i]);
    }
    state->sfonCount = 0;

    // Parse array elements
    int arrayLen = json_object_array_length(root);
    for (int i = 0; i < arrayLen; i++) {
        json_object *item = json_object_array_get_idx(root, i);
        SfonElement *elem = parseJsonValue(item);

        if (elem) {
            // Resize if needed
            if (state->sfonCount >= state->sfonCapacity) {
                int newCapacity = state->sfonCapacity * 2;
                SfonElement **newSfon = realloc(state->sfon,
                                                 newCapacity * sizeof(SfonElement*));
                if (!newSfon) {
                    sfonElementDestroy(elem);
                    json_object_put(root);
                    return false;
                }
                state->sfon = newSfon;
                state->sfonCapacity = newCapacity;
            }

            state->sfon[state->sfonCount++] = elem;
        }
    }

    json_object_put(root);

    // If empty, add one empty element
    if (state->sfonCount == 0) {
        state->sfon[0] = sfonElementCreateString("");
        state->sfonCount = 1;
    }

    return true;
}

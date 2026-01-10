#include "view.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

FfonElement* parseJsonValue(json_object *jobj) {
    if (!jobj) return ffonElementCreateString("");

    enum json_type type = json_object_get_type(jobj);

    switch (type) {
        case json_type_string: {
            const char *str = json_object_get_string(jobj);
            return ffonElementCreateString(str);
        }

        case json_type_int:
        case json_type_double: {
            const char *str = json_object_to_json_string(jobj);
            return ffonElementCreateString(str);
        }

        case json_type_boolean: {
            bool val = json_object_get_boolean(jobj);
            return ffonElementCreateString(val ? "true" : "false");
        }

        case json_type_null: {
            return ffonElementCreateString("null");
        }

        case json_type_array: {
            int arrayLen = json_object_array_length(jobj);

            // Create a temporary object to hold array items
            FfonElement *elem = ffonElementCreateObject("array");
            if (!elem) return NULL;

            for (int i = 0; i < arrayLen; i++) {
                json_object *item = json_object_array_get_idx(jobj, i);
                FfonElement *child = parseJsonValue(item);
                if (child) {
                    ffonObjectAddElement(elem->data.object, child);
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
                return ffonElementCreateString("");
            }

            const char *key = json_object_iter_peek_name(&it);
            json_object *val = json_object_iter_peek_value(&it);

            // Create object element with the key
            FfonElement *elem = ffonElementCreateObject(key);
            if (!elem) return NULL;

            // Parse the value (should be an array)
            if (json_object_is_type(val, json_type_array)) {
                int arrayLen = json_object_array_length(val);
                for (int i = 0; i < arrayLen; i++) {
                    json_object *item = json_object_array_get_idx(val, i);
                    FfonElement *child = parseJsonValue(item);
                    if (child) {
                        ffonObjectAddElement(elem->data.object, child);
                    }
                }
            }

            return elem;
        }

        default:
            return ffonElementCreateString("");
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

    // Clear existing FFON data
    for (int i = 0; i < state->ffonCount; i++) {
        ffonElementDestroy(state->ffon[i]);
    }
    state->ffonCount = 0;

    // Parse array elements
    int arrayLen = json_object_array_length(root);
    for (int i = 0; i < arrayLen; i++) {
        json_object *item = json_object_array_get_idx(root, i);
        FfonElement *elem = parseJsonValue(item);

        if (elem) {
            // Resize if needed
            if (state->ffonCount >= state->ffonCapacity) {
                int newCapacity = state->ffonCapacity * 2;
                FfonElement **newFfon = realloc(state->ffon,
                                                 newCapacity * sizeof(FfonElement*));
                if (!newFfon) {
                    ffonElementDestroy(elem);
                    json_object_put(root);
                    return false;
                }
                state->ffon = newFfon;
                state->ffonCapacity = newCapacity;
            }

            state->ffon[state->ffonCount++] = elem;
        }
    }

    json_object_put(root);

    // If empty, add one empty element
    if (state->ffonCount == 0) {
        state->ffon[0] = ffonElementCreateString("");
        state->ffonCount = 1;
    }

    return true;
}

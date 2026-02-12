#include "ffon.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// ============================================
// ID array operations
// ============================================

void idArrayInit(IdArray *arr) {
    arr->depth = 0;
    memset(arr->ids, 0, sizeof(arr->ids));
}

void idArrayCopy(IdArray *dst, const IdArray *src) {
    dst->depth = src->depth;
    memcpy(dst->ids, src->ids, sizeof(int) * src->depth);
}

bool idArrayEqual(const IdArray *a, const IdArray *b) {
    if (a->depth != b->depth) return false;
    return memcmp(a->ids, b->ids, sizeof(int) * a->depth) == 0;
}

void idArrayPush(IdArray *arr, int val) {
    if (arr->depth < MAX_ID_DEPTH) {
        arr->ids[arr->depth++] = val;
    }
}

int idArrayPop(IdArray *arr) {
    if (arr->depth > 0) {
        return arr->ids[--arr->depth];
    }
    return -1;
}

char* idArrayToString(const IdArray *arr) {
    static char buffer[MAX_ID_DEPTH * 16];
    buffer[0] = '\0';

    for (int i = 0; i < arr->depth; i++) {
        if (i > 0) strcat(buffer, ",");
        char num[16];
        snprintf(num, sizeof(num), "%d", arr->ids[i]);
        strcat(buffer, num);
    }

    return buffer;
}

// ============================================
// Core FFON element operations
// ============================================

FfonElement* ffonElementCreateString(const char *str) {
    FfonElement *elem = malloc(sizeof(FfonElement));
    if (!elem) return NULL;

    elem->type = FFON_STRING;
    elem->data.string = strdup(str ? str : "");
    if (!elem->data.string) {
        free(elem);
        return NULL;
    }

    return elem;
}

FfonElement* ffonElementCreateObject(const char *key) {
    FfonElement *elem = malloc(sizeof(FfonElement));
    if (!elem) return NULL;

    elem->type = FFON_OBJECT;
    elem->data.object = ffonObjectCreate(key);
    if (!elem->data.object) {
        free(elem);
        return NULL;
    }

    return elem;
}

void ffonElementDestroy(FfonElement *elem) {
    if (!elem) return;

    if (elem->type == FFON_STRING) {
        free(elem->data.string);
    } else if (elem->type == FFON_OBJECT) {
        ffonObjectDestroy(elem->data.object);
    }

    free(elem);
}

FfonElement* ffonElementClone(FfonElement *elem) {
    if (!elem) return NULL;

    if (elem->type == FFON_STRING) {
        return ffonElementCreateString(elem->data.string);
    } else {
        FfonElement *newElem = ffonElementCreateObject(elem->data.object->key);
        if (!newElem) return NULL;

        // Clone all child elements
        for (int i = 0; i < elem->data.object->count; i++) {
            FfonElement *child = ffonElementClone(elem->data.object->elements[i]);
            if (child) {
                ffonObjectAddElement(newElem->data.object, child);
            }
        }

        return newElem;
    }
}

FfonObject* ffonObjectCreate(const char *key) {
    FfonObject *obj = malloc(sizeof(FfonObject));
    if (!obj) return NULL;

    obj->key = strdup(key ? key : "");
    if (!obj->key) {
        free(obj);
        return NULL;
    }

    obj->capacity = 10;
    obj->count = 0;
    obj->elements = calloc(obj->capacity, sizeof(FfonElement*));
    if (!obj->elements) {
        free(obj->key);
        free(obj);
        return NULL;
    }

    return obj;
}

void ffonObjectDestroy(FfonObject *obj) {
    if (!obj) return;

    free(obj->key);

    for (int i = 0; i < obj->count; i++) {
        ffonElementDestroy(obj->elements[i]);
    }
    free(obj->elements);

    free(obj);
}

void ffonObjectAddElement(FfonObject *obj, FfonElement *elem) {
    if (!obj || !elem) return;

    // Resize if needed
    if (obj->count >= obj->capacity) {
        int newCapacity = obj->capacity * 2;
        FfonElement **newElements = realloc(obj->elements,
                                             newCapacity * sizeof(FfonElement*));
        if (!newElements) return;

        obj->elements = newElements;
        obj->capacity = newCapacity;
    }

    obj->elements[obj->count++] = elem;
}

void ffonObjectInsertElement(FfonObject *obj, FfonElement *elem, int index) {
    if (!obj || !elem) return;

    // Resize if needed
    if (obj->count >= obj->capacity) {
        int newCapacity = obj->capacity * 2;
        FfonElement **newElements = realloc(obj->elements,
                                             newCapacity * sizeof(FfonElement*));
        if (!newElements) return;

        obj->elements = newElements;
        obj->capacity = newCapacity;
    }

    // Clamp index to valid range [0, count]
    if (index < 0) index = 0;
    if (index > obj->count) index = obj->count;

    // Shift elements to make room
    for (int i = obj->count; i > index; i--) {
        obj->elements[i] = obj->elements[i - 1];
    }

    obj->elements[index] = elem;
    obj->count++;
}

FfonElement* ffonObjectRemoveElement(FfonObject *obj, int index) {
    if (!obj || index < 0 || index >= obj->count) return NULL;

    FfonElement *elem = obj->elements[index];

    for (int i = index; i < obj->count - 1; i++) {
        obj->elements[i] = obj->elements[i + 1];
    }
    obj->count--;

    return elem;
}

// ============================================
// JSON parsing
// ============================================

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
            // Iterate over object entries to get the first key-value pair
            const char *first_key = NULL;
            json_object *first_val = NULL;

            json_object_object_foreach(jobj, key, val) {
                first_key = key;
                first_val = val;
                break; // Only get the first entry
            }

            if (!first_key) {
                // Empty object
                return ffonElementCreateString("");
            }

            // Create object element with the key
            FfonElement *elem = ffonElementCreateObject(first_key);
            if (!elem) return NULL;

            // Parse the value (should be an array)
            if (json_object_is_type(first_val, json_type_array)) {
                int arrayLen = json_object_array_length(first_val);
                for (int i = 0; i < arrayLen; i++) {
                    json_object *item = json_object_array_get_idx(first_val, i);
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

FfonElement** loadJsonFileToElements(const char *filename, int *out_count) {
    *out_count = 0;

    // Read file
    FILE *fp = fopen(filename, "r");
    if (!fp) {
        fprintf(stderr, "Failed to open file: %s\n", filename);
        return NULL;
    }

    fseek(fp, 0, SEEK_END);
    long fileSize = ftell(fp);
    fseek(fp, 0, SEEK_SET);

    char *jsonData = malloc(fileSize + 1);
    if (!jsonData) {
        fclose(fp);
        return NULL;
    }

    size_t readSize = fread(jsonData, 1, fileSize, fp);
    jsonData[readSize] = '\0';
    fclose(fp);

    // Parse JSON
    json_object *root = json_tokener_parse(jsonData);
    free(jsonData);

    if (!root) {
        fprintf(stderr, "Failed to parse JSON\n");
        return NULL;
    }

    // Check if root is an array
    if (!json_object_is_type(root, json_type_array)) {
        fprintf(stderr, "Root must be an array\n");
        json_object_put(root);
        return NULL;
    }

    // Parse array elements
    int arrayLen = json_object_array_length(root);
    int capacity = arrayLen > 0 ? arrayLen : 16;
    int count = 0;
    FfonElement **elements = malloc(sizeof(FfonElement*) * capacity);

    for (int i = 0; i < arrayLen; i++) {
        json_object *item = json_object_array_get_idx(root, i);
        FfonElement *elem = parseJsonValue(item);

        if (elem) {
            if (count >= capacity) {
                capacity *= 2;
                FfonElement **newElements = realloc(elements, sizeof(FfonElement*) * capacity);
                if (!newElements) {
                    ffonElementDestroy(elem);
                    json_object_put(root);
                    for (int j = 0; j < count; j++) ffonElementDestroy(elements[j]);
                    free(elements);
                    return NULL;
                }
                elements = newElements;
            }
            elements[count++] = elem;
        }
    }

    json_object_put(root);

    *out_count = count;
    return elements;
}

// ============================================
// Binary serialization
// ============================================

// Helper: write element recursively to buffer
static void writeElementBinary(FfonElement *elem, uint32_t layer,
                               uint8_t **buf, size_t *size, size_t *capacity) {
    const char *content;
    uint32_t content_len;
    bool isObject = (elem->type == FFON_OBJECT);

    if (isObject) {
        // For objects, content is "key:"
        const char *key = elem->data.object->key;
        content_len = strlen(key) + 1;  // +1 for ':'
    } else {
        content = elem->data.string;
        content_len = strlen(content);
    }

    // Ensure capacity (8 bytes header + content + optional ':')
    size_t needed = *size + 8 + content_len;
    while (needed > *capacity) {
        *capacity *= 2;
        *buf = realloc(*buf, *capacity);
    }

    // Write layer (4 bytes, little-endian)
    memcpy(*buf + *size, &layer, 4);
    *size += 4;

    // Write content_length (4 bytes, little-endian)
    memcpy(*buf + *size, &content_len, 4);
    *size += 4;

    // Write content
    if (isObject) {
        const char *key = elem->data.object->key;
        size_t keyLen = strlen(key);
        memcpy(*buf + *size, key, keyLen);
        *size += keyLen;
        (*buf)[(*size)++] = ':';

        // Recursively write children
        for (int i = 0; i < elem->data.object->count; i++) {
            writeElementBinary(elem->data.object->elements[i], layer + 1,
                              buf, size, capacity);
        }
    } else {
        memcpy(*buf + *size, content, content_len);
        *size += content_len;
    }
}

uint8_t* ffonSerializeBinary(FfonElement **ffon, int count, size_t *out_size) {
    size_t capacity = 1024;
    size_t size = 0;
    uint8_t *buf = malloc(capacity);

    for (int i = 0; i < count; i++) {
        writeElementBinary(ffon[i], 0, &buf, &size, &capacity);
    }

    *out_size = size;
    return buf;
}

// Helper: check if content ends with ':'
static bool isKeyContent(const char *content, uint32_t len) {
    return len > 0 && content[len - 1] == ':';
}

FfonElement** ffonDeserializeBinary(uint8_t *data, size_t size, int *out_count) {
    // First pass: count entries and find max layer
    size_t pos = 0;
    int entryCount = 0;

    while (pos + 8 <= size) {
        uint32_t layer, content_len;
        memcpy(&layer, data + pos, 4);
        memcpy(&content_len, data + pos + 4, 4);
        pos += 8 + content_len;
        entryCount++;
    }

    if (entryCount == 0) {
        *out_count = 0;
        return NULL;
    }

    // Allocate temporary storage for entries
    typedef struct {
        uint32_t layer;
        char *content;
        bool isKey;
    } Entry;

    Entry *entries = malloc(sizeof(Entry) * entryCount);
    pos = 0;
    for (int i = 0; i < entryCount; i++) {
        uint32_t layer, content_len;
        memcpy(&layer, data + pos, 4);
        memcpy(&content_len, data + pos + 4, 4);
        pos += 8;

        entries[i].layer = layer;
        entries[i].isKey = isKeyContent((char*)(data + pos), content_len);

        if (entries[i].isKey) {
            // Strip trailing ':'
            entries[i].content = malloc(content_len);
            memcpy(entries[i].content, data + pos, content_len - 1);
            entries[i].content[content_len - 1] = '\0';
        } else {
            entries[i].content = malloc(content_len + 1);
            memcpy(entries[i].content, data + pos, content_len);
            entries[i].content[content_len] = '\0';
        }
        pos += content_len;
    }

    // Build tree using a stack
    FfonElement **stack = malloc(sizeof(FfonElement*) * 64);  // Max depth 64
    int stackDepth = 0;

    // Result array
    int resultCap = 16;
    int resultCount = 0;
    FfonElement **result = malloc(sizeof(FfonElement*) * resultCap);

    for (int i = 0; i < entryCount; i++) {
        Entry *e = &entries[i];

        // Pop stack to match layer
        while (stackDepth > (int)e->layer) {
            stackDepth--;
        }

        FfonElement *elem;
        if (e->isKey) {
            elem = ffonElementCreateObject(e->content);
        } else {
            elem = ffonElementCreateString(e->content);
        }

        if (stackDepth == 0) {
            // Root level
            if (resultCount >= resultCap) {
                resultCap *= 2;
                result = realloc(result, sizeof(FfonElement*) * resultCap);
            }
            result[resultCount++] = elem;
        } else {
            // Add to parent object
            FfonElement *parent = stack[stackDepth - 1];
            if (parent->type == FFON_OBJECT) {
                ffonObjectAddElement(parent->data.object, elem);
            }
        }

        // If this is an object, push to stack for children
        if (e->isKey) {
            stack[stackDepth++] = elem;
        }

        free(e->content);
    }

    free(entries);
    free(stack);

    *out_count = resultCount;
    return result;
}

bool saveFfonFile(FfonElement **ffon, int count, const char *filename) {
    size_t size;
    uint8_t *data = ffonSerializeBinary(ffon, count, &size);
    if (!data) return false;

    FILE *fp = fopen(filename, "wb");
    if (!fp) {
        free(data);
        return false;
    }

    size_t written = fwrite(data, 1, size, fp);
    fclose(fp);
    free(data);

    return written == size;
}

FfonElement** loadFfonFileToElements(const char *filename, int *out_count) {
    *out_count = 0;

    FILE *fp = fopen(filename, "rb");
    if (!fp) return NULL;

    fseek(fp, 0, SEEK_END);
    long fileSize = ftell(fp);
    fseek(fp, 0, SEEK_SET);

    uint8_t *data = malloc(fileSize);
    if (!data) {
        fclose(fp);
        return NULL;
    }

    size_t readSize = fread(data, 1, fileSize, fp);
    fclose(fp);

    if (readSize != (size_t)fileSize) {
        free(data);
        return NULL;
    }

    int count;
    FfonElement **elements = ffonDeserializeBinary(data, fileSize, &count);
    free(data);

    *out_count = count;
    return elements;
}

// ============================================
// Navigation functions
// ============================================

FfonElement** getFfonAtId(FfonElement **ffon, int ffonCount, const IdArray *id, int *outCount) {
    if (id->depth == 0) {
        *outCount = ffonCount;
        return ffon;
    }

    FfonElement **current = ffon;
    int currentCount = ffonCount;

    for (int i = 0; i < id->depth - 1; i++) {
        int idx = id->ids[i];
        if (idx < 0 || idx >= currentCount) {
            *outCount = 0;
            return NULL;
        }

        FfonElement *elem = current[idx];
        if (elem->type != FFON_OBJECT) {
            *outCount = 0;
            return NULL;
        }

        current = elem->data.object->elements;
        currentCount = elem->data.object->count;
    }

    *outCount = currentCount;
    return current;
}

bool nextFfonLayerExists(FfonElement **ffon, int ffonCount, const IdArray *id) {
    if (id->depth == 0) return false;

    int count;
    FfonElement **arr = getFfonAtId(ffon, ffonCount, id, &count);
    if (!arr || count == 0) return false;

    int lastIdx = id->ids[id->depth - 1];
    if (lastIdx < 0 || lastIdx >= count) return false;

    return arr[lastIdx]->type == FFON_OBJECT;
}

int getFfonMaxIdAtPath(FfonElement **ffon, int ffonCount, const IdArray *id) {
    int count;
    FfonElement **arr = getFfonAtId(ffon, ffonCount, id, &count);
    if (!arr || count == 0) return 0;

    return count - 1;
}

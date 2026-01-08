#include "view.h"
#include <stdlib.h>
#include <string.h>

SfonElement* sfonElementCreateString(const char *str) {
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

SfonElement* sfonElementCreateObject(const char *key) {
    SfonElement *elem = malloc(sizeof(SfonElement));
    if (!elem) return NULL;

    elem->type = SFON_OBJECT;
    elem->data.object = sfonObjectCreate(key);
    if (!elem->data.object) {
        free(elem);
        return NULL;
    }

    return elem;
}

void sfonElementDestroy(SfonElement *elem) {
    if (!elem) return;

    if (elem->type == SFON_STRING) {
        free(elem->data.string);
    } else if (elem->type == SFON_OBJECT) {
        sfonObjectDestroy(elem->data.object);
    }

    free(elem);
}

SfonElement* sfonElementClone(SfonElement *elem) {
    if (!elem) return NULL;

    if (elem->type == SFON_STRING) {
        return sfonElementCreateString(elem->data.string);
    } else {
        SfonElement *newElem = sfonElementCreateObject(elem->data.object->key);
        if (!newElem) return NULL;

        // Clone all child elements
        for (int i = 0; i < elem->data.object->count; i++) {
            SfonElement *child = sfonElementClone(elem->data.object->elements[i]);
            if (child) {
                sfonObjectAddElement(newElem->data.object, child);
            }
        }

        return newElem;
    }
}

SfonObject* sfonObjectCreate(const char *key) {
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

void sfonObjectDestroy(SfonObject *obj) {
    if (!obj) return;

    free(obj->key);

    for (int i = 0; i < obj->count; i++) {
        sfonElementDestroy(obj->elements[i]);
    }
    free(obj->elements);

    free(obj);
}

void sfonObjectAddElement(SfonObject *obj, SfonElement *elem) {
    if (!obj || !elem) return;

    // Resize if needed
    if (obj->count >= obj->capacity) {
        int newCapacity = obj->capacity * 2;
        SfonElement **newElements = realloc(obj->elements,
                                             newCapacity * sizeof(SfonElement*));
        if (!newElements) return;

        obj->elements = newElements;
        obj->capacity = newCapacity;
    }

    obj->elements[obj->count++] = elem;
}

// Get SFON element(s) at a given ID path
SfonElement** getSfonAtId(EditorState *state, const IdArray *id, int *outCount) {
    if (id->depth == 0) {
        *outCount = state->sfonCount;
        return state->sfon;
    }

    SfonElement **current = state->sfon;
    int currentCount = state->sfonCount;

    for (int i = 0; i < id->depth - 1; i++) {
        int idx = id->ids[i];
        if (idx < 0 || idx >= currentCount) {
            *outCount = 0;
            return NULL;
        }

        SfonElement *elem = current[idx];
        if (elem->type != SFON_OBJECT) {
            *outCount = 0;
            return NULL;
        }

        current = elem->data.object->elements;
        currentCount = elem->data.object->count;
    }

    *outCount = currentCount;
    return current;
}

bool nextLayerExists(EditorState *state) {
    if (state->previousId.depth == 0) return false;

    int count;
    SfonElement **arr = getSfonAtId(state, &state->previousId, &count);
    if (!arr || count == 0) return false;

    int lastIdx = state->previousId.ids[state->previousId.depth - 1];
    if (lastIdx < 0 || lastIdx >= count) return false;

    return arr[lastIdx]->type == SFON_OBJECT;
}

int getMaxIdInCurrent(EditorState *state) {
    int count;
    SfonElement **arr = getSfonAtId(state, &state->currentId, &count);
    if (!arr || count == 0) return 0;

    return count - 1;
}

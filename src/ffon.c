#include "view.h"
#include <stdlib.h>
#include <string.h>

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

// Get FFON element(s) at a given ID path
FfonElement** getFfonAtId(AppRenderer *appRenderer, const IdArray *id, int *outCount) {
    if (id->depth == 0) {
        *outCount = appRenderer->ffonCount;
        return appRenderer->ffon;
    }

    FfonElement **current = appRenderer->ffon;
    int currentCount = appRenderer->ffonCount;

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

bool nextLayerExists(AppRenderer *appRenderer) {
    if (appRenderer->previousId.depth == 0) return false;

    int count;
    FfonElement **arr = getFfonAtId(appRenderer, &appRenderer->previousId, &count);
    if (!arr || count == 0) return false;

    int lastIdx = appRenderer->previousId.ids[appRenderer->previousId.depth - 1];
    if (lastIdx < 0 || lastIdx >= count) return false;

    return arr[lastIdx]->type == FFON_OBJECT;
}

int getMaxIdInCurrent(AppRenderer *appRenderer) {
    int count;
    FfonElement **arr = getFfonAtId(appRenderer, &appRenderer->currentId, &count);
    if (!arr || count == 0) return 0;

    return count - 1;
}

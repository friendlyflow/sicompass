#pragma once

#include <stdint.h>
#include <stdbool.h>
#include <json-c/json.h>

// Constants
#define MAX_ID_DEPTH 32

// Forward declarations
typedef struct FfonElement FfonElement;
typedef struct FfonObject FfonObject;

// ID array structure for navigation
typedef struct {
    int ids[MAX_ID_DEPTH];
    int depth;
} IdArray;

// .ffon file extension structure (binary format entry)
typedef struct Ffon {
    uint32_t    layer;
    uint32_t    content_length;
    char*       content;
} Ffon;

// FFON data structures
struct FfonElement {
    enum { FFON_STRING, FFON_OBJECT } type;
    union {
        char *string;
        FfonObject *object;
    } data;
};

struct FfonObject {
    char *key;
    FfonElement **elements;
    int count;
    int capacity;
};

// FFON operations
FfonElement* ffonElementCreateString(const char *str);
FfonElement* ffonElementCreateObject(const char *key);
void ffonElementDestroy(FfonElement *elem);
FfonElement* ffonElementClone(FfonElement *elem);
FfonObject* ffonObjectCreate(const char *key);
void ffonObjectDestroy(FfonObject *obj);
void ffonObjectAddElement(FfonObject *obj, FfonElement *elem);
void ffonObjectInsertElement(FfonObject *obj, FfonElement *elem, int index);

// JSON parsing
FfonElement* parseJsonValue(json_object *jobj);
FfonElement** loadJsonFileToElements(const char *filename, int *out_count);

// Binary serialization (.ffon files)
uint8_t* ffonSerializeBinary(FfonElement **ffon, int count, size_t *out_size);
FfonElement** ffonDeserializeBinary(uint8_t *data, size_t size, int *out_count);
bool saveFfonFile(FfonElement **ffon, int count, const char *filename);
FfonElement** loadFfonFileToElements(const char *filename, int *out_count);

// ID array operations
void idArrayInit(IdArray *arr);
void idArrayCopy(IdArray *dst, const IdArray *src);
bool idArrayEqual(const IdArray *a, const IdArray *b);
void idArrayPush(IdArray *arr, int val);
int idArrayPop(IdArray *arr);
char* idArrayToString(const IdArray *arr);

// Navigation functions
FfonElement** getFfonAtId(FfonElement **ffon, int ffonCount, const IdArray *id, int *outCount);
bool nextFfonLayerExists(FfonElement **ffon, int ffonCount, const IdArray *id);
int getFfonMaxIdAtPath(FfonElement **ffon, int ffonCount, const IdArray *id);

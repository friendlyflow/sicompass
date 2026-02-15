#include "unicode_search.h"
#include <utf8proc.h>
#include <stdlib.h>
#include <string.h>

// Unicode-aware case-insensitive substring search
// Returns pointer to haystack if needle is found (case-insensitive), or NULL
const char* utf8_stristr(const char* haystack, const char* needle) {
    if (!haystack || !needle) return NULL;
    if (*needle == '\0') return haystack;

    utf8proc_uint8_t *folded_haystack = NULL;
    utf8proc_uint8_t *folded_needle = NULL;

    // Case-fold and normalize both strings for comparison
    utf8proc_map((const utf8proc_uint8_t*)haystack, 0, &folded_haystack,
                 UTF8PROC_NULLTERM | UTF8PROC_STABLE |
                 UTF8PROC_COMPOSE | UTF8PROC_CASEFOLD);

    utf8proc_map((const utf8proc_uint8_t*)needle, 0, &folded_needle,
                 UTF8PROC_NULLTERM | UTF8PROC_STABLE |
                 UTF8PROC_COMPOSE | UTF8PROC_CASEFOLD);

    if (!folded_haystack || !folded_needle) {
        free(folded_haystack);
        free(folded_needle);
        return NULL;
    }

    const char* result = strstr((const char*)folded_haystack,
                                (const char*)folded_needle) ? haystack : NULL;

    free(folded_haystack);
    free(folded_needle);

    return result;
}

// Build a byte-offset mapping from folded string positions to original string positions.
// Returns an allocated array where map[folded_byte_pos] = original_byte_pos.
// The array has (foldedLen + 1) entries. Caller must free the result.
static int* buildOffsetMap(const char *original, const char *folded, int foldedLen) {
    int *map = malloc((foldedLen + 1) * sizeof(int));
    if (!map) return NULL;

    const utf8proc_uint8_t *op = (const utf8proc_uint8_t *)original;
    const utf8proc_uint8_t *fp = (const utf8proc_uint8_t *)folded;
    int origPos = 0;
    int foldPos = 0;

    while (foldPos < foldedLen && *op && *fp) {
        // Record mapping at current fold position
        map[foldPos] = origPos;

        // Read one code point from original
        utf8proc_int32_t origCp, foldCp;
        utf8proc_ssize_t origBytes = utf8proc_iterate(op, -1, &origCp);
        utf8proc_ssize_t foldBytes = utf8proc_iterate(fp, -1, &foldCp);

        if (origBytes < 1) origBytes = 1;
        if (foldBytes < 1) foldBytes = 1;

        // Fill intermediate fold bytes with same original position
        for (int i = 1; i < foldBytes && foldPos + i < foldedLen; i++) {
            map[foldPos + i] = origPos;
        }

        origPos += (int)origBytes;
        foldPos += (int)foldBytes;
        op += origBytes;
        fp += foldBytes;
    }

    // Fill remaining entries
    for (int i = foldPos; i <= foldedLen; i++) {
        map[i] = origPos;
    }

    return map;
}

const char* utf8_stristr_pos(const char* haystack, const char* needle) {
    if (!haystack || !needle) return NULL;
    if (*needle == '\0') return haystack;

    utf8proc_uint8_t *folded_haystack = NULL;
    utf8proc_uint8_t *folded_needle = NULL;

    utf8proc_map((const utf8proc_uint8_t*)haystack, 0, &folded_haystack,
                 UTF8PROC_NULLTERM | UTF8PROC_STABLE |
                 UTF8PROC_COMPOSE | UTF8PROC_CASEFOLD);

    utf8proc_map((const utf8proc_uint8_t*)needle, 0, &folded_needle,
                 UTF8PROC_NULLTERM | UTF8PROC_STABLE |
                 UTF8PROC_COMPOSE | UTF8PROC_CASEFOLD);

    if (!folded_haystack || !folded_needle) {
        free(folded_haystack);
        free(folded_needle);
        return NULL;
    }

    const char *found = strstr((const char*)folded_haystack, (const char*)folded_needle);
    if (!found) {
        free(folded_haystack);
        free(folded_needle);
        return NULL;
    }

    int foldedLen = (int)strlen((const char*)folded_haystack);
    int *map = buildOffsetMap(haystack, (const char*)folded_haystack, foldedLen);
    if (!map) {
        free(folded_haystack);
        free(folded_needle);
        return NULL;
    }

    int foldedOffset = (int)(found - (const char*)folded_haystack);
    int originalOffset = map[foldedOffset];

    free(map);
    free(folded_haystack);
    free(folded_needle);

    return haystack + originalOffset;
}

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

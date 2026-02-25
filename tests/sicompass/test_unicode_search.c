/*
 * Tests for unicode_search.c functions:
 * - utf8_stristr (case-insensitive substring search)
 * - utf8_stristr_pos (case-insensitive search with exact position)
 *
 * Links against libutf8proc for real Unicode case folding.
 */

#include <unity.h>
#include <utf8proc.h>
#include <stdlib.h>
#include <string.h>

/* ============================================
 * Functions under test (from unicode_search.c)
 * ============================================ */

const char* utf8_stristr(const char* haystack, const char* needle) {
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

    const char* result = strstr((const char*)folded_haystack,
                                (const char*)folded_needle) ? haystack : NULL;

    free(folded_haystack);
    free(folded_needle);

    return result;
}

static int* buildOffsetMap(const char *original, const char *folded, int foldedLen) {
    int *map = malloc((foldedLen + 1) * sizeof(int));
    if (!map) return NULL;

    const utf8proc_uint8_t *op = (const utf8proc_uint8_t *)original;
    const utf8proc_uint8_t *fp = (const utf8proc_uint8_t *)folded;
    int origPos = 0;
    int foldPos = 0;

    while (foldPos < foldedLen && *op && *fp) {
        map[foldPos] = origPos;

        utf8proc_int32_t origCp, foldCp;
        utf8proc_ssize_t origBytes = utf8proc_iterate(op, -1, &origCp);
        utf8proc_ssize_t foldBytes = utf8proc_iterate(fp, -1, &foldCp);

        if (origBytes < 1) origBytes = 1;
        if (foldBytes < 1) foldBytes = 1;

        for (int i = 1; i < foldBytes && foldPos + i < foldedLen; i++) {
            map[foldPos + i] = origPos;
        }

        origPos += (int)origBytes;
        foldPos += (int)foldBytes;
        op += origBytes;
        fp += foldBytes;
    }

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

/* ============================================
 * Unity Setup/Teardown
 * ============================================ */

void setUp(void) {}
void tearDown(void) {}

/* ============================================
 * utf8_stristr tests
 * ============================================ */

void test_utf8_stristr_null_haystack(void) {
    TEST_ASSERT_NULL(utf8_stristr(NULL, "test"));
}

void test_utf8_stristr_null_needle(void) {
    TEST_ASSERT_NULL(utf8_stristr("test", NULL));
}

void test_utf8_stristr_empty_needle(void) {
    const char *result = utf8_stristr("hello", "");
    TEST_ASSERT_NOT_NULL(result);
    TEST_ASSERT_EQUAL_STRING("hello", result);
}

void test_utf8_stristr_exact_match(void) {
    TEST_ASSERT_NOT_NULL(utf8_stristr("hello", "hello"));
}

void test_utf8_stristr_case_insensitive(void) {
    TEST_ASSERT_NOT_NULL(utf8_stristr("Hello World", "hello"));
}

void test_utf8_stristr_case_insensitive_uppercase_needle(void) {
    TEST_ASSERT_NOT_NULL(utf8_stristr("hello world", "HELLO"));
}

void test_utf8_stristr_substring(void) {
    TEST_ASSERT_NOT_NULL(utf8_stristr("hello world", "world"));
}

void test_utf8_stristr_no_match(void) {
    TEST_ASSERT_NULL(utf8_stristr("hello", "xyz"));
}

void test_utf8_stristr_unicode_case_fold(void) {
    // "É" (U+00C9) should match "é" (U+00E9)
    TEST_ASSERT_NOT_NULL(utf8_stristr("café", "CAFÉ"));
}

void test_utf8_stristr_unicode_accent(void) {
    TEST_ASSERT_NOT_NULL(utf8_stristr("Résumé", "résumé"));
}

void test_utf8_stristr_returns_haystack_on_match(void) {
    const char *haystack = "Hello World";
    const char *result = utf8_stristr(haystack, "hello");
    // utf8_stristr returns haystack pointer (not position within it)
    TEST_ASSERT_EQUAL_PTR(haystack, result);
}

void test_utf8_stristr_partial_no_match(void) {
    TEST_ASSERT_NULL(utf8_stristr("hel", "hello"));
}

/* ============================================
 * utf8_stristr_pos tests
 * ============================================ */

void test_utf8_stristr_pos_null_haystack(void) {
    TEST_ASSERT_NULL(utf8_stristr_pos(NULL, "test"));
}

void test_utf8_stristr_pos_null_needle(void) {
    TEST_ASSERT_NULL(utf8_stristr_pos("test", NULL));
}

void test_utf8_stristr_pos_empty_needle(void) {
    const char *haystack = "hello";
    TEST_ASSERT_EQUAL_PTR(haystack, utf8_stristr_pos(haystack, ""));
}

void test_utf8_stristr_pos_at_start(void) {
    const char *haystack = "Hello World";
    const char *result = utf8_stristr_pos(haystack, "hello");
    TEST_ASSERT_NOT_NULL(result);
    TEST_ASSERT_EQUAL_PTR(haystack, result);
}

void test_utf8_stristr_pos_in_middle(void) {
    const char *haystack = "Hello World";
    const char *result = utf8_stristr_pos(haystack, "world");
    TEST_ASSERT_NOT_NULL(result);
    // "World" starts at byte offset 6
    TEST_ASSERT_EQUAL_PTR(haystack + 6, result);
}

void test_utf8_stristr_pos_no_match(void) {
    TEST_ASSERT_NULL(utf8_stristr_pos("hello", "xyz"));
}

void test_utf8_stristr_pos_unicode_at_start(void) {
    const char *haystack = "Café Latte";
    const char *result = utf8_stristr_pos(haystack, "café");
    TEST_ASSERT_NOT_NULL(result);
    TEST_ASSERT_EQUAL_PTR(haystack, result);
}

void test_utf8_stristr_pos_unicode_in_middle(void) {
    const char *haystack = "My Café";
    const char *result = utf8_stristr_pos(haystack, "café");
    TEST_ASSERT_NOT_NULL(result);
    // "Café" starts at byte 3 ("My " = 3 bytes)
    TEST_ASSERT_EQUAL_PTR(haystack + 3, result);
}

void test_utf8_stristr_pos_case_fold_unicode(void) {
    const char *haystack = "RÉSUMÉ";
    const char *result = utf8_stristr_pos(haystack, "résumé");
    TEST_ASSERT_NOT_NULL(result);
    TEST_ASSERT_EQUAL_PTR(haystack, result);
}

/* ============================================
 * Main
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // utf8_stristr
    RUN_TEST(test_utf8_stristr_null_haystack);
    RUN_TEST(test_utf8_stristr_null_needle);
    RUN_TEST(test_utf8_stristr_empty_needle);
    RUN_TEST(test_utf8_stristr_exact_match);
    RUN_TEST(test_utf8_stristr_case_insensitive);
    RUN_TEST(test_utf8_stristr_case_insensitive_uppercase_needle);
    RUN_TEST(test_utf8_stristr_substring);
    RUN_TEST(test_utf8_stristr_no_match);
    RUN_TEST(test_utf8_stristr_unicode_case_fold);
    RUN_TEST(test_utf8_stristr_unicode_accent);
    RUN_TEST(test_utf8_stristr_returns_haystack_on_match);
    RUN_TEST(test_utf8_stristr_partial_no_match);

    // utf8_stristr_pos
    RUN_TEST(test_utf8_stristr_pos_null_haystack);
    RUN_TEST(test_utf8_stristr_pos_null_needle);
    RUN_TEST(test_utf8_stristr_pos_empty_needle);
    RUN_TEST(test_utf8_stristr_pos_at_start);
    RUN_TEST(test_utf8_stristr_pos_in_middle);
    RUN_TEST(test_utf8_stristr_pos_no_match);
    RUN_TEST(test_utf8_stristr_pos_unicode_at_start);
    RUN_TEST(test_utf8_stristr_pos_unicode_in_middle);
    RUN_TEST(test_utf8_stristr_pos_case_fold_unicode);

    return UNITY_END();
}

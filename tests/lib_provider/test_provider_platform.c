/*
 * Tests for platform utility functions.
 * Functions under test: platformGetConfigHome, platformGetHomeDir,
 *                       platformGetCacheHome, platformGetPathSeparator,
 *                       platformIsWindows, platformFreeApplications
 */

#include <unity.h>
#include <platform.h>
#include <stdlib.h>
#include <string.h>

void setUp(void) {}
void tearDown(void) {}

// --- platformGetConfigHome ---

void test_getConfigHome_returns_non_null(void) {
    char *path = platformGetConfigHome();
    TEST_ASSERT_NOT_NULL(path);
    free(path);
}

void test_getConfigHome_ends_with_separator(void) {
    char *path = platformGetConfigHome();
    TEST_ASSERT_NOT_NULL(path);
    size_t len = strlen(path);
    TEST_ASSERT_TRUE(len > 0);
    TEST_ASSERT_EQUAL_CHAR('/', path[len - 1]);
    free(path);
}

// --- platformGetHomeDir ---

void test_getHomeDir_returns_non_null(void) {
    char *home = platformGetHomeDir();
    TEST_ASSERT_NOT_NULL(home);
    free(home);
}

void test_getHomeDir_starts_with_slash(void) {
    char *home = platformGetHomeDir();
    TEST_ASSERT_NOT_NULL(home);
    TEST_ASSERT_EQUAL_CHAR('/', home[0]);
    free(home);
}

// --- platformGetCacheHome ---

void test_getCacheHome_returns_non_null(void) {
    char *path = platformGetCacheHome();
    TEST_ASSERT_NOT_NULL(path);
    free(path);
}

void test_getCacheHome_ends_with_separator(void) {
    char *path = platformGetCacheHome();
    TEST_ASSERT_NOT_NULL(path);
    size_t len = strlen(path);
    TEST_ASSERT_TRUE(len > 0);
    TEST_ASSERT_EQUAL_CHAR('/', path[len - 1]);
    free(path);
}

// --- platformGetPathSeparator ---

void test_getPathSeparator_linux(void) {
    const char *sep = platformGetPathSeparator();
    TEST_ASSERT_EQUAL_STRING("/", sep);
}

// --- platformIsWindows ---

void test_isWindows_returns_false_on_linux(void) {
    TEST_ASSERT_FALSE(platformIsWindows());
}

// --- platformFreeApplications ---

void test_freeApplications_null(void) {
    platformFreeApplications(NULL, 0);  // should not crash
}

void test_freeApplications_empty(void) {
    PlatformApplication *apps = malloc(sizeof(PlatformApplication));
    apps[0].name = strdup("Test");
    apps[0].exec = strdup("test");
    platformFreeApplications(apps, 1);  // should free everything
}

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_getConfigHome_returns_non_null);
    RUN_TEST(test_getConfigHome_ends_with_separator);

    RUN_TEST(test_getHomeDir_returns_non_null);
    RUN_TEST(test_getHomeDir_starts_with_slash);

    RUN_TEST(test_getCacheHome_returns_non_null);
    RUN_TEST(test_getCacheHome_ends_with_separator);

    RUN_TEST(test_getPathSeparator_linux);
    RUN_TEST(test_isWindows_returns_false_on_linux);

    RUN_TEST(test_freeApplications_null);
    RUN_TEST(test_freeApplications_empty);

    return UNITY_END();
}

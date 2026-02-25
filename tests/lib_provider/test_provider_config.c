/*
 * Tests for provider config path functions.
 * Functions under test: providerGetConfigDir, providerGetConfigPath,
 *                       providerGetMainConfigPath
 */

#include <unity.h>
#include <provider_interface.h>
#include <stdlib.h>
#include <string.h>

void setUp(void) {}
void tearDown(void) {}

// --- providerGetConfigDir ---

void test_getConfigDir_returns_non_null(void) {
    char *dir = providerGetConfigDir();
    TEST_ASSERT_NOT_NULL(dir);
    free(dir);
}

void test_getConfigDir_contains_sicompass(void) {
    char *dir = providerGetConfigDir();
    TEST_ASSERT_NOT_NULL(dir);
    TEST_ASSERT_NOT_NULL(strstr(dir, "sicompass"));
    free(dir);
}

void test_getConfigDir_ends_with_separator(void) {
    char *dir = providerGetConfigDir();
    TEST_ASSERT_NOT_NULL(dir);
    size_t len = strlen(dir);
    TEST_ASSERT_TRUE(len > 0);
    TEST_ASSERT_EQUAL_CHAR('/', dir[len - 1]);
    free(dir);
}

// --- providerGetConfigPath ---

void test_getConfigPath_returns_json_path(void) {
    char *path = providerGetConfigPath("filebrowser");
    TEST_ASSERT_NOT_NULL(path);
    // Should end with "filebrowser.json"
    const char *suffix = "filebrowser.json";
    size_t pathLen = strlen(path);
    size_t suffixLen = strlen(suffix);
    TEST_ASSERT_TRUE(pathLen >= suffixLen);
    TEST_ASSERT_EQUAL_STRING(suffix, path + pathLen - suffixLen);
    free(path);
}

void test_getConfigPath_null_name(void) {
    char *path = providerGetConfigPath(NULL);
    TEST_ASSERT_NULL(path);
}

// --- providerGetMainConfigPath ---

void test_getMainConfigPath_returns_settings_json(void) {
    char *path = providerGetMainConfigPath();
    TEST_ASSERT_NOT_NULL(path);
    const char *suffix = "settings.json";
    size_t pathLen = strlen(path);
    size_t suffixLen = strlen(suffix);
    TEST_ASSERT_TRUE(pathLen >= suffixLen);
    TEST_ASSERT_EQUAL_STRING(suffix, path + pathLen - suffixLen);
    free(path);
}

void test_getMainConfigPath_contains_sicompass(void) {
    char *path = providerGetMainConfigPath();
    TEST_ASSERT_NOT_NULL(path);
    TEST_ASSERT_NOT_NULL(strstr(path, "sicompass"));
    free(path);
}

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_getConfigDir_returns_non_null);
    RUN_TEST(test_getConfigDir_contains_sicompass);
    RUN_TEST(test_getConfigDir_ends_with_separator);

    RUN_TEST(test_getConfigPath_returns_json_path);
    RUN_TEST(test_getConfigPath_null_name);

    RUN_TEST(test_getMainConfigPath_returns_settings_json);
    RUN_TEST(test_getMainConfigPath_contains_sicompass);

    return UNITY_END();
}

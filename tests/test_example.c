/*
 * Example test file demonstrating unity + fff usage
 *
 * unity: Testing framework (assertions, test structure)
 * fff: Fake Function Framework (mocking)
 */

#include <unity.h>
#include <fff/fff.h>
#include <stdlib.h>

DEFINE_FFF_GLOBALS;

/* ============================================
 * Example: Mocking an external dependency
 * ============================================
 * Suppose we have a function that depends on reading a file.
 * We can mock the file reading function to test our logic
 * without actually touching the filesystem.
 */

// Declare a fake for a hypothetical file_read function
// FAKE_VALUE_FUNC(return_type, function_name, arg_types...)
FAKE_VALUE_FUNC(int, file_read, const char*, char*, int);

// Function under test that uses file_read
int load_config_value(const char *path) {
    char buffer[256];
    int result = file_read(path, buffer, sizeof(buffer));
    if (result < 0) {
        return -1;
    }
    // Parse the buffer as an integer
    return atoi(buffer);
}

/* ============================================
 * Unity Test Setup/Teardown
 * ============================================ */

void setUp(void) {
    // Reset all fakes before each test
    RESET_FAKE(file_read);
    FFF_RESET_HISTORY();
}

void tearDown(void) {
    // Cleanup after each test (if needed)
}

/* ============================================
 * Test Cases
 * ============================================ */

void test_example_basic_assertion(void) {
    TEST_ASSERT_TRUE(1 + 1 == 2);
    TEST_ASSERT_EQUAL_INT(42, 42);
    TEST_ASSERT_NOT_EQUAL(1, 2);
}

void test_example_string_assertions(void) {
    const char *hello = "hello";
    TEST_ASSERT_EQUAL_STRING("hello", hello);
    TEST_ASSERT_TRUE(strcmp(hello, "world") != 0);
}

void test_fff_demo_mock_file_read_success(void) {
    // Configure the mock to return success and set buffer content
    file_read_fake.return_val = 5;
    file_read_fake.custom_fake = NULL;

    // Call the function under test
    // Note: In a real test, you'd set up the buffer content via custom_fake
    int result = load_config_value("/fake/path");

    // Verify the mock was called
    TEST_ASSERT_EQUAL_INT(1, file_read_fake.call_count);
    TEST_ASSERT_EQUAL_STRING("/fake/path", file_read_fake.arg0_val);
}

void test_fff_demo_mock_file_read_failure(void) {
    // Configure the mock to return failure
    file_read_fake.return_val = -1;

    int result = load_config_value("/nonexistent");

    TEST_ASSERT_EQUAL_INT(-1, result);
    TEST_ASSERT_EQUAL_INT(1, file_read_fake.call_count);
}

void test_fff_demo_call_history(void) {
    file_read_fake.return_val = 0;

    // Make multiple calls
    load_config_value("/path/a");
    load_config_value("/path/b");
    load_config_value("/path/c");

    // Check call count
    TEST_ASSERT_EQUAL_INT(3, file_read_fake.call_count);

    // Check argument history
    TEST_ASSERT_EQUAL_STRING("/path/a", file_read_fake.arg0_history[0]);
    TEST_ASSERT_EQUAL_STRING("/path/b", file_read_fake.arg0_history[1]);
    TEST_ASSERT_EQUAL_STRING("/path/c", file_read_fake.arg0_history[2]);
}

/* ============================================
 * Main - Run all tests
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_example_basic_assertion);
    RUN_TEST(test_example_string_assertions);
    RUN_TEST(test_fff_demo_mock_file_read_success);
    RUN_TEST(test_fff_demo_mock_file_read_failure);
    RUN_TEST(test_fff_demo_call_history);

    return UNITY_END();
}

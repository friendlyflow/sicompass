/*
 * Example test file demonstrating tau + fff usage
 *
 * tau: Testing framework (assertions, test structure)
 * fff: Fake Function Framework (mocking)
 */

#include <tau/tau.h>
#include <fff/fff.h>

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
 * tau Test Suite
 * ============================================ */

TAU_MAIN()

TEST(example, basic_assertion) {
    CHECK(1 + 1 == 2);
    CHECK_EQ(42, 42);
    CHECK_NE(1, 2);
}

TEST(example, string_assertions) {
    const char *hello = "hello";
    CHECK_STREQ(hello, "hello");
    CHECK_STRNE(hello, "world");
}

TEST(fff_demo, mock_file_read_success) {
    // Reset the fake before each test
    RESET_FAKE(file_read);

    // Configure the mock to return success and set buffer content
    file_read_fake.return_val = 5;
    file_read_fake.custom_fake = NULL;

    // Call the function under test
    // Note: In a real test, you'd set up the buffer content via custom_fake
    int result = load_config_value("/fake/path");

    // Verify the mock was called
    CHECK_EQ(file_read_fake.call_count, 1);
    CHECK_STREQ(file_read_fake.arg0_val, "/fake/path");
}

TEST(fff_demo, mock_file_read_failure) {
    RESET_FAKE(file_read);

    // Configure the mock to return failure
    file_read_fake.return_val = -1;

    int result = load_config_value("/nonexistent");

    CHECK_EQ(result, -1);
    CHECK_EQ(file_read_fake.call_count, 1);
}

TEST(fff_demo, call_history) {
    RESET_FAKE(file_read);

    file_read_fake.return_val = 0;

    // Make multiple calls
    load_config_value("/path/a");
    load_config_value("/path/b");
    load_config_value("/path/c");

    // Check call count
    CHECK_EQ(file_read_fake.call_count, 3);

    // Check argument history
    CHECK_STREQ(file_read_fake.arg0_history[0], "/path/a");
    CHECK_STREQ(file_read_fake.arg0_history[1], "/path/b");
    CHECK_STREQ(file_read_fake.arg0_history[2], "/path/c");
}

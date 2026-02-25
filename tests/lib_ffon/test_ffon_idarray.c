/*
 * Tests for IdArray operations.
 * Functions under test: idArrayInit, idArrayCopy, idArrayEqual,
 *                       idArrayPush, idArrayPop, idArrayToString
 */

#include <unity.h>
#include <ffon.h>
#include <string.h>

void setUp(void) {}
void tearDown(void) {}

// --- idArrayInit ---

void test_init_sets_depth_zero(void) {
    IdArray arr;
    arr.depth = 99;  // dirty state
    idArrayInit(&arr);
    TEST_ASSERT_EQUAL_INT(0, arr.depth);
}

void test_init_zeros_ids(void) {
    IdArray arr;
    memset(arr.ids, 0xFF, sizeof(arr.ids));  // dirty state
    idArrayInit(&arr);
    for (int i = 0; i < MAX_ID_DEPTH; i++) {
        TEST_ASSERT_EQUAL_INT(0, arr.ids[i]);
    }
}

// --- idArrayCopy ---

void test_copy_populated(void) {
    IdArray src, dst;
    idArrayInit(&src);
    idArrayInit(&dst);
    idArrayPush(&src, 1);
    idArrayPush(&src, 2);
    idArrayPush(&src, 3);

    idArrayCopy(&dst, &src);
    TEST_ASSERT_EQUAL_INT(3, dst.depth);
    TEST_ASSERT_EQUAL_INT(1, dst.ids[0]);
    TEST_ASSERT_EQUAL_INT(2, dst.ids[1]);
    TEST_ASSERT_EQUAL_INT(3, dst.ids[2]);
}

void test_copy_empty(void) {
    IdArray src, dst;
    idArrayInit(&src);
    idArrayInit(&dst);
    idArrayPush(&dst, 99);  // pre-existing data in dst

    idArrayCopy(&dst, &src);
    TEST_ASSERT_EQUAL_INT(0, dst.depth);
}

void test_copy_is_independent(void) {
    IdArray src, dst;
    idArrayInit(&src);
    idArrayInit(&dst);
    idArrayPush(&src, 5);
    idArrayCopy(&dst, &src);

    idArrayPush(&src, 10);
    TEST_ASSERT_EQUAL_INT(1, dst.depth);  // dst unaffected
}

// --- idArrayEqual ---

void test_equal_same(void) {
    IdArray a, b;
    idArrayInit(&a);
    idArrayInit(&b);
    idArrayPush(&a, 1);
    idArrayPush(&a, 2);
    idArrayPush(&b, 1);
    idArrayPush(&b, 2);
    TEST_ASSERT_TRUE(idArrayEqual(&a, &b));
}

void test_equal_both_empty(void) {
    IdArray a, b;
    idArrayInit(&a);
    idArrayInit(&b);
    TEST_ASSERT_TRUE(idArrayEqual(&a, &b));
}

void test_equal_different_depth(void) {
    IdArray a, b;
    idArrayInit(&a);
    idArrayInit(&b);
    idArrayPush(&a, 1);
    TEST_ASSERT_FALSE(idArrayEqual(&a, &b));
}

void test_equal_different_values(void) {
    IdArray a, b;
    idArrayInit(&a);
    idArrayInit(&b);
    idArrayPush(&a, 1);
    idArrayPush(&b, 2);
    TEST_ASSERT_FALSE(idArrayEqual(&a, &b));
}

// --- idArrayPush ---

void test_push_increments_depth(void) {
    IdArray arr;
    idArrayInit(&arr);
    idArrayPush(&arr, 42);
    TEST_ASSERT_EQUAL_INT(1, arr.depth);
    TEST_ASSERT_EQUAL_INT(42, arr.ids[0]);
}

void test_push_multiple(void) {
    IdArray arr;
    idArrayInit(&arr);
    idArrayPush(&arr, 10);
    idArrayPush(&arr, 20);
    idArrayPush(&arr, 30);
    TEST_ASSERT_EQUAL_INT(3, arr.depth);
    TEST_ASSERT_EQUAL_INT(10, arr.ids[0]);
    TEST_ASSERT_EQUAL_INT(20, arr.ids[1]);
    TEST_ASSERT_EQUAL_INT(30, arr.ids[2]);
}

void test_push_at_max_depth_is_noop(void) {
    IdArray arr;
    idArrayInit(&arr);
    for (int i = 0; i < MAX_ID_DEPTH; i++) {
        idArrayPush(&arr, i);
    }
    TEST_ASSERT_EQUAL_INT(MAX_ID_DEPTH, arr.depth);

    // One more push should be a no-op
    idArrayPush(&arr, 999);
    TEST_ASSERT_EQUAL_INT(MAX_ID_DEPTH, arr.depth);
}

// --- idArrayPop ---

void test_pop_returns_value(void) {
    IdArray arr;
    idArrayInit(&arr);
    idArrayPush(&arr, 5);
    idArrayPush(&arr, 10);
    TEST_ASSERT_EQUAL_INT(10, idArrayPop(&arr));
    TEST_ASSERT_EQUAL_INT(1, arr.depth);
}

void test_pop_empty_returns_negative_one(void) {
    IdArray arr;
    idArrayInit(&arr);
    TEST_ASSERT_EQUAL_INT(-1, idArrayPop(&arr));
    TEST_ASSERT_EQUAL_INT(0, arr.depth);
}

void test_pop_all(void) {
    IdArray arr;
    idArrayInit(&arr);
    idArrayPush(&arr, 1);
    idArrayPush(&arr, 2);
    TEST_ASSERT_EQUAL_INT(2, idArrayPop(&arr));
    TEST_ASSERT_EQUAL_INT(1, idArrayPop(&arr));
    TEST_ASSERT_EQUAL_INT(-1, idArrayPop(&arr));
}

// --- idArrayToString ---

void test_toString_empty(void) {
    IdArray arr;
    idArrayInit(&arr);
    char *str = idArrayToString(&arr);
    TEST_ASSERT_EQUAL_STRING("", str);
}

void test_toString_single(void) {
    IdArray arr;
    idArrayInit(&arr);
    idArrayPush(&arr, 42);
    char *str = idArrayToString(&arr);
    TEST_ASSERT_EQUAL_STRING("42", str);
}

void test_toString_multiple(void) {
    IdArray arr;
    idArrayInit(&arr);
    idArrayPush(&arr, 1);
    idArrayPush(&arr, 2);
    idArrayPush(&arr, 3);
    char *str = idArrayToString(&arr);
    TEST_ASSERT_EQUAL_STRING("1,2,3", str);
}

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_init_sets_depth_zero);
    RUN_TEST(test_init_zeros_ids);

    RUN_TEST(test_copy_populated);
    RUN_TEST(test_copy_empty);
    RUN_TEST(test_copy_is_independent);

    RUN_TEST(test_equal_same);
    RUN_TEST(test_equal_both_empty);
    RUN_TEST(test_equal_different_depth);
    RUN_TEST(test_equal_different_values);

    RUN_TEST(test_push_increments_depth);
    RUN_TEST(test_push_multiple);
    RUN_TEST(test_push_at_max_depth_is_noop);

    RUN_TEST(test_pop_returns_value);
    RUN_TEST(test_pop_empty_returns_negative_one);
    RUN_TEST(test_pop_all);

    RUN_TEST(test_toString_empty);
    RUN_TEST(test_toString_single);
    RUN_TEST(test_toString_multiple);

    return UNITY_END();
}

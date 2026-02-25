/*
 * Tests for FFON navigation functions.
 * Functions under test: getFfonAtId, nextFfonLayerExists, getFfonMaxIdAtPath
 */

#include <unity.h>
#include <ffon.h>
#include <stdlib.h>
#include <string.h>

// Test tree:
//   [0] "string0"
//   [1] obj "parent" -> [0] "child0", [1] "child1", [2] obj "nested" -> [0] "leaf"
//   [2] "string2"
static FfonElement *testElements[3];
static int testCount = 3;

void setUp(void) {
    testElements[0] = ffonElementCreateString("string0");

    testElements[1] = ffonElementCreateObject("parent");
    ffonObjectAddElement(testElements[1]->data.object, ffonElementCreateString("child0"));
    ffonObjectAddElement(testElements[1]->data.object, ffonElementCreateString("child1"));
    FfonElement *nested = ffonElementCreateObject("nested");
    ffonObjectAddElement(nested->data.object, ffonElementCreateString("leaf"));
    ffonObjectAddElement(testElements[1]->data.object, nested);

    testElements[2] = ffonElementCreateString("string2");
}

void tearDown(void) {
    for (int i = 0; i < testCount; i++) {
        ffonElementDestroy(testElements[i]);
    }
}

// --- getFfonAtId ---

void test_getFfonAtId_depth_zero(void) {
    IdArray id;
    idArrayInit(&id);
    int outCount;
    FfonElement **result = getFfonAtId(testElements, testCount, &id, &outCount);
    TEST_ASSERT_EQUAL_PTR(testElements, result);
    TEST_ASSERT_EQUAL_INT(3, outCount);
}

void test_getFfonAtId_depth_one_into_object(void) {
    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 1);  // points at "parent" object
    int outCount;
    FfonElement **result = getFfonAtId(testElements, testCount, &id, &outCount);
    // depth=1 means we're at root level, looking at index 1
    TEST_ASSERT_NOT_NULL(result);
    TEST_ASSERT_EQUAL_INT(3, outCount);  // root array count
}

void test_getFfonAtId_depth_two(void) {
    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 1);  // go into "parent"
    idArrayPush(&id, 2);  // look at "nested" within parent
    int outCount;
    FfonElement **result = getFfonAtId(testElements, testCount, &id, &outCount);
    TEST_ASSERT_NOT_NULL(result);
    TEST_ASSERT_EQUAL_INT(3, outCount);  // parent's children count
}

void test_getFfonAtId_depth_three(void) {
    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 1);  // into "parent"
    idArrayPush(&id, 2);  // into "nested"
    idArrayPush(&id, 0);  // look at "leaf"
    int outCount;
    FfonElement **result = getFfonAtId(testElements, testCount, &id, &outCount);
    TEST_ASSERT_NOT_NULL(result);
    TEST_ASSERT_EQUAL_INT(1, outCount);  // nested's children count
}

void test_getFfonAtId_invalid_index(void) {
    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 99);  // out of bounds
    idArrayPush(&id, 0);
    int outCount;
    FfonElement **result = getFfonAtId(testElements, testCount, &id, &outCount);
    TEST_ASSERT_NULL(result);
    TEST_ASSERT_EQUAL_INT(0, outCount);
}

void test_getFfonAtId_non_object_at_path(void) {
    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 0);  // "string0" is not an object
    idArrayPush(&id, 0);
    int outCount;
    FfonElement **result = getFfonAtId(testElements, testCount, &id, &outCount);
    TEST_ASSERT_NULL(result);
    TEST_ASSERT_EQUAL_INT(0, outCount);
}

// --- nextFfonLayerExists ---

void test_nextLayerExists_object(void) {
    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 1);  // "parent" is an object
    TEST_ASSERT_TRUE(nextFfonLayerExists(testElements, testCount, &id));
}

void test_nextLayerExists_string(void) {
    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 0);  // "string0" is a string
    TEST_ASSERT_FALSE(nextFfonLayerExists(testElements, testCount, &id));
}

void test_nextLayerExists_empty_id(void) {
    IdArray id;
    idArrayInit(&id);
    TEST_ASSERT_FALSE(nextFfonLayerExists(testElements, testCount, &id));
}

void test_nextLayerExists_out_of_bounds(void) {
    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 99);
    TEST_ASSERT_FALSE(nextFfonLayerExists(testElements, testCount, &id));
}

void test_nextLayerExists_nested_object(void) {
    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 1);  // into "parent"
    idArrayPush(&id, 2);  // "nested" is an object
    TEST_ASSERT_TRUE(nextFfonLayerExists(testElements, testCount, &id));
}

void test_nextLayerExists_nested_string(void) {
    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 1);  // into "parent"
    idArrayPush(&id, 0);  // "child0" is a string
    TEST_ASSERT_FALSE(nextFfonLayerExists(testElements, testCount, &id));
}

// --- getFfonMaxIdAtPath ---

void test_maxId_root(void) {
    IdArray id;
    idArrayInit(&id);
    int maxId = getFfonMaxIdAtPath(testElements, testCount, &id);
    TEST_ASSERT_EQUAL_INT(2, maxId);  // 3 elements, max index = 2
}

void test_maxId_nested(void) {
    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 1);  // into "parent" which has 3 children
    int maxId = getFfonMaxIdAtPath(testElements, testCount, &id);
    TEST_ASSERT_EQUAL_INT(2, maxId);
}

void test_maxId_deep_nested(void) {
    IdArray id;
    idArrayInit(&id);
    idArrayPush(&id, 1);  // into "parent"
    idArrayPush(&id, 2);  // into "nested"
    idArrayPush(&id, 0);  // look at "nested"'s children (1 child -> maxId 0)
    int maxId = getFfonMaxIdAtPath(testElements, testCount, &id);
    TEST_ASSERT_EQUAL_INT(0, maxId);
}

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_getFfonAtId_depth_zero);
    RUN_TEST(test_getFfonAtId_depth_one_into_object);
    RUN_TEST(test_getFfonAtId_depth_two);
    RUN_TEST(test_getFfonAtId_depth_three);
    RUN_TEST(test_getFfonAtId_invalid_index);
    RUN_TEST(test_getFfonAtId_non_object_at_path);

    RUN_TEST(test_nextLayerExists_object);
    RUN_TEST(test_nextLayerExists_string);
    RUN_TEST(test_nextLayerExists_empty_id);
    RUN_TEST(test_nextLayerExists_out_of_bounds);
    RUN_TEST(test_nextLayerExists_nested_object);
    RUN_TEST(test_nextLayerExists_nested_string);

    RUN_TEST(test_maxId_root);
    RUN_TEST(test_maxId_nested);
    RUN_TEST(test_maxId_deep_nested);

    return UNITY_END();
}

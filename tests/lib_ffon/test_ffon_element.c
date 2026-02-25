/*
 * Tests for FfonElement create/destroy/clone operations.
 * Functions under test: ffonElementCreateString, ffonElementCreateObject,
 *                       ffonElementDestroy, ffonElementClone
 */

#include <unity.h>
#include <ffon.h>
#include <string.h>

void setUp(void) {}
void tearDown(void) {}

// --- ffonElementCreateString ---

void test_createString_normal(void) {
    FfonElement *elem = ffonElementCreateString("hello");
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, elem->type);
    TEST_ASSERT_EQUAL_STRING("hello", elem->data.string);
    ffonElementDestroy(elem);
}

void test_createString_empty(void) {
    FfonElement *elem = ffonElementCreateString("");
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, elem->type);
    TEST_ASSERT_EQUAL_STRING("", elem->data.string);
    ffonElementDestroy(elem);
}

void test_createString_null_input(void) {
    FfonElement *elem = ffonElementCreateString(NULL);
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, elem->type);
    TEST_ASSERT_EQUAL_STRING("", elem->data.string);
    ffonElementDestroy(elem);
}

void test_createString_special_chars(void) {
    FfonElement *elem = ffonElementCreateString("<input>test</input>");
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_STRING("<input>test</input>", elem->data.string);
    ffonElementDestroy(elem);
}

void test_createString_is_independent_copy(void) {
    char buf[32] = "original";
    FfonElement *elem = ffonElementCreateString(buf);
    strcpy(buf, "modified");
    TEST_ASSERT_EQUAL_STRING("original", elem->data.string);
    ffonElementDestroy(elem);
}

// --- ffonElementCreateObject ---

void test_createObject_normal(void) {
    FfonElement *elem = ffonElementCreateObject("mykey");
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, elem->type);
    TEST_ASSERT_NOT_NULL(elem->data.object);
    TEST_ASSERT_EQUAL_STRING("mykey", elem->data.object->key);
    TEST_ASSERT_EQUAL_INT(0, elem->data.object->count);
    ffonElementDestroy(elem);
}

void test_createObject_empty_key(void) {
    FfonElement *elem = ffonElementCreateObject("");
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, elem->type);
    TEST_ASSERT_EQUAL_STRING("", elem->data.object->key);
    ffonElementDestroy(elem);
}

void test_createObject_null_key(void) {
    FfonElement *elem = ffonElementCreateObject(NULL);
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, elem->type);
    TEST_ASSERT_EQUAL_STRING("", elem->data.object->key);
    ffonElementDestroy(elem);
}

// --- ffonElementDestroy ---

void test_destroy_null(void) {
    ffonElementDestroy(NULL);  // should not crash
}

void test_destroy_string_element(void) {
    FfonElement *elem = ffonElementCreateString("test");
    ffonElementDestroy(elem);  // should not leak
}

void test_destroy_object_with_children(void) {
    FfonElement *parent = ffonElementCreateObject("parent");
    ffonObjectAddElement(parent->data.object, ffonElementCreateString("child1"));
    ffonObjectAddElement(parent->data.object, ffonElementCreateString("child2"));
    ffonElementDestroy(parent);  // should destroy children too
}

// --- ffonElementClone ---

void test_clone_null(void) {
    FfonElement *clone = ffonElementClone(NULL);
    TEST_ASSERT_NULL(clone);
}

void test_clone_string(void) {
    FfonElement *orig = ffonElementCreateString("hello");
    FfonElement *clone = ffonElementClone(orig);
    TEST_ASSERT_NOT_NULL(clone);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, clone->type);
    TEST_ASSERT_EQUAL_STRING("hello", clone->data.string);
    // Verify independence
    TEST_ASSERT_NOT_EQUAL(orig->data.string, clone->data.string);
    ffonElementDestroy(orig);
    ffonElementDestroy(clone);
}

void test_clone_object_empty(void) {
    FfonElement *orig = ffonElementCreateObject("key");
    FfonElement *clone = ffonElementClone(orig);
    TEST_ASSERT_NOT_NULL(clone);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, clone->type);
    TEST_ASSERT_EQUAL_STRING("key", clone->data.object->key);
    TEST_ASSERT_EQUAL_INT(0, clone->data.object->count);
    ffonElementDestroy(orig);
    ffonElementDestroy(clone);
}

void test_clone_object_with_children(void) {
    FfonElement *orig = ffonElementCreateObject("parent");
    ffonObjectAddElement(orig->data.object, ffonElementCreateString("child1"));
    ffonObjectAddElement(orig->data.object, ffonElementCreateString("child2"));

    FfonElement *clone = ffonElementClone(orig);
    TEST_ASSERT_NOT_NULL(clone);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, clone->type);
    TEST_ASSERT_EQUAL_INT(2, clone->data.object->count);
    TEST_ASSERT_EQUAL_STRING("child1", clone->data.object->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("child2", clone->data.object->elements[1]->data.string);

    // Verify independence
    TEST_ASSERT_NOT_EQUAL(orig->data.object->elements[0],
                          clone->data.object->elements[0]);

    ffonElementDestroy(orig);
    ffonElementDestroy(clone);
}

void test_clone_nested_object(void) {
    FfonElement *root = ffonElementCreateObject("root");
    FfonElement *child = ffonElementCreateObject("child");
    ffonObjectAddElement(child->data.object, ffonElementCreateString("leaf"));
    ffonObjectAddElement(root->data.object, child);

    FfonElement *clone = ffonElementClone(root);
    TEST_ASSERT_NOT_NULL(clone);
    TEST_ASSERT_EQUAL_INT(1, clone->data.object->count);
    FfonElement *clonedChild = clone->data.object->elements[0];
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, clonedChild->type);
    TEST_ASSERT_EQUAL_STRING("child", clonedChild->data.object->key);
    TEST_ASSERT_EQUAL_INT(1, clonedChild->data.object->count);
    TEST_ASSERT_EQUAL_STRING("leaf", clonedChild->data.object->elements[0]->data.string);

    ffonElementDestroy(root);
    ffonElementDestroy(clone);
}

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_createString_normal);
    RUN_TEST(test_createString_empty);
    RUN_TEST(test_createString_null_input);
    RUN_TEST(test_createString_special_chars);
    RUN_TEST(test_createString_is_independent_copy);

    RUN_TEST(test_createObject_normal);
    RUN_TEST(test_createObject_empty_key);
    RUN_TEST(test_createObject_null_key);

    RUN_TEST(test_destroy_null);
    RUN_TEST(test_destroy_string_element);
    RUN_TEST(test_destroy_object_with_children);

    RUN_TEST(test_clone_null);
    RUN_TEST(test_clone_string);
    RUN_TEST(test_clone_object_empty);
    RUN_TEST(test_clone_object_with_children);
    RUN_TEST(test_clone_nested_object);

    return UNITY_END();
}

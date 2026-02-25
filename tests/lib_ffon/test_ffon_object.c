/*
 * Tests for FfonObject operations.
 * Functions under test: ffonObjectCreate, ffonObjectDestroy,
 *                       ffonObjectAddElement, ffonObjectInsertElement,
 *                       ffonObjectRemoveElement
 */

#include <unity.h>
#include <ffon.h>
#include <string.h>

void setUp(void) {}
void tearDown(void) {}

// --- ffonObjectCreate ---

void test_objectCreate_normal(void) {
    FfonObject *obj = ffonObjectCreate("testkey");
    TEST_ASSERT_NOT_NULL(obj);
    TEST_ASSERT_EQUAL_STRING("testkey", obj->key);
    TEST_ASSERT_EQUAL_INT(0, obj->count);
    TEST_ASSERT_TRUE(obj->capacity >= 10);
    TEST_ASSERT_NOT_NULL(obj->elements);
    ffonObjectDestroy(obj);
}

void test_objectCreate_empty_key(void) {
    FfonObject *obj = ffonObjectCreate("");
    TEST_ASSERT_NOT_NULL(obj);
    TEST_ASSERT_EQUAL_STRING("", obj->key);
    ffonObjectDestroy(obj);
}

void test_objectCreate_null_key(void) {
    FfonObject *obj = ffonObjectCreate(NULL);
    TEST_ASSERT_NOT_NULL(obj);
    TEST_ASSERT_EQUAL_STRING("", obj->key);
    ffonObjectDestroy(obj);
}

// --- ffonObjectDestroy ---

void test_objectDestroy_null(void) {
    ffonObjectDestroy(NULL);  // should not crash
}

void test_objectDestroy_empty(void) {
    FfonObject *obj = ffonObjectCreate("key");
    ffonObjectDestroy(obj);  // should not leak
}

void test_objectDestroy_with_elements(void) {
    FfonObject *obj = ffonObjectCreate("key");
    ffonObjectAddElement(obj, ffonElementCreateString("a"));
    ffonObjectAddElement(obj, ffonElementCreateString("b"));
    ffonObjectDestroy(obj);  // should destroy elements too
}

// --- ffonObjectAddElement ---

void test_addElement_one(void) {
    FfonObject *obj = ffonObjectCreate("key");
    FfonElement *elem = ffonElementCreateString("hello");
    ffonObjectAddElement(obj, elem);
    TEST_ASSERT_EQUAL_INT(1, obj->count);
    TEST_ASSERT_EQUAL_STRING("hello", obj->elements[0]->data.string);
    ffonObjectDestroy(obj);
}

void test_addElement_multiple(void) {
    FfonObject *obj = ffonObjectCreate("key");
    for (int i = 0; i < 5; i++) {
        char buf[16];
        snprintf(buf, sizeof(buf), "item%d", i);
        ffonObjectAddElement(obj, ffonElementCreateString(buf));
    }
    TEST_ASSERT_EQUAL_INT(5, obj->count);
    TEST_ASSERT_EQUAL_STRING("item0", obj->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("item4", obj->elements[4]->data.string);
    ffonObjectDestroy(obj);
}

void test_addElement_triggers_resize(void) {
    FfonObject *obj = ffonObjectCreate("key");
    // Initial capacity is 10, adding 15 elements should trigger resize
    for (int i = 0; i < 15; i++) {
        char buf[16];
        snprintf(buf, sizeof(buf), "item%d", i);
        ffonObjectAddElement(obj, ffonElementCreateString(buf));
    }
    TEST_ASSERT_EQUAL_INT(15, obj->count);
    TEST_ASSERT_TRUE(obj->capacity >= 15);
    TEST_ASSERT_EQUAL_STRING("item14", obj->elements[14]->data.string);
    ffonObjectDestroy(obj);
}

void test_addElement_null_obj(void) {
    FfonElement *elem = ffonElementCreateString("test");
    ffonObjectAddElement(NULL, elem);  // should not crash
    ffonElementDestroy(elem);
}

void test_addElement_null_elem(void) {
    FfonObject *obj = ffonObjectCreate("key");
    ffonObjectAddElement(obj, NULL);  // should not crash
    TEST_ASSERT_EQUAL_INT(0, obj->count);
    ffonObjectDestroy(obj);
}

// --- ffonObjectInsertElement ---

void test_insertElement_at_beginning(void) {
    FfonObject *obj = ffonObjectCreate("key");
    ffonObjectAddElement(obj, ffonElementCreateString("b"));
    ffonObjectAddElement(obj, ffonElementCreateString("c"));
    ffonObjectInsertElement(obj, ffonElementCreateString("a"), 0);
    TEST_ASSERT_EQUAL_INT(3, obj->count);
    TEST_ASSERT_EQUAL_STRING("a", obj->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("b", obj->elements[1]->data.string);
    TEST_ASSERT_EQUAL_STRING("c", obj->elements[2]->data.string);
    ffonObjectDestroy(obj);
}

void test_insertElement_at_middle(void) {
    FfonObject *obj = ffonObjectCreate("key");
    ffonObjectAddElement(obj, ffonElementCreateString("a"));
    ffonObjectAddElement(obj, ffonElementCreateString("c"));
    ffonObjectInsertElement(obj, ffonElementCreateString("b"), 1);
    TEST_ASSERT_EQUAL_INT(3, obj->count);
    TEST_ASSERT_EQUAL_STRING("a", obj->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("b", obj->elements[1]->data.string);
    TEST_ASSERT_EQUAL_STRING("c", obj->elements[2]->data.string);
    ffonObjectDestroy(obj);
}

void test_insertElement_at_end(void) {
    FfonObject *obj = ffonObjectCreate("key");
    ffonObjectAddElement(obj, ffonElementCreateString("a"));
    ffonObjectInsertElement(obj, ffonElementCreateString("b"), 1);
    TEST_ASSERT_EQUAL_INT(2, obj->count);
    TEST_ASSERT_EQUAL_STRING("a", obj->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("b", obj->elements[1]->data.string);
    ffonObjectDestroy(obj);
}

void test_insertElement_negative_index_clamped(void) {
    FfonObject *obj = ffonObjectCreate("key");
    ffonObjectAddElement(obj, ffonElementCreateString("b"));
    ffonObjectInsertElement(obj, ffonElementCreateString("a"), -5);
    TEST_ASSERT_EQUAL_INT(2, obj->count);
    TEST_ASSERT_EQUAL_STRING("a", obj->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("b", obj->elements[1]->data.string);
    ffonObjectDestroy(obj);
}

void test_insertElement_beyond_count_clamped(void) {
    FfonObject *obj = ffonObjectCreate("key");
    ffonObjectAddElement(obj, ffonElementCreateString("a"));
    ffonObjectInsertElement(obj, ffonElementCreateString("b"), 100);
    TEST_ASSERT_EQUAL_INT(2, obj->count);
    TEST_ASSERT_EQUAL_STRING("a", obj->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("b", obj->elements[1]->data.string);
    ffonObjectDestroy(obj);
}

// --- ffonObjectRemoveElement ---

void test_removeElement_first(void) {
    FfonObject *obj = ffonObjectCreate("key");
    ffonObjectAddElement(obj, ffonElementCreateString("a"));
    ffonObjectAddElement(obj, ffonElementCreateString("b"));
    ffonObjectAddElement(obj, ffonElementCreateString("c"));
    FfonElement *removed = ffonObjectRemoveElement(obj, 0);
    TEST_ASSERT_NOT_NULL(removed);
    TEST_ASSERT_EQUAL_STRING("a", removed->data.string);
    TEST_ASSERT_EQUAL_INT(2, obj->count);
    TEST_ASSERT_EQUAL_STRING("b", obj->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("c", obj->elements[1]->data.string);
    ffonElementDestroy(removed);
    ffonObjectDestroy(obj);
}

void test_removeElement_middle(void) {
    FfonObject *obj = ffonObjectCreate("key");
    ffonObjectAddElement(obj, ffonElementCreateString("a"));
    ffonObjectAddElement(obj, ffonElementCreateString("b"));
    ffonObjectAddElement(obj, ffonElementCreateString("c"));
    FfonElement *removed = ffonObjectRemoveElement(obj, 1);
    TEST_ASSERT_NOT_NULL(removed);
    TEST_ASSERT_EQUAL_STRING("b", removed->data.string);
    TEST_ASSERT_EQUAL_INT(2, obj->count);
    TEST_ASSERT_EQUAL_STRING("a", obj->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("c", obj->elements[1]->data.string);
    ffonElementDestroy(removed);
    ffonObjectDestroy(obj);
}

void test_removeElement_last(void) {
    FfonObject *obj = ffonObjectCreate("key");
    ffonObjectAddElement(obj, ffonElementCreateString("a"));
    ffonObjectAddElement(obj, ffonElementCreateString("b"));
    FfonElement *removed = ffonObjectRemoveElement(obj, 1);
    TEST_ASSERT_NOT_NULL(removed);
    TEST_ASSERT_EQUAL_STRING("b", removed->data.string);
    TEST_ASSERT_EQUAL_INT(1, obj->count);
    ffonElementDestroy(removed);
    ffonObjectDestroy(obj);
}

void test_removeElement_out_of_bounds(void) {
    FfonObject *obj = ffonObjectCreate("key");
    ffonObjectAddElement(obj, ffonElementCreateString("a"));
    TEST_ASSERT_NULL(ffonObjectRemoveElement(obj, 5));
    TEST_ASSERT_NULL(ffonObjectRemoveElement(obj, -1));
    TEST_ASSERT_EQUAL_INT(1, obj->count);
    ffonObjectDestroy(obj);
}

void test_removeElement_null_obj(void) {
    TEST_ASSERT_NULL(ffonObjectRemoveElement(NULL, 0));
}

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_objectCreate_normal);
    RUN_TEST(test_objectCreate_empty_key);
    RUN_TEST(test_objectCreate_null_key);

    RUN_TEST(test_objectDestroy_null);
    RUN_TEST(test_objectDestroy_empty);
    RUN_TEST(test_objectDestroy_with_elements);

    RUN_TEST(test_addElement_one);
    RUN_TEST(test_addElement_multiple);
    RUN_TEST(test_addElement_triggers_resize);
    RUN_TEST(test_addElement_null_obj);
    RUN_TEST(test_addElement_null_elem);

    RUN_TEST(test_insertElement_at_beginning);
    RUN_TEST(test_insertElement_at_middle);
    RUN_TEST(test_insertElement_at_end);
    RUN_TEST(test_insertElement_negative_index_clamped);
    RUN_TEST(test_insertElement_beyond_count_clamped);

    RUN_TEST(test_removeElement_first);
    RUN_TEST(test_removeElement_middle);
    RUN_TEST(test_removeElement_last);
    RUN_TEST(test_removeElement_out_of_bounds);
    RUN_TEST(test_removeElement_null_obj);

    return UNITY_END();
}

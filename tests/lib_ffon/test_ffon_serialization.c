/*
 * Tests for FFON binary serialization/deserialization and JSON parsing.
 * Functions under test: ffonSerializeBinary, ffonDeserializeBinary,
 *                       saveFfonFile, loadFfonFileToElements,
 *                       parseJsonValue, loadJsonFileToElements
 */

#include <unity.h>
#include <ffon.h>
#include <json-c/json.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <unistd.h>

static char tmpDir[256];

void setUp(void) {
    snprintf(tmpDir, sizeof(tmpDir), "/tmp/sicompass_ffon_test_XXXXXX");
    mkdtemp(tmpDir);
}

void tearDown(void) {
    char cmd[512];
    snprintf(cmd, sizeof(cmd), "rm -rf %s", tmpDir);
    system(cmd);
}

// --- Binary serialization round-trip ---

void test_serialize_deserialize_strings(void) {
    FfonElement *elems[2];
    elems[0] = ffonElementCreateString("hello");
    elems[1] = ffonElementCreateString("world");

    size_t size;
    uint8_t *data = ffonSerializeBinary(elems, 2, &size);
    TEST_ASSERT_NOT_NULL(data);
    TEST_ASSERT_TRUE(size > 0);

    int outCount;
    FfonElement **result = ffonDeserializeBinary(data, size, &outCount);
    TEST_ASSERT_EQUAL_INT(2, outCount);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, result[0]->type);
    TEST_ASSERT_EQUAL_STRING("hello", result[0]->data.string);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, result[1]->type);
    TEST_ASSERT_EQUAL_STRING("world", result[1]->data.string);

    for (int i = 0; i < 2; i++) ffonElementDestroy(elems[i]);
    for (int i = 0; i < outCount; i++) ffonElementDestroy(result[i]);
    free(result);
    free(data);
}

void test_serialize_deserialize_object(void) {
    FfonElement *elems[1];
    elems[0] = ffonElementCreateObject("myobj");
    ffonObjectAddElement(elems[0]->data.object, ffonElementCreateString("child1"));
    ffonObjectAddElement(elems[0]->data.object, ffonElementCreateString("child2"));

    size_t size;
    uint8_t *data = ffonSerializeBinary(elems, 1, &size);
    TEST_ASSERT_NOT_NULL(data);

    int outCount;
    FfonElement **result = ffonDeserializeBinary(data, size, &outCount);
    TEST_ASSERT_EQUAL_INT(1, outCount);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, result[0]->type);
    TEST_ASSERT_EQUAL_STRING("myobj", result[0]->data.object->key);
    TEST_ASSERT_EQUAL_INT(2, result[0]->data.object->count);
    TEST_ASSERT_EQUAL_STRING("child1", result[0]->data.object->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("child2", result[0]->data.object->elements[1]->data.string);

    ffonElementDestroy(elems[0]);
    for (int i = 0; i < outCount; i++) ffonElementDestroy(result[i]);
    free(result);
    free(data);
}

void test_serialize_deserialize_nested(void) {
    FfonElement *root = ffonElementCreateObject("root");
    FfonElement *child = ffonElementCreateObject("child");
    ffonObjectAddElement(child->data.object, ffonElementCreateString("leaf"));
    ffonObjectAddElement(root->data.object, child);
    ffonObjectAddElement(root->data.object, ffonElementCreateString("sibling"));

    FfonElement *elems[1] = { root };
    size_t size;
    uint8_t *data = ffonSerializeBinary(elems, 1, &size);
    TEST_ASSERT_NOT_NULL(data);

    int outCount;
    FfonElement **result = ffonDeserializeBinary(data, size, &outCount);
    TEST_ASSERT_EQUAL_INT(1, outCount);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, result[0]->type);
    TEST_ASSERT_EQUAL_STRING("root", result[0]->data.object->key);

    FfonObject *rootObj = result[0]->data.object;
    TEST_ASSERT_EQUAL_INT(2, rootObj->count);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, rootObj->elements[0]->type);
    TEST_ASSERT_EQUAL_STRING("child", rootObj->elements[0]->data.object->key);
    TEST_ASSERT_EQUAL_INT(1, rootObj->elements[0]->data.object->count);
    TEST_ASSERT_EQUAL_STRING("leaf", rootObj->elements[0]->data.object->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("sibling", rootObj->elements[1]->data.string);

    ffonElementDestroy(root);
    for (int i = 0; i < outCount; i++) ffonElementDestroy(result[i]);
    free(result);
    free(data);
}

void test_deserialize_empty(void) {
    int outCount;
    FfonElement **result = ffonDeserializeBinary(NULL, 0, &outCount);
    TEST_ASSERT_NULL(result);
    TEST_ASSERT_EQUAL_INT(0, outCount);
}

// --- File save/load ---

void test_save_load_ffon_file(void) {
    FfonElement *elems[2];
    elems[0] = ffonElementCreateString("hello");
    elems[1] = ffonElementCreateObject("obj");
    ffonObjectAddElement(elems[1]->data.object, ffonElementCreateString("child"));

    char filepath[512];
    snprintf(filepath, sizeof(filepath), "%s/test.ffon", tmpDir);

    bool saved = saveFfonFile(elems, 2, filepath);
    TEST_ASSERT_TRUE(saved);

    int outCount;
    FfonElement **loaded = loadFfonFileToElements(filepath, &outCount);
    TEST_ASSERT_NOT_NULL(loaded);
    TEST_ASSERT_EQUAL_INT(2, outCount);
    TEST_ASSERT_EQUAL_STRING("hello", loaded[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("obj", loaded[1]->data.object->key);

    for (int i = 0; i < 2; i++) ffonElementDestroy(elems[i]);
    for (int i = 0; i < outCount; i++) ffonElementDestroy(loaded[i]);
    free(loaded);
}

void test_load_ffon_nonexistent(void) {
    int outCount;
    FfonElement **result = loadFfonFileToElements("/nonexistent/path.ffon", &outCount);
    TEST_ASSERT_NULL(result);
    TEST_ASSERT_EQUAL_INT(0, outCount);
}

// --- JSON parsing ---

void test_parseJsonValue_string(void) {
    json_object *jobj = json_tokener_parse("\"hello\"");
    FfonElement *elem = parseJsonValue(jobj);
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, elem->type);
    TEST_ASSERT_EQUAL_STRING("hello", elem->data.string);
    ffonElementDestroy(elem);
    json_object_put(jobj);
}

void test_parseJsonValue_integer(void) {
    json_object *jobj = json_tokener_parse("42");
    FfonElement *elem = parseJsonValue(jobj);
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, elem->type);
    TEST_ASSERT_EQUAL_STRING("42", elem->data.string);
    ffonElementDestroy(elem);
    json_object_put(jobj);
}

void test_parseJsonValue_boolean(void) {
    json_object *jobj = json_tokener_parse("true");
    FfonElement *elem = parseJsonValue(jobj);
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_STRING("true", elem->data.string);
    ffonElementDestroy(elem);
    json_object_put(jobj);
}

void test_parseJsonValue_null(void) {
    FfonElement *elem = parseJsonValue(NULL);
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_STRING("", elem->data.string);
    ffonElementDestroy(elem);
}

void test_parseJsonValue_object(void) {
    json_object *jobj = json_tokener_parse("{\"key\": [\"a\", \"b\"]}");
    FfonElement *elem = parseJsonValue(jobj);
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, elem->type);
    TEST_ASSERT_EQUAL_STRING("key", elem->data.object->key);
    TEST_ASSERT_EQUAL_INT(2, elem->data.object->count);
    ffonElementDestroy(elem);
    json_object_put(jobj);
}

void test_parseJsonValue_array(void) {
    json_object *jobj = json_tokener_parse("[\"x\", \"y\"]");
    FfonElement *elem = parseJsonValue(jobj);
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, elem->type);
    TEST_ASSERT_EQUAL_STRING("array", elem->data.object->key);
    TEST_ASSERT_EQUAL_INT(2, elem->data.object->count);
    ffonElementDestroy(elem);
    json_object_put(jobj);
}

// --- loadJsonFileToElements ---

void test_loadJsonFile_valid(void) {
    char filepath[512];
    snprintf(filepath, sizeof(filepath), "%s/test.json", tmpDir);
    FILE *fp = fopen(filepath, "w");
    fprintf(fp, "[\"item1\", {\"key\": [\"child\"]}]");
    fclose(fp);

    int outCount;
    FfonElement **result = loadJsonFileToElements(filepath, &outCount);
    TEST_ASSERT_NOT_NULL(result);
    TEST_ASSERT_EQUAL_INT(2, outCount);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, result[0]->type);
    TEST_ASSERT_EQUAL_STRING("item1", result[0]->data.string);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, result[1]->type);
    TEST_ASSERT_EQUAL_STRING("key", result[1]->data.object->key);

    for (int i = 0; i < outCount; i++) ffonElementDestroy(result[i]);
    free(result);
}

void test_loadJsonFile_nonexistent(void) {
    int outCount;
    FfonElement **result = loadJsonFileToElements("/nonexistent/file.json", &outCount);
    TEST_ASSERT_NULL(result);
    TEST_ASSERT_EQUAL_INT(0, outCount);
}

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_serialize_deserialize_strings);
    RUN_TEST(test_serialize_deserialize_object);
    RUN_TEST(test_serialize_deserialize_nested);
    RUN_TEST(test_deserialize_empty);

    RUN_TEST(test_save_load_ffon_file);
    RUN_TEST(test_load_ffon_nonexistent);

    RUN_TEST(test_parseJsonValue_string);
    RUN_TEST(test_parseJsonValue_integer);
    RUN_TEST(test_parseJsonValue_boolean);
    RUN_TEST(test_parseJsonValue_null);
    RUN_TEST(test_parseJsonValue_object);
    RUN_TEST(test_parseJsonValue_array);

    RUN_TEST(test_loadJsonFile_valid);
    RUN_TEST(test_loadJsonFile_nonexistent);

    return UNITY_END();
}

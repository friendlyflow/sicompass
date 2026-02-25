/*
 * Tests for provider interface functions.
 * Functions under test: providerCreate, providerDestroy, providerFreeCommandListItems,
 *                       providerGetInitialElement, providerFactoryRegister/Create,
 *                       generic path management (pushPath, popPath, getCurrentPath, setCurrentPath)
 */

#include <unity.h>
#include <fff/fff.h>
#include <provider_interface.h>
#include <ffon.h>
#include <stdlib.h>
#include <string.h>

DEFINE_FFF_GLOBALS;

// Mock fetch function for testing
static FfonElement **mockElements = NULL;
static int mockCount = 0;

static FfonElement** mockFetch(const char *path, int *outCount) {
    (void)path;
    if (!mockElements) {
        *outCount = 0;
        return NULL;
    }
    // Return clones so caller can free them
    FfonElement **result = malloc(mockCount * sizeof(FfonElement*));
    for (int i = 0; i < mockCount; i++) {
        result[i] = ffonElementClone(mockElements[i]);
    }
    *outCount = mockCount;
    return result;
}

static FfonElement *storedElements[4];

void setUp(void) {
    FFF_RESET_HISTORY();
    mockElements = NULL;
    mockCount = 0;
    memset(storedElements, 0, sizeof(storedElements));
}

void tearDown(void) {
    for (int i = 0; i < 4; i++) {
        if (storedElements[i]) {
            ffonElementDestroy(storedElements[i]);
            storedElements[i] = NULL;
        }
    }
}

// --- providerCreate ---

void test_providerCreate_minimal(void) {
    ProviderOps ops = { .name = "test", .fetch = mockFetch };
    Provider *p = providerCreate(&ops);
    TEST_ASSERT_NOT_NULL(p);
    TEST_ASSERT_EQUAL_STRING("test", p->name);
    TEST_ASSERT_NOT_NULL(p->fetch);
    TEST_ASSERT_NOT_NULL(p->pushPath);
    TEST_ASSERT_NOT_NULL(p->popPath);
    TEST_ASSERT_NOT_NULL(p->getCurrentPath);
    TEST_ASSERT_NOT_NULL(p->init);
    TEST_ASSERT_NULL(p->commitEdit);
    TEST_ASSERT_NULL(p->createDirectory);
    TEST_ASSERT_NULL(p->createFile);
    providerDestroy(p);
}

void test_providerCreate_null_ops(void) {
    Provider *p = providerCreate(NULL);
    TEST_ASSERT_NULL(p);
}

void test_providerCreate_null_name(void) {
    ProviderOps ops = { .name = NULL, .fetch = mockFetch };
    Provider *p = providerCreate(&ops);
    TEST_ASSERT_NULL(p);
}

// --- providerDestroy ---

void test_providerDestroy_null(void) {
    providerDestroy(NULL);  // should not crash
}

void test_providerDestroy_normal(void) {
    ProviderOps ops = { .name = "test", .fetch = mockFetch };
    Provider *p = providerCreate(&ops);
    providerDestroy(p);  // should not leak
}

// --- providerFreeCommandListItems ---

void test_freeCommandListItems_null(void) {
    providerFreeCommandListItems(NULL, 0);  // should not crash
}

void test_freeCommandListItems_populated(void) {
    ProviderListItem *items = malloc(2 * sizeof(ProviderListItem));
    items[0].label = strdup("label1");
    items[0].data = strdup("data1");
    items[1].label = strdup("label2");
    items[1].data = strdup("data2");
    providerFreeCommandListItems(items, 2);  // should free all
}

// --- Generic path management ---

void test_init_sets_root_path(void) {
    ProviderOps ops = { .name = "test", .fetch = mockFetch };
    Provider *p = providerCreate(&ops);
    p->init(p);
    TEST_ASSERT_EQUAL_STRING("/", p->getCurrentPath(p));
    providerDestroy(p);
}

void test_pushPath_appends(void) {
    ProviderOps ops = { .name = "test", .fetch = mockFetch };
    Provider *p = providerCreate(&ops);
    p->init(p);
    p->pushPath(p, "documents");
    TEST_ASSERT_EQUAL_STRING("/documents", p->getCurrentPath(p));
    providerDestroy(p);
}

void test_pushPath_multiple(void) {
    ProviderOps ops = { .name = "test", .fetch = mockFetch };
    Provider *p = providerCreate(&ops);
    p->init(p);
    p->pushPath(p, "home");
    p->pushPath(p, "user");
    TEST_ASSERT_EQUAL_STRING("/home/user", p->getCurrentPath(p));
    providerDestroy(p);
}

void test_pushPath_strips_trailing_slash(void) {
    ProviderOps ops = { .name = "test", .fetch = mockFetch };
    Provider *p = providerCreate(&ops);
    p->init(p);
    p->pushPath(p, "docs/");
    TEST_ASSERT_EQUAL_STRING("/docs", p->getCurrentPath(p));
    providerDestroy(p);
}

void test_popPath_removes_last(void) {
    ProviderOps ops = { .name = "test", .fetch = mockFetch };
    Provider *p = providerCreate(&ops);
    p->init(p);
    p->pushPath(p, "home");
    p->pushPath(p, "user");
    p->popPath(p);
    TEST_ASSERT_EQUAL_STRING("/home", p->getCurrentPath(p));
    providerDestroy(p);
}

void test_popPath_to_root(void) {
    ProviderOps ops = { .name = "test", .fetch = mockFetch };
    Provider *p = providerCreate(&ops);
    p->init(p);
    p->pushPath(p, "home");
    p->popPath(p);
    TEST_ASSERT_EQUAL_STRING("/", p->getCurrentPath(p));
    providerDestroy(p);
}

void test_popPath_at_root_stays(void) {
    ProviderOps ops = { .name = "test", .fetch = mockFetch };
    Provider *p = providerCreate(&ops);
    p->init(p);
    p->popPath(p);
    TEST_ASSERT_EQUAL_STRING("/", p->getCurrentPath(p));
    providerDestroy(p);
}

void test_setCurrentPath(void) {
    ProviderOps ops = { .name = "test", .fetch = mockFetch };
    Provider *p = providerCreate(&ops);
    p->init(p);
    p->setCurrentPath(p, "/some/absolute/path");
    TEST_ASSERT_EQUAL_STRING("/some/absolute/path", p->getCurrentPath(p));
    providerDestroy(p);
}

// --- providerGetInitialElement ---

void test_getInitialElement_with_children(void) {
    storedElements[0] = ffonElementCreateString("child1");
    storedElements[1] = ffonElementCreateString("child2");
    mockElements = storedElements;
    mockCount = 2;

    ProviderOps ops = { .name = "test", .displayName = "Test Provider", .fetch = mockFetch };
    Provider *p = providerCreate(&ops);
    p->init(p);

    FfonElement *root = providerGetInitialElement(p);
    TEST_ASSERT_NOT_NULL(root);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, root->type);
    TEST_ASSERT_EQUAL_STRING("Test Provider", root->data.object->key);
    TEST_ASSERT_EQUAL_INT(2, root->data.object->count);

    ffonElementDestroy(root);
    providerDestroy(p);
}

void test_getInitialElement_uses_name_when_no_displayName(void) {
    storedElements[0] = ffonElementCreateString("child");
    mockElements = storedElements;
    mockCount = 1;

    ProviderOps ops = { .name = "myname", .displayName = NULL, .fetch = mockFetch };
    Provider *p = providerCreate(&ops);
    p->init(p);

    FfonElement *root = providerGetInitialElement(p);
    TEST_ASSERT_NOT_NULL(root);
    TEST_ASSERT_EQUAL_STRING("myname", root->data.object->key);

    ffonElementDestroy(root);
    providerDestroy(p);
}

void test_getInitialElement_null_provider(void) {
    TEST_ASSERT_NULL(providerGetInitialElement(NULL));
}

// --- Factory ---

static Provider* dummyFactoryCreate(void) {
    ProviderOps ops = { .name = "dummy", .fetch = mockFetch };
    return providerCreate(&ops);
}

void test_factory_register_and_create(void) {
    providerFactoryRegister("dummy", dummyFactoryCreate);
    Provider *p = providerFactoryCreate("dummy");
    TEST_ASSERT_NOT_NULL(p);
    TEST_ASSERT_EQUAL_STRING("dummy", p->name);
    providerDestroy(p);
}

void test_factory_create_unknown(void) {
    Provider *p = providerFactoryCreate("nonexistent_provider_xyz");
    TEST_ASSERT_NULL(p);
}

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_providerCreate_minimal);
    RUN_TEST(test_providerCreate_null_ops);
    RUN_TEST(test_providerCreate_null_name);

    RUN_TEST(test_providerDestroy_null);
    RUN_TEST(test_providerDestroy_normal);

    RUN_TEST(test_freeCommandListItems_null);
    RUN_TEST(test_freeCommandListItems_populated);

    RUN_TEST(test_init_sets_root_path);
    RUN_TEST(test_pushPath_appends);
    RUN_TEST(test_pushPath_multiple);
    RUN_TEST(test_pushPath_strips_trailing_slash);
    RUN_TEST(test_popPath_removes_last);
    RUN_TEST(test_popPath_to_root);
    RUN_TEST(test_popPath_at_root_stays);
    RUN_TEST(test_setCurrentPath);

    RUN_TEST(test_getInitialElement_with_children);
    RUN_TEST(test_getInitialElement_uses_name_when_no_displayName);
    RUN_TEST(test_getInitialElement_null_provider);

    RUN_TEST(test_factory_register_and_create);
    RUN_TEST(test_factory_create_unknown);

    return UNITY_END();
}

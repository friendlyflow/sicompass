/*
 * Tests for provider.c functions:
 * - Provider registry: providerRegister, providerFindByName,
 *   providerGetRegisteredCount, providerGetRegisteredAt
 * - Provider dispatch: providerGetActive, providerGetCurrentPath,
 *   providerCommitEdit, providerCreateDirectory, providerCreateFile,
 *   providerDeleteItem, providerGetCommands, providerExecuteCommand
 * - Navigation: providerNavigateLeft
 */

#include <unity.h>
#include <fff/fff.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

DEFINE_FFF_GLOBALS;

// Mock setErrorMessage
FAKE_VOID_FUNC(setErrorMessage, void*, const char*);

/* ============================================
 * Type definitions
 * ============================================ */

#define MAX_ID_DEPTH 32
#define MAX_PROVIDERS 16

typedef enum { FFON_STRING, FFON_OBJECT } FfonType;

typedef struct FfonElement FfonElement;
typedef struct FfonObject FfonObject;

struct FfonObject {
    char *key;
    FfonElement **elements;
    int count;
    int capacity;
};

struct FfonElement {
    FfonType type;
    union {
        char *string;
        FfonObject *object;
    } data;
};

typedef struct {
    int ids[MAX_ID_DEPTH];
    int depth;
} IdArray;

typedef struct {
    char *label;
    char *data;
} ProviderListItem;

typedef struct {
    char *label;
    char *breadcrumb;
    char *navPath;
} SearchResultItem;

typedef struct Provider {
    const char *name;
    FfonElement** (*fetch)(struct Provider *self, int *outCount);
    bool (*commitEdit)(struct Provider *self, const char *oldContent, const char *newContent);
    void (*init)(struct Provider *self);
    void (*cleanup)(struct Provider *self);
    void (*pushPath)(struct Provider *self, const char *segment);
    void (*popPath)(struct Provider *self);
    const char* (*getCurrentPath)(struct Provider *self);
    bool (*createDirectory)(struct Provider *self, const char *name);
    bool (*createFile)(struct Provider *self, const char *name);
    bool (*deleteItem)(struct Provider *self, const char *name);
    bool (*copyItem)(struct Provider *self, const char *srcDir, const char *srcName,
                     const char *destDir, const char *destName);
    const char** (*getCommands)(struct Provider *self, int *outCount);
    FfonElement* (*handleCommand)(struct Provider *self, const char *command,
                                   const char *elementKey, int elementType,
                                   char *errorMsg, int errorMsgSize);
    ProviderListItem* (*getCommandListItems)(struct Provider *self, const char *command, int *outCount);
    bool (*executeCommand)(struct Provider *self, const char *command, const char *selection);
    void (*onRadioChange)(struct Provider *self, const char *groupKey, const char *selectedValue);
    void (*onButtonPress)(struct Provider *self, const char *functionName);
    FfonElement* (*createElement)(struct Provider *self, const char *elementKey);
    void (*setCurrentPath)(struct Provider *self, const char *absolutePath);
    SearchResultItem* (*collectDeepSearchItems)(struct Provider *self, int *outCount);
    bool (*loadConfig)(struct Provider *self, const char *configPath);
    bool (*saveConfig)(struct Provider *self, const char *configPath);
    void *state;
} Provider;

typedef struct {
    FfonElement **ffon;
    Provider **providers;
    int ffonCount;
    int ffonCapacity;
    IdArray currentId;
    IdArray previousId;
    char errorMessage[256];
} AppRenderer;

/* ============================================
 * IdArray helpers
 * ============================================ */

static void idArrayInit(IdArray *arr) {
    memset(arr, 0, sizeof(IdArray));
}

static void idArrayCopy(IdArray *dst, const IdArray *src) {
    memcpy(dst, src, sizeof(IdArray));
}

static void idArrayPush(IdArray *arr, int val) {
    if (arr->depth < MAX_ID_DEPTH) {
        arr->ids[arr->depth++] = val;
    }
}

static int idArrayPop(IdArray *arr) {
    if (arr->depth > 0) {
        return arr->ids[--arr->depth];
    }
    return -1;
}

/* ============================================
 * Provider registry (from provider.c)
 * ============================================ */

static Provider *g_providers[MAX_PROVIDERS];
static int g_providerCount = 0;

void providerRegister(Provider *provider) {
    if (!provider || g_providerCount >= MAX_PROVIDERS) {
        return;
    }
    g_providers[g_providerCount++] = provider;
}

Provider* providerFindByName(const char *name) {
    if (!name) return NULL;

    for (int i = 0; i < g_providerCount; i++) {
        if (g_providers[i]->name && strcmp(g_providers[i]->name, name) == 0) {
            return g_providers[i];
        }
    }
    return NULL;
}

int providerGetRegisteredCount(void) {
    return g_providerCount;
}

Provider* providerGetRegisteredAt(int i) {
    if (i < 0 || i >= g_providerCount) return NULL;
    return g_providers[i];
}

void providerInitAll(void) {
    for (int i = 0; i < g_providerCount; i++) {
        if (g_providers[i]->init) {
            g_providers[i]->init(g_providers[i]);
        }
    }
}

void providerCleanupAll(void) {
    for (int i = 0; i < g_providerCount; i++) {
        if (g_providers[i]->cleanup) {
            g_providers[i]->cleanup(g_providers[i]);
        }
    }
}

// Remove a provider from the global registry (from provider.c)
void providerUnregister(Provider *provider) {
    if (!provider) return;
    for (int i = 0; i < g_providerCount; i++) {
        if (g_providers[i] == provider) {
            for (int j = i; j < g_providerCount - 1; j++) {
                g_providers[j] = g_providers[j + 1];
            }
            g_providers[--g_providerCount] = NULL;
            return;
        }
    }
}

/* ============================================
 * Auth registry (from provider.c)
 * ============================================ */

#define MAX_AUTH_ENTRIES 16

static struct {
    char origin[512];
    char apiKey[512];
} g_authEntries[MAX_AUTH_ENTRIES];
static int g_authCount = 0;

void providerRegisterAuth(const char *origin, const char *apiKey) {
    if (!origin || !apiKey || g_authCount >= MAX_AUTH_ENTRIES) return;
    strncpy(g_authEntries[g_authCount].origin, origin, sizeof(g_authEntries[0].origin) - 1);
    g_authEntries[g_authCount].origin[sizeof(g_authEntries[0].origin) - 1] = '\0';
    strncpy(g_authEntries[g_authCount].apiKey, apiKey, sizeof(g_authEntries[0].apiKey) - 1);
    g_authEntries[g_authCount].apiKey[sizeof(g_authEntries[0].apiKey) - 1] = '\0';
    g_authCount++;
}

static const char* findApiKeyForUrl(const char *url) {
    if (!url) return NULL;
    for (int i = 0; i < g_authCount; i++) {
        if (strncmp(url, g_authEntries[i].origin, strlen(g_authEntries[i].origin)) == 0) {
            return g_authEntries[i].apiKey;
        }
    }
    return NULL;
}

/* ============================================
 * nameMatchesProvider (from programs.c)
 * ============================================ */

static bool nameMatchesProvider(const char *displayName, const char *providerName) {
    if (strcmp(displayName, providerName) == 0) return true;
    const char *d = displayName, *p = providerName;
    while (*d && *p) {
        if (*d == ' ') { d++; continue; }
        if (*d != *p) return false;
        d++; p++;
    }
    while (*d == ' ') d++;
    return *d == '\0' && *p == '\0';
}

/* ============================================
 * Provider dispatch (from provider.c)
 * ============================================ */

Provider* providerGetActive(AppRenderer *appRenderer) {
    if (!appRenderer || appRenderer->currentId.depth < 1) return NULL;
    int rootIndex = appRenderer->currentId.ids[0];
    if (rootIndex < 0 || rootIndex >= appRenderer->ffonCount) return NULL;
    if (!appRenderer->providers) return NULL;
    return appRenderer->providers[rootIndex];
}

const char* providerGetCurrentPath(AppRenderer *appRenderer) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->getCurrentPath) return NULL;
    return provider->getCurrentPath(provider);
}

bool providerCommitEdit(AppRenderer *appRenderer, const char *oldContent, const char *newContent) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->commitEdit) return false;
    return provider->commitEdit(provider, oldContent, newContent);
}

bool providerCreateDirectory(AppRenderer *appRenderer, const char *name) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->createDirectory) return false;
    return provider->createDirectory(provider, name);
}

bool providerCreateFile(AppRenderer *appRenderer, const char *name) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->createFile) return false;
    return provider->createFile(provider, name);
}

bool providerDeleteItem(AppRenderer *appRenderer, const char *name) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->deleteItem) return false;
    return provider->deleteItem(provider, name);
}

const char** providerGetCommands(AppRenderer *appRenderer, int *outCount) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->getCommands) { *outCount = 0; return NULL; }
    return provider->getCommands(provider, outCount);
}

bool providerExecuteCommand(AppRenderer *appRenderer, const char *command, const char *selection) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->executeCommand) return false;
    return provider->executeCommand(provider, command, selection);
}

bool providerNavigateLeft(AppRenderer *appRenderer) {
    if (appRenderer->currentId.depth <= 1) {
        return false;
    }

    // Simplified: skip link check, just pop path and pop id
    Provider *provider = providerGetActive(appRenderer);
    if (provider && provider->popPath) {
        provider->popPath(provider);
    }

    idArrayPop(&appRenderer->currentId);
    return true;
}

/* ============================================
 * Mock provider callbacks (for dispatch tests)
 * ============================================ */

FAKE_VALUE_FUNC(const char*, mock_getCurrentPath, Provider*);
FAKE_VALUE_FUNC(bool, mock_commitEdit, Provider*, const char*, const char*);
FAKE_VALUE_FUNC(bool, mock_createDirectory, Provider*, const char*);
FAKE_VALUE_FUNC(bool, mock_createFile, Provider*, const char*);
FAKE_VALUE_FUNC(bool, mock_deleteItem, Provider*, const char*);
FAKE_VOID_FUNC(mock_init, Provider*);
FAKE_VOID_FUNC(mock_cleanup, Provider*);
FAKE_VOID_FUNC(mock_popPath, Provider*);

/* ============================================
 * Test helpers
 * ============================================ */

static void resetRegistry(void) {
    g_providerCount = 0;
    memset(g_providers, 0, sizeof(g_providers));
}

static Provider createMockProvider(const char *name) {
    Provider p = {0};
    p.name = name;
    return p;
}

static FfonElement* createStringElem(const char *str) {
    FfonElement *e = calloc(1, sizeof(FfonElement));
    e->type = FFON_STRING;
    e->data.string = strdup(str);
    return e;
}

static FfonObject* createFfonObj(const char *key) {
    FfonObject *o = calloc(1, sizeof(FfonObject));
    o->key = strdup(key);
    return o;
}

static FfonElement* createObjectElem(const char *key) {
    FfonElement *e = calloc(1, sizeof(FfonElement));
    e->type = FFON_OBJECT;
    e->data.object = createFfonObj(key);
    return e;
}

static void destroyElem(FfonElement *e) {
    if (!e) return;
    if (e->type == FFON_STRING) free(e->data.string);
    else {
        for (int i = 0; i < e->data.object->count; i++)
            destroyElem(e->data.object->elements[i]);
        free(e->data.object->elements);
        free(e->data.object->key);
        free(e->data.object);
    }
    free(e);
}

/* ============================================
 * Unity Setup/Teardown
 * ============================================ */

void setUp(void) {
    resetRegistry();
    g_authCount = 0;
    memset(g_authEntries, 0, sizeof(g_authEntries));
    RESET_FAKE(setErrorMessage);
    RESET_FAKE(mock_getCurrentPath);
    RESET_FAKE(mock_commitEdit);
    RESET_FAKE(mock_createDirectory);
    RESET_FAKE(mock_createFile);
    RESET_FAKE(mock_deleteItem);
    RESET_FAKE(mock_init);
    RESET_FAKE(mock_cleanup);
    RESET_FAKE(mock_popPath);
    FFF_RESET_HISTORY();
}

void tearDown(void) {}

/* ============================================
 * providerRegister tests
 * ============================================ */

void test_providerRegister_single(void) {
    Provider p = createMockProvider("test");
    providerRegister(&p);
    TEST_ASSERT_EQUAL_INT(1, providerGetRegisteredCount());
}

void test_providerRegister_multiple(void) {
    Provider a = createMockProvider("a");
    Provider b = createMockProvider("b");
    Provider c = createMockProvider("c");
    providerRegister(&a);
    providerRegister(&b);
    providerRegister(&c);
    TEST_ASSERT_EQUAL_INT(3, providerGetRegisteredCount());
}

void test_providerRegister_null(void) {
    providerRegister(NULL);
    TEST_ASSERT_EQUAL_INT(0, providerGetRegisteredCount());
}

void test_providerRegister_max_providers(void) {
    Provider providers[MAX_PROVIDERS + 2];
    for (int i = 0; i < MAX_PROVIDERS + 2; i++) {
        providers[i] = createMockProvider("x");
        providerRegister(&providers[i]);
    }
    TEST_ASSERT_EQUAL_INT(MAX_PROVIDERS, providerGetRegisteredCount());
}

/* ============================================
 * providerFindByName tests
 * ============================================ */

void test_providerFindByName_found(void) {
    Provider a = createMockProvider("alpha");
    Provider b = createMockProvider("beta");
    providerRegister(&a);
    providerRegister(&b);
    TEST_ASSERT_EQUAL_PTR(&b, providerFindByName("beta"));
}

void test_providerFindByName_not_found(void) {
    Provider a = createMockProvider("alpha");
    providerRegister(&a);
    TEST_ASSERT_NULL(providerFindByName("gamma"));
}

void test_providerFindByName_null(void) {
    TEST_ASSERT_NULL(providerFindByName(NULL));
}

void test_providerFindByName_empty_registry(void) {
    TEST_ASSERT_NULL(providerFindByName("test"));
}

/* ============================================
 * providerGetRegisteredAt tests
 * ============================================ */

void test_providerGetRegisteredAt_valid(void) {
    Provider a = createMockProvider("a");
    Provider b = createMockProvider("b");
    providerRegister(&a);
    providerRegister(&b);
    TEST_ASSERT_EQUAL_PTR(&a, providerGetRegisteredAt(0));
    TEST_ASSERT_EQUAL_PTR(&b, providerGetRegisteredAt(1));
}

void test_providerGetRegisteredAt_negative(void) {
    Provider a = createMockProvider("a");
    providerRegister(&a);
    TEST_ASSERT_NULL(providerGetRegisteredAt(-1));
}

void test_providerGetRegisteredAt_out_of_bounds(void) {
    Provider a = createMockProvider("a");
    providerRegister(&a);
    TEST_ASSERT_NULL(providerGetRegisteredAt(5));
}

/* ============================================
 * providerInitAll / providerCleanupAll tests
 * ============================================ */

void test_providerInitAll_calls_init(void) {
    Provider a = createMockProvider("a");
    a.init = mock_init;
    Provider b = createMockProvider("b");
    b.init = mock_init;
    providerRegister(&a);
    providerRegister(&b);
    providerInitAll();
    TEST_ASSERT_EQUAL_INT(2, mock_init_fake.call_count);
}

void test_providerCleanupAll_calls_cleanup(void) {
    Provider a = createMockProvider("a");
    a.cleanup = mock_cleanup;
    providerRegister(&a);
    providerCleanupAll();
    TEST_ASSERT_EQUAL_INT(1, mock_cleanup_fake.call_count);
}

void test_providerInitAll_skips_null_init(void) {
    Provider a = createMockProvider("a");
    a.init = NULL;
    providerRegister(&a);
    providerInitAll(); // Should not crash
}

/* ============================================
 * providerGetActive tests
 * ============================================ */

void test_providerGetActive_null_appRenderer(void) {
    TEST_ASSERT_NULL(providerGetActive(NULL));
}

void test_providerGetActive_no_depth(void) {
    AppRenderer app = {0};
    app.currentId.depth = 0;
    TEST_ASSERT_NULL(providerGetActive(&app));
}

void test_providerGetActive_no_providers_array(void) {
    AppRenderer app = {0};
    app.currentId.depth = 1;
    app.currentId.ids[0] = 0;
    app.ffonCount = 1;
    app.providers = NULL;
    TEST_ASSERT_NULL(providerGetActive(&app));
}

void test_providerGetActive_valid(void) {
    Provider p = createMockProvider("test");
    Provider *providers[] = {&p};
    FfonElement *dummyElem = createStringElem("dummy");
    FfonElement *ffon[] = {dummyElem};

    AppRenderer app = {0};
    app.ffon = ffon;
    app.providers = providers;
    app.ffonCount = 1;
    app.currentId.depth = 1;
    app.currentId.ids[0] = 0;

    TEST_ASSERT_EQUAL_PTR(&p, providerGetActive(&app));
    destroyElem(dummyElem);
}

void test_providerGetActive_out_of_bounds_index(void) {
    AppRenderer app = {0};
    app.ffonCount = 1;
    app.currentId.depth = 1;
    app.currentId.ids[0] = 5;
    Provider *providers[] = {NULL};
    app.providers = providers;
    TEST_ASSERT_NULL(providerGetActive(&app));
}

/* ============================================
 * providerGetCurrentPath dispatch tests
 * ============================================ */

void test_providerGetCurrentPath_dispatches(void) {
    mock_getCurrentPath_fake.return_val = "/home/user";

    Provider p = createMockProvider("fb");
    p.getCurrentPath = mock_getCurrentPath;

    Provider *providers[] = {&p};
    FfonElement *dummyElem = createStringElem("dummy");
    FfonElement *ffon[] = {dummyElem};

    AppRenderer app = {0};
    app.ffon = ffon;
    app.providers = providers;
    app.ffonCount = 1;
    app.currentId.depth = 1;
    app.currentId.ids[0] = 0;

    const char *result = providerGetCurrentPath(&app);
    TEST_ASSERT_EQUAL_STRING("/home/user", result);
    TEST_ASSERT_EQUAL_INT(1, mock_getCurrentPath_fake.call_count);

    destroyElem(dummyElem);
}

void test_providerGetCurrentPath_no_provider(void) {
    AppRenderer app = {0};
    app.currentId.depth = 0;
    TEST_ASSERT_NULL(providerGetCurrentPath(&app));
}

/* ============================================
 * providerCommitEdit dispatch tests
 * ============================================ */

void test_providerCommitEdit_dispatches(void) {
    mock_commitEdit_fake.return_val = true;

    Provider p = createMockProvider("fb");
    p.commitEdit = mock_commitEdit;

    Provider *providers[] = {&p};
    FfonElement *dummyElem = createStringElem("dummy");
    FfonElement *ffon[] = {dummyElem};

    AppRenderer app = {0};
    app.ffon = ffon;
    app.providers = providers;
    app.ffonCount = 1;
    app.currentId.depth = 1;
    app.currentId.ids[0] = 0;

    bool result = providerCommitEdit(&app, "old", "new");
    TEST_ASSERT_TRUE(result);
    TEST_ASSERT_EQUAL_INT(1, mock_commitEdit_fake.call_count);

    destroyElem(dummyElem);
}

void test_providerCommitEdit_no_callback(void) {
    Provider p = createMockProvider("fb");
    p.commitEdit = NULL;

    Provider *providers[] = {&p};
    FfonElement *dummyElem = createStringElem("dummy");
    FfonElement *ffon[] = {dummyElem};

    AppRenderer app = {0};
    app.ffon = ffon;
    app.providers = providers;
    app.ffonCount = 1;
    app.currentId.depth = 1;
    app.currentId.ids[0] = 0;

    TEST_ASSERT_FALSE(providerCommitEdit(&app, "old", "new"));

    destroyElem(dummyElem);
}

/* ============================================
 * providerCreateDirectory dispatch tests
 * ============================================ */

void test_providerCreateDirectory_dispatches(void) {
    mock_createDirectory_fake.return_val = true;

    Provider p = createMockProvider("fb");
    p.createDirectory = mock_createDirectory;

    Provider *providers[] = {&p};
    FfonElement *dummyElem = createStringElem("dummy");
    FfonElement *ffon[] = {dummyElem};

    AppRenderer app = {0};
    app.ffon = ffon;
    app.providers = providers;
    app.ffonCount = 1;
    app.currentId.depth = 1;
    app.currentId.ids[0] = 0;

    TEST_ASSERT_TRUE(providerCreateDirectory(&app, "newdir"));
    TEST_ASSERT_EQUAL_INT(1, mock_createDirectory_fake.call_count);

    destroyElem(dummyElem);
}

/* ============================================
 * providerNavigateLeft tests
 * ============================================ */

void test_providerNavigateLeft_at_root(void) {
    AppRenderer app = {0};
    app.currentId.depth = 1;
    app.currentId.ids[0] = 0;
    TEST_ASSERT_FALSE(providerNavigateLeft(&app));
}

void test_providerNavigateLeft_pops_depth(void) {
    Provider p = createMockProvider("fb");
    p.popPath = mock_popPath;

    Provider *providers[] = {&p};
    FfonElement *dummyElem = createStringElem("dummy");
    FfonElement *ffon[] = {dummyElem};

    AppRenderer app = {0};
    app.ffon = ffon;
    app.providers = providers;
    app.ffonCount = 1;
    app.currentId.depth = 2;
    app.currentId.ids[0] = 0;
    app.currentId.ids[1] = 3;

    TEST_ASSERT_TRUE(providerNavigateLeft(&app));
    TEST_ASSERT_EQUAL_INT(1, app.currentId.depth);
    TEST_ASSERT_EQUAL_INT(1, mock_popPath_fake.call_count);

    destroyElem(dummyElem);
}

void test_providerNavigateLeft_no_popPath(void) {
    Provider p = createMockProvider("fb");
    p.popPath = NULL;

    Provider *providers[] = {&p};
    FfonElement *dummyElem = createStringElem("dummy");
    FfonElement *ffon[] = {dummyElem};

    AppRenderer app = {0};
    app.ffon = ffon;
    app.providers = providers;
    app.ffonCount = 1;
    app.currentId.depth = 2;
    app.currentId.ids[0] = 0;
    app.currentId.ids[1] = 0;

    TEST_ASSERT_TRUE(providerNavigateLeft(&app));
    TEST_ASSERT_EQUAL_INT(1, app.currentId.depth);

    destroyElem(dummyElem);
}

/* ============================================
 * providerUnregister tests
 * ============================================ */

void test_providerUnregister_middle(void) {
    Provider a = createMockProvider("a");
    Provider b = createMockProvider("b");
    Provider c = createMockProvider("c");
    providerRegister(&a);
    providerRegister(&b);
    providerRegister(&c);
    providerUnregister(&b);
    TEST_ASSERT_EQUAL_INT(2, providerGetRegisteredCount());
    TEST_ASSERT_EQUAL_PTR(&a, providerGetRegisteredAt(0));
    TEST_ASSERT_EQUAL_PTR(&c, providerGetRegisteredAt(1));
}

void test_providerUnregister_first(void) {
    Provider a = createMockProvider("a");
    Provider b = createMockProvider("b");
    providerRegister(&a);
    providerRegister(&b);
    providerUnregister(&a);
    TEST_ASSERT_EQUAL_INT(1, providerGetRegisteredCount());
    TEST_ASSERT_EQUAL_PTR(&b, providerGetRegisteredAt(0));
}

void test_providerUnregister_last(void) {
    Provider a = createMockProvider("a");
    Provider b = createMockProvider("b");
    providerRegister(&a);
    providerRegister(&b);
    providerUnregister(&b);
    TEST_ASSERT_EQUAL_INT(1, providerGetRegisteredCount());
    TEST_ASSERT_EQUAL_PTR(&a, providerGetRegisteredAt(0));
}

void test_providerUnregister_null(void) {
    Provider a = createMockProvider("a");
    providerRegister(&a);
    providerUnregister(NULL);
    TEST_ASSERT_EQUAL_INT(1, providerGetRegisteredCount());
}

void test_providerUnregister_not_found(void) {
    Provider a = createMockProvider("a");
    Provider b = createMockProvider("b");
    providerRegister(&a);
    providerUnregister(&b);
    TEST_ASSERT_EQUAL_INT(1, providerGetRegisteredCount());
}

void test_providerUnregister_clears_slot(void) {
    Provider a = createMockProvider("a");
    Provider b = createMockProvider("b");
    providerRegister(&a);
    providerRegister(&b);
    providerUnregister(&b);
    TEST_ASSERT_NULL(providerGetRegisteredAt(1));
}

/* ============================================
 * Auth registry tests
 * ============================================ */

void test_registerAuth_and_find(void) {
    providerRegisterAuth("https://example.com", "secret123");
    const char *key = findApiKeyForUrl("https://example.com/api/data");
    TEST_ASSERT_NOT_NULL(key);
    TEST_ASSERT_EQUAL_STRING("secret123", key);
}

void test_findApiKeyForUrl_no_match(void) {
    providerRegisterAuth("https://example.com", "secret");
    TEST_ASSERT_NULL(findApiKeyForUrl("https://other.com/foo"));
}

void test_findApiKeyForUrl_null(void) {
    TEST_ASSERT_NULL(findApiKeyForUrl(NULL));
}

void test_registerAuth_null_params(void) {
    providerRegisterAuth(NULL, "key");
    providerRegisterAuth("origin", NULL);
    TEST_ASSERT_EQUAL_INT(0, g_authCount);
}

void test_registerAuth_multiple(void) {
    providerRegisterAuth("https://a.com", "key_a");
    providerRegisterAuth("https://b.com", "key_b");
    TEST_ASSERT_EQUAL_STRING("key_a", findApiKeyForUrl("https://a.com/path"));
    TEST_ASSERT_EQUAL_STRING("key_b", findApiKeyForUrl("https://b.com/path"));
}

void test_registerAuth_prefix_match(void) {
    providerRegisterAuth("https://api.example.com", "bearer_token");
    TEST_ASSERT_NOT_NULL(findApiKeyForUrl("https://api.example.com/v1/data"));
    TEST_ASSERT_NULL(findApiKeyForUrl("https://example.com/v1/data"));
}

/* ============================================
 * nameMatchesProvider tests
 * ============================================ */

void test_nameMatchesProvider_exact(void) {
    TEST_ASSERT_TRUE(nameMatchesProvider("tutorial", "tutorial"));
}

void test_nameMatchesProvider_with_spaces(void) {
    TEST_ASSERT_TRUE(nameMatchesProvider("chat client", "chatclient"));
}

void test_nameMatchesProvider_no_match(void) {
    TEST_ASSERT_FALSE(nameMatchesProvider("chat client", "emailclient"));
}

void test_nameMatchesProvider_web_browser(void) {
    TEST_ASSERT_TRUE(nameMatchesProvider("web browser", "webbrowser"));
}

void test_nameMatchesProvider_empty_strings(void) {
    TEST_ASSERT_TRUE(nameMatchesProvider("", ""));
}

void test_nameMatchesProvider_trailing_spaces(void) {
    TEST_ASSERT_TRUE(nameMatchesProvider("chat ", "chat"));
}

/* ============================================
 * Main
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // providerRegister
    RUN_TEST(test_providerRegister_single);
    RUN_TEST(test_providerRegister_multiple);
    RUN_TEST(test_providerRegister_null);
    RUN_TEST(test_providerRegister_max_providers);

    // providerFindByName
    RUN_TEST(test_providerFindByName_found);
    RUN_TEST(test_providerFindByName_not_found);
    RUN_TEST(test_providerFindByName_null);
    RUN_TEST(test_providerFindByName_empty_registry);

    // providerGetRegisteredAt
    RUN_TEST(test_providerGetRegisteredAt_valid);
    RUN_TEST(test_providerGetRegisteredAt_negative);
    RUN_TEST(test_providerGetRegisteredAt_out_of_bounds);

    // providerInitAll / providerCleanupAll
    RUN_TEST(test_providerInitAll_calls_init);
    RUN_TEST(test_providerCleanupAll_calls_cleanup);
    RUN_TEST(test_providerInitAll_skips_null_init);

    // providerGetActive
    RUN_TEST(test_providerGetActive_null_appRenderer);
    RUN_TEST(test_providerGetActive_no_depth);
    RUN_TEST(test_providerGetActive_no_providers_array);
    RUN_TEST(test_providerGetActive_valid);
    RUN_TEST(test_providerGetActive_out_of_bounds_index);

    // providerGetCurrentPath
    RUN_TEST(test_providerGetCurrentPath_dispatches);
    RUN_TEST(test_providerGetCurrentPath_no_provider);

    // providerCommitEdit
    RUN_TEST(test_providerCommitEdit_dispatches);
    RUN_TEST(test_providerCommitEdit_no_callback);

    // providerCreateDirectory
    RUN_TEST(test_providerCreateDirectory_dispatches);

    // providerNavigateLeft
    RUN_TEST(test_providerNavigateLeft_at_root);
    RUN_TEST(test_providerNavigateLeft_pops_depth);
    RUN_TEST(test_providerNavigateLeft_no_popPath);

    // providerUnregister
    RUN_TEST(test_providerUnregister_middle);
    RUN_TEST(test_providerUnregister_first);
    RUN_TEST(test_providerUnregister_last);
    RUN_TEST(test_providerUnregister_null);
    RUN_TEST(test_providerUnregister_not_found);
    RUN_TEST(test_providerUnregister_clears_slot);

    // Auth registry
    RUN_TEST(test_registerAuth_and_find);
    RUN_TEST(test_findApiKeyForUrl_no_match);
    RUN_TEST(test_findApiKeyForUrl_null);
    RUN_TEST(test_registerAuth_null_params);
    RUN_TEST(test_registerAuth_multiple);
    RUN_TEST(test_registerAuth_prefix_match);

    // nameMatchesProvider
    RUN_TEST(test_nameMatchesProvider_exact);
    RUN_TEST(test_nameMatchesProvider_with_spaces);
    RUN_TEST(test_nameMatchesProvider_no_match);
    RUN_TEST(test_nameMatchesProvider_web_browser);
    RUN_TEST(test_nameMatchesProvider_empty_strings);
    RUN_TEST(test_nameMatchesProvider_trailing_spaces);

    return UNITY_END();
}

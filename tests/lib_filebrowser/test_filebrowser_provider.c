/*
 * Tests for filebrowser provider layer.
 * Functions under test: fbGetCommands, fbHandleCommand, fbFetch,
 *                       fbCollectDeepSearchItems, fbGetCommandListItems,
 *                       fbExecuteCommand (all via Provider function pointers)
 */

#include <unity.h>
#include <filebrowser.h>
#include <filebrowser_provider.h>
#include <provider_interface.h>
#include <provider_tags.h>
#include <platform.h>
#include <ffon.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <sys/stat.h>
#include <test_compat.h>

static char tmpDir[256];
static Provider *provider = NULL;

// --- Linker wraps for platform mocking ---

static PlatformApplication g_mockApps[8];
static int g_mockAppCount = 0;
static bool g_openWithCalled = false;
static char g_openWithProgram[256];
static char g_openWithPath[4096];

PlatformApplication* __wrap_platformGetApplications(int *outCount) {
    *outCount = g_mockAppCount;
    if (g_mockAppCount == 0) return NULL;
    PlatformApplication *apps = malloc(g_mockAppCount * sizeof(PlatformApplication));
    for (int i = 0; i < g_mockAppCount; i++) {
        apps[i].name = strdup(g_mockApps[i].name);
        apps[i].exec = strdup(g_mockApps[i].exec);
    }
    return apps;
}

void __wrap_platformFreeApplications(PlatformApplication *apps, int count) {
    if (!apps) return;
    for (int i = 0; i < count; i++) {
        free(apps[i].name);
        free(apps[i].exec);
    }
    free(apps);
}

bool __wrap_platformOpenWith(const char *program, const char *filePath) {
    g_openWithCalled = true;
    snprintf(g_openWithProgram, sizeof(g_openWithProgram), "%s", program);
    snprintf(g_openWithPath, sizeof(g_openWithPath), "%s", filePath);
    return true;
}

// --- Helpers ---

static void createFile(const char *dir, const char *name) {
    char path[512];
    snprintf(path, sizeof(path), "%s/%s", dir, name);
    FILE *fp = fopen(path, "w");
    if (fp) fclose(fp);
}

static void createDir(const char *dir, const char *name) {
    char path[512];
    snprintf(path, sizeof(path), "%s/%s", dir, name);
#ifdef _WIN32
    _mkdir(path);
#else
    mkdir(path, 0755);
#endif
}

static void resetMockState(void) {
    g_mockAppCount = 0;
    g_openWithCalled = false;
    g_openWithProgram[0] = '\0';
    g_openWithPath[0] = '\0';
}

void setUp(void) {
#ifdef _WIN32
    snprintf(tmpDir, sizeof(tmpDir), "%s\\sicompass_fbprov_test",
             getenv("TEMP") ? getenv("TEMP") : "C:\\Temp");
    _mkdir(tmpDir);
#else
    snprintf(tmpDir, sizeof(tmpDir), "/tmp/sicompass_fbprov_test_XXXXXX");
    mkdtemp(tmpDir);
#endif

    provider = filebrowserGetProvider();
    provider->setCurrentPath(provider, tmpDir);

    // Reset global state to known defaults via commands
    char err[256] = {0};
    provider->handleCommand(provider, "sort alphanumerically", NULL, 0, err, sizeof(err));

    resetMockState();
}

void tearDown(void) {
    char cmd[512];
#ifdef _WIN32
    snprintf(cmd, sizeof(cmd), "rmdir /s /q \"%s\"", tmpDir);
#else
    snprintf(cmd, sizeof(cmd), "rm -rf %s", tmpDir);
#endif
    system(cmd);
}

// --- getCommands ---

void test_getCommands_returns_six(void) {
    int count = 0;
    const char **cmds = provider->getCommands(provider, &count);
    TEST_ASSERT_EQUAL_INT(6, count);
    TEST_ASSERT_NOT_NULL(cmds);

    // Verify all expected commands are present
    bool found[6] = {false};
    const char *expected[] = {
        "create directory", "create file", "open file with",
        "show/hide properties", "sort alphanumerically", "sort chronologically"
    };
    for (int i = 0; i < count; i++) {
        for (int j = 0; j < 6; j++) {
            if (strcmp(cmds[i], expected[j]) == 0) found[j] = true;
        }
    }
    for (int j = 0; j < 6; j++) {
        TEST_ASSERT_TRUE_MESSAGE(found[j], expected[j]);
    }
}

// --- handleCommand ---

void test_handleCommand_create_directory(void) {
    char err[256] = {0};
    FfonElement *elem = provider->handleCommand(provider, "create directory",
                                                 NULL, 0, err, sizeof(err));
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, elem->type);
    TEST_ASSERT_TRUE(providerTagHasInput(elem->data.object->key));

    // Should have a STRING child with <input></input>
    TEST_ASSERT_NOT_NULL(elem->data.object->elements);
    TEST_ASSERT_EQUAL_INT(1, elem->data.object->count);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, elem->data.object->elements[0]->type);
    TEST_ASSERT_TRUE(providerTagHasInput(elem->data.object->elements[0]->data.string));

    ffonElementDestroy(elem);
}

void test_handleCommand_create_file(void) {
    char err[256] = {0};
    FfonElement *elem = provider->handleCommand(provider, "create file",
                                                 NULL, 0, err, sizeof(err));
    TEST_ASSERT_NOT_NULL(elem);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, elem->type);
    TEST_ASSERT_TRUE(providerTagHasInput(elem->data.string));

    ffonElementDestroy(elem);
}

void test_handleCommand_show_hide_properties_toggle(void) {
    // Create a test file
    createFile(tmpDir, "testfile.txt");

    // Fetch without properties (default state after setUp reset)
    int count = 0;
    FfonElement **elems = provider->fetch(provider, &count);
    TEST_ASSERT_EQUAL_INT(1, count);
    char *contentBefore = NULL;
    if (elems[0]->type == FFON_STRING)
        contentBefore = strdup(elems[0]->data.string);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    // Toggle properties on
    char err[256] = {0};
    FfonElement *result = provider->handleCommand(provider, "show/hide properties",
                                                   NULL, 0, err, sizeof(err));
    TEST_ASSERT_NULL(result);  // returns NULL for state-change commands

    // Fetch with properties
    elems = provider->fetch(provider, &count);
    TEST_ASSERT_EQUAL_INT(1, count);
    char *contentAfter = NULL;
    if (elems[0]->type == FFON_STRING)
        contentAfter = strdup(elems[0]->data.string);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    // Content should differ (properties prefix added)
    TEST_ASSERT_NOT_NULL(contentBefore);
    TEST_ASSERT_NOT_NULL(contentAfter);
    TEST_ASSERT_TRUE(strlen(contentAfter) > strlen(contentBefore));

    free(contentBefore);
    free(contentAfter);

    // Toggle back off for cleanup
    provider->handleCommand(provider, "show/hide properties", NULL, 0, err, sizeof(err));
}

void test_handleCommand_sort_chrono(void) {
    // Create files with staggered mtimes
    createFile(tmpDir, "oldest.txt");
    createFile(tmpDir, "newest.txt");
    createFile(tmpDir, "middle.txt");

    // Set distinct modification times
    char path[512];
    struct timeval times[2];

    snprintf(path, sizeof(path), "%s/oldest.txt", tmpDir);
    times[0].tv_sec = 1000000; times[0].tv_usec = 0;
    times[1].tv_sec = 1000000; times[1].tv_usec = 0;
    utimes(path, times);

    snprintf(path, sizeof(path), "%s/middle.txt", tmpDir);
    times[0].tv_sec = 2000000; times[0].tv_usec = 0;
    times[1].tv_sec = 2000000; times[1].tv_usec = 0;
    utimes(path, times);

    snprintf(path, sizeof(path), "%s/newest.txt", tmpDir);
    times[0].tv_sec = 3000000; times[0].tv_usec = 0;
    times[1].tv_sec = 3000000; times[1].tv_usec = 0;
    utimes(path, times);

    // Switch to chronological sort
    char err[256] = {0};
    provider->handleCommand(provider, "sort chronologically", NULL, 0, err, sizeof(err));

    int count = 0;
    FfonElement **elems = provider->fetch(provider, &count);
    TEST_ASSERT_EQUAL_INT(3, count);

    // Chrono sort: newest first
    char *name0 = providerTagExtractContent(elems[0]->data.string);
    char *name1 = providerTagExtractContent(elems[1]->data.string);
    char *name2 = providerTagExtractContent(elems[2]->data.string);
    TEST_ASSERT_EQUAL_STRING("newest.txt", name0);
    TEST_ASSERT_EQUAL_STRING("middle.txt", name1);
    TEST_ASSERT_EQUAL_STRING("oldest.txt", name2);
    free(name0); free(name1); free(name2);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    // Reset sort mode
    provider->handleCommand(provider, "sort alphanumerically", NULL, 0, err, sizeof(err));
}

void test_handleCommand_sort_alpha(void) {
    createFile(tmpDir, "cherry.txt");
    createFile(tmpDir, "apple.txt");
    createFile(tmpDir, "banana.txt");

    // Ensure alphabetical sort (should be default after setUp)
    char err[256] = {0};
    provider->handleCommand(provider, "sort alphanumerically", NULL, 0, err, sizeof(err));

    int count = 0;
    FfonElement **elems = provider->fetch(provider, &count);
    TEST_ASSERT_EQUAL_INT(3, count);

    char *name0 = providerTagExtractContent(elems[0]->data.string);
    char *name1 = providerTagExtractContent(elems[1]->data.string);
    char *name2 = providerTagExtractContent(elems[2]->data.string);
    TEST_ASSERT_EQUAL_STRING("apple.txt", name0);
    TEST_ASSERT_EQUAL_STRING("banana.txt", name1);
    TEST_ASSERT_EQUAL_STRING("cherry.txt", name2);
    free(name0); free(name1); free(name2);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_handleCommand_open_with_directory_error(void) {
    char err[256] = {0};
    FfonElement *elem = provider->handleCommand(provider, "open file with",
                                                 "<input>somedir</input>",
                                                 FFON_OBJECT, err, sizeof(err));
    TEST_ASSERT_NULL(elem);
    TEST_ASSERT_TRUE(strlen(err) > 0);
    TEST_ASSERT_NOT_NULL(strstr(err, "select a file, not a directory"));
}

void test_handleCommand_open_with_file(void) {
    char err[256] = {0};
    FfonElement *elem = provider->handleCommand(provider, "open file with",
                                                 "<input>test.txt</input>",
                                                 FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(elem);
    TEST_ASSERT_EQUAL_STRING("", err);
}

void test_handleCommand_unknown(void) {
    char err[256] = {0};
    FfonElement *elem = provider->handleCommand(provider, "nonexistent command",
                                                 NULL, 0, err, sizeof(err));
    TEST_ASSERT_NULL(elem);
}

// --- Deep search ---

void test_deepSearch_empty_dir(void) {
    int count = 0;
    SearchResultItem *items = provider->collectDeepSearchItems(provider, &count);
    TEST_ASSERT_EQUAL_INT(0, count);
    if (items) free(items);
}

void test_deepSearch_flat_files(void) {
    createFile(tmpDir, "alpha.txt");
    createFile(tmpDir, "beta.txt");
    createFile(tmpDir, "gamma.txt");

    int count = 0;
    SearchResultItem *items = provider->collectDeepSearchItems(provider, &count);
    TEST_ASSERT_EQUAL_INT(3, count);
    TEST_ASSERT_NOT_NULL(items);

    // All should have "- " prefix (files, not dirs) and empty breadcrumb
    for (int i = 0; i < count; i++) {
        TEST_ASSERT_TRUE(items[i].label[0] == '-');
        TEST_ASSERT_EQUAL_STRING("", items[i].breadcrumb);
        TEST_ASSERT_NOT_NULL(strstr(items[i].navPath, tmpDir));
        free(items[i].label);
        free(items[i].breadcrumb);
        free(items[i].navPath);
    }
    free(items);
}

void test_deepSearch_nested_dirs(void) {
    // Create a/b/c.txt
    createDir(tmpDir, "a");
    char subA[512], subB[512];
    snprintf(subA, sizeof(subA), "%s/a", tmpDir);
    createDir(subA, "b");
    snprintf(subB, sizeof(subB), "%s/a/b", tmpDir);
    createFile(subB, "c.txt");

    int count = 0;
    SearchResultItem *items = provider->collectDeepSearchItems(provider, &count);
    TEST_ASSERT_EQUAL_INT(3, count);  // a, b, c.txt

    // BFS order: a first (depth 1), then b (depth 2), then c.txt (depth 3)
    // Find items by label
    int idxA = -1, idxB = -1, idxC = -1;
    for (int i = 0; i < count; i++) {
        if (strstr(items[i].label, " a")) idxA = i;
        else if (strstr(items[i].label, " b")) idxB = i;
        else if (strstr(items[i].label, " c.txt")) idxC = i;
    }
    TEST_ASSERT_NOT_EQUAL(-1, idxA);
    TEST_ASSERT_NOT_EQUAL(-1, idxB);
    TEST_ASSERT_NOT_EQUAL(-1, idxC);

    // BFS: a before b before c.txt
    TEST_ASSERT_TRUE(idxA < idxB);
    TEST_ASSERT_TRUE(idxB < idxC);

    // Verify breadcrumbs
    TEST_ASSERT_EQUAL_STRING("", items[idxA].breadcrumb);
    TEST_ASSERT_EQUAL_STRING("a > ", items[idxB].breadcrumb);
    TEST_ASSERT_EQUAL_STRING("a > b > ", items[idxC].breadcrumb);

    // Verify directory prefix "+" vs file prefix "-"
    TEST_ASSERT_EQUAL_CHAR('+', items[idxA].label[0]);
    TEST_ASSERT_EQUAL_CHAR('+', items[idxB].label[0]);
    TEST_ASSERT_EQUAL_CHAR('-', items[idxC].label[0]);

    for (int i = 0; i < count; i++) {
        free(items[i].label);
        free(items[i].breadcrumb);
        free(items[i].navPath);
    }
    free(items);
}

void test_deepSearch_symlink_not_followed(void) {
#ifndef _WIN32
    // Create a circular symlink: tmpDir/loop -> tmpDir
    char linkPath[512];
    snprintf(linkPath, sizeof(linkPath), "%s/loop", tmpDir);
    symlink(tmpDir, linkPath);

    // Also create a regular file so we know search works
    createFile(tmpDir, "regular.txt");

    int count = 0;
    SearchResultItem *items = provider->collectDeepSearchItems(provider, &count);
    TEST_ASSERT_EQUAL_INT(2, count);  // loop (as file via lstat) + regular.txt

    // Verify the symlink is not traversed as a directory (no infinite loop)
    bool foundLoop = false;
    for (int i = 0; i < count; i++) {
        if (strstr(items[i].label, "loop")) {
            foundLoop = true;
            TEST_ASSERT_EQUAL_CHAR('-', items[i].label[0]);  // file, not dir
        }
    }
    TEST_ASSERT_TRUE(foundLoop);

    for (int i = 0; i < count; i++) {
        free(items[i].label);
        free(items[i].breadcrumb);
        free(items[i].navPath);
    }
    free(items);
#endif /* _WIN32 */
}

// --- Provider wrappers (fetch, commit, create, delete) ---

void test_provider_commit_renames(void) {
    createFile(tmpDir, "old.txt");
    bool result = provider->commitEdit(provider, "old.txt", "new.txt");
    TEST_ASSERT_TRUE(result);

    char path[512];
    struct stat st;
    snprintf(path, sizeof(path), "%s/old.txt", tmpDir);
    TEST_ASSERT_NOT_EQUAL(0, stat(path, &st));
    snprintf(path, sizeof(path), "%s/new.txt", tmpDir);
    TEST_ASSERT_EQUAL(0, stat(path, &st));
}

void test_provider_createDirectory(void) {
    bool result = provider->createDirectory(provider, "newdir");
    TEST_ASSERT_TRUE(result);

    char path[512];
    struct stat st;
    snprintf(path, sizeof(path), "%s/newdir", tmpDir);
    TEST_ASSERT_EQUAL(0, stat(path, &st));
    TEST_ASSERT_TRUE(S_ISDIR(st.st_mode));
}

void test_provider_createFile(void) {
    bool result = provider->createFile(provider, "newfile.txt");
    TEST_ASSERT_TRUE(result);

    char path[512];
    struct stat st;
    snprintf(path, sizeof(path), "%s/newfile.txt", tmpDir);
    TEST_ASSERT_EQUAL(0, stat(path, &st));
}

void test_provider_deleteItem(void) {
    createFile(tmpDir, "todelete.txt");
    bool result = provider->deleteItem(provider, "todelete.txt");
    TEST_ASSERT_TRUE(result);

    char path[512];
    struct stat st;
    snprintf(path, sizeof(path), "%s/todelete.txt", tmpDir);
    TEST_ASSERT_NOT_EQUAL(0, stat(path, &st));
}

// --- getCommandListItems / executeCommand (mocked platform) ---

void test_getCommandListItems_non_open_with(void) {
    int count = 99;
    ProviderListItem *items = provider->getCommandListItems(provider, "create directory", &count);
    TEST_ASSERT_NULL(items);
    TEST_ASSERT_EQUAL_INT(0, count);
}

void test_getCommandListItems_open_with(void) {
#ifndef _WIN32
    g_mockAppCount = 2;
    g_mockApps[0].name = "Firefox";
    g_mockApps[0].exec = "firefox";
    g_mockApps[1].name = "VLC";
    g_mockApps[1].exec = "vlc";

    int count = 0;
    ProviderListItem *items = provider->getCommandListItems(provider, "open file with", &count);
    TEST_ASSERT_EQUAL_INT(2, count);
    TEST_ASSERT_NOT_NULL(items);
    TEST_ASSERT_EQUAL_STRING("Firefox", items[0].label);
    TEST_ASSERT_EQUAL_STRING("firefox", items[0].data);
    TEST_ASSERT_EQUAL_STRING("VLC", items[1].label);
    TEST_ASSERT_EQUAL_STRING("vlc", items[1].data);

    providerFreeCommandListItems(items, count);
#endif
}

void test_getCommandListItems_open_with_no_apps(void) {
#ifndef _WIN32
    g_mockAppCount = 0;

    int count = 99;
    ProviderListItem *items = provider->getCommandListItems(provider, "open file with", &count);
    TEST_ASSERT_NULL(items);
    TEST_ASSERT_EQUAL_INT(0, count);
#endif
}

void test_executeCommand_open_with(void) {
#ifndef _WIN32
    // First store a path via handleCommand
    char err[256] = {0};
    provider->handleCommand(provider, "open file with",
                             "<input>test.txt</input>", FFON_STRING,
                             err, sizeof(err));

    // Now execute
    bool result = provider->executeCommand(provider, "open file with", "firefox");
    TEST_ASSERT_TRUE(result);
    TEST_ASSERT_TRUE(g_openWithCalled);
    TEST_ASSERT_EQUAL_STRING("firefox", g_openWithProgram);
    // Path should contain tmpDir/test.txt
    TEST_ASSERT_NOT_NULL(strstr(g_openWithPath, tmpDir));
    TEST_ASSERT_NOT_NULL(strstr(g_openWithPath, "test.txt"));
#endif
}

void test_executeCommand_unknown(void) {
    bool result = provider->executeCommand(provider, "nonexistent", "anything");
    TEST_ASSERT_FALSE(result);
}

// --- Windows drive navigation ---

void test_provider_init_starts_at_drive_list(void) {
#ifdef _WIN32
    provider->init(provider);
    const char *path = provider->getCurrentPath(provider);
    TEST_ASSERT_EQUAL_STRING("/", path);
#endif
}

void test_provider_pushPath_from_drive_list(void) {
#ifdef _WIN32
    provider->setCurrentPath(provider, "/");
    provider->pushPath(provider, "C:\\");
    const char *path = provider->getCurrentPath(provider);
    TEST_ASSERT_EQUAL_STRING("C:\\", path);
#endif
}

void test_provider_popPath_from_drive_root(void) {
#ifdef _WIN32
    provider->setCurrentPath(provider, "C:\\");
    provider->popPath(provider);
    const char *path = provider->getCurrentPath(provider);
    TEST_ASSERT_EQUAL_STRING("/", path);
#endif
}

void test_provider_popPath_from_drive_list_no_change(void) {
#ifdef _WIN32
    provider->setCurrentPath(provider, "/");
    provider->popPath(provider);
    const char *path = provider->getCurrentPath(provider);
    TEST_ASSERT_EQUAL_STRING("/", path);
#endif
}

void test_provider_fetch_drive_list_on_windows(void) {
#ifdef _WIN32
    provider->setCurrentPath(provider, "/");
    int count = 0;
    FfonElement **elems = provider->fetch(provider, &count);
    TEST_ASSERT_GREATER_THAN(0, count);
    TEST_ASSERT_NOT_NULL(elems);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
#endif
}

// --- main ---

int main(void) {
    UNITY_BEGIN();

    // Commands
    RUN_TEST(test_getCommands_returns_six);
    RUN_TEST(test_handleCommand_create_directory);
    RUN_TEST(test_handleCommand_create_file);
    RUN_TEST(test_handleCommand_show_hide_properties_toggle);
    RUN_TEST(test_handleCommand_sort_chrono);
    RUN_TEST(test_handleCommand_sort_alpha);
    RUN_TEST(test_handleCommand_open_with_directory_error);
    RUN_TEST(test_handleCommand_open_with_file);
    RUN_TEST(test_handleCommand_unknown);

    // Deep search
    RUN_TEST(test_deepSearch_empty_dir);
    RUN_TEST(test_deepSearch_flat_files);
    RUN_TEST(test_deepSearch_nested_dirs);
    RUN_TEST(test_deepSearch_symlink_not_followed);

    // Provider wrappers
    RUN_TEST(test_provider_commit_renames);
    RUN_TEST(test_provider_createDirectory);
    RUN_TEST(test_provider_createFile);
    RUN_TEST(test_provider_deleteItem);

    // Platform mocks
    RUN_TEST(test_getCommandListItems_non_open_with);
    RUN_TEST(test_getCommandListItems_open_with);
    RUN_TEST(test_getCommandListItems_open_with_no_apps);
    RUN_TEST(test_executeCommand_open_with);
    RUN_TEST(test_executeCommand_unknown);

    // Windows drive navigation
    RUN_TEST(test_provider_init_starts_at_drive_list);
    RUN_TEST(test_provider_pushPath_from_drive_list);
    RUN_TEST(test_provider_popPath_from_drive_root);
    RUN_TEST(test_provider_popPath_from_drive_list_no_change);
    RUN_TEST(test_provider_fetch_drive_list_on_windows);

    return UNITY_END();
}

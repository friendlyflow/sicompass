/*
 * Integration tests for sicompass.
 *
 * These tests link real handler/event/update/list/provider code and real
 * provider libraries (filebrowser, settings) against a headless harness
 * that mocks only SDL windowing, Vulkan rendering, and accessibility.
 *
 * Key presses are simulated via constructed SDL_Event structs.
 */

#include <unity.h>
#include "harness.h"
#include <provider_interface.h>
#include <provider_tags.h>
#include <ffon.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <sys/stat.h>
#include <test_compat.h>

static char tmpDir[256];
static AppRenderer *app = NULL;

// --- Temp directory helpers ---

static void createFile(const char *dir, const char *name) {
    char path[512];
    snprintf(path, sizeof(path), "%s/%s", dir, name);
    FILE *fp = fopen(path, "w");
    if (fp) { fprintf(fp, "test content"); fclose(fp); }
}

static void createDir(const char *dir, const char *name) {
    char path[512];
    snprintf(path, sizeof(path), "%s/%s", dir, name);
    mkdir(path, 0755);
}

static bool fileExists(const char *dir, const char *name) {
    char path[512];
    snprintf(path, sizeof(path), "%s/%s", dir, name);
    return access(path, F_OK) == 0;
}

static void rmrf(const char *path) {
    char cmd[600];
#ifdef _WIN32
    snprintf(cmd, sizeof(cmd), "rd /s /q \"%s\"", path);
#else
    snprintf(cmd, sizeof(cmd), "rm -rf '%s'", path);
#endif
    system(cmd);
}

// --- setUp / tearDown ---

void setUp(void) {
#ifdef _WIN32
    snprintf(tmpDir, sizeof(tmpDir), "%s\\sicompass_integration",
             getenv("TEMP") ? getenv("TEMP") : "C:\\Temp");
    char *result = (_mkdir(tmpDir) == 0) ? tmpDir : NULL;
#else
    snprintf(tmpDir, sizeof(tmpDir), "/tmp/sicompass_integration_XXXXXX");
    char *result = mkdtemp(tmpDir);
#endif
    TEST_ASSERT_NOT_NULL_MESSAGE(result, "Failed to create temp directory");

    // Pre-populate with test files and directories
    createFile(tmpDir, "alpha.txt");
    createFile(tmpDir, "beta.txt");
    createDir(tmpDir, "subdir");
    createFile(tmpDir, "subdir/nested.txt");

    app = harnessCreateAppRenderer();
    TEST_ASSERT_NOT_NULL(app);

    bool ok = harnessSetupProviders(app, tmpDir);
    TEST_ASSERT_TRUE_MESSAGE(ok, "Failed to set up providers");
}

void tearDown(void) {
    if (app) {
        harnessDestroyAppRenderer(app);
        app = NULL;
    }
    providerCleanupAll();
    // Unregister all providers from global registry to prevent accumulation
    while (providerGetRegisteredCount() > 0) {
        Provider *p = providerGetRegisteredAt(0);
        providerUnregister(p);
    }
    rmrf(tmpDir);
}

// ============================================================
// Test: Initial state is OPERATOR_GENERAL at root
// ============================================================

void test_initial_state(void) {
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);
    TEST_ASSERT_EQUAL(1, app->currentId.depth);
    TEST_ASSERT_EQUAL(0, app->currentId.ids[0]);
    TEST_ASSERT_TRUE(app->ffonCount >= 2);  // file browser + settings
}

// ============================================================
// Test: Navigate between providers with up/down
// ============================================================

void test_navigate_between_providers(void) {
    int startIdx = app->currentId.ids[0];

    // Navigate down to second provider
    pressDown(app);
    TEST_ASSERT_EQUAL(startIdx + 1, app->currentId.ids[0]);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);

    // Navigate back up
    pressUp(app);
    TEST_ASSERT_EQUAL(startIdx, app->currentId.ids[0]);
}

// ============================================================
// Test: Enter provider and navigate back
// ============================================================

void test_enter_provider_and_navigate_back(void) {
    // Find the file browser provider index
    int fbIdx = -1;
    for (int i = 0; i < app->ffonCount; i++) {
        if (strcmp(app->providers[i]->name, "filebrowser") == 0) {
            fbIdx = i;
            break;
        }
    }
    TEST_ASSERT_NOT_EQUAL(-1, fbIdx);

    // Navigate to file browser
    while (app->currentId.ids[0] != fbIdx) {
        pressDown(app);
    }

    // Enter the file browser (depth 1 -> 2)
    pressRight(app);
    TEST_ASSERT_EQUAL(2, app->currentId.depth);

    // Should still be in operator general
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);

    // Navigate back to root
    pressLeft(app);
    TEST_ASSERT_EQUAL(1, app->currentId.depth);
}

// ============================================================
// Test: File browser shows files from temp directory
// ============================================================

void test_filebrowser_shows_temp_files(void) {
    // Find and enter file browser
    int fbIdx = -1;
    for (int i = 0; i < app->ffonCount; i++) {
        if (strcmp(app->providers[i]->name, "filebrowser") == 0) {
            fbIdx = i;
            break;
        }
    }
    TEST_ASSERT_NOT_EQUAL(-1, fbIdx);

    while (app->currentId.ids[0] != fbIdx) pressDown(app);
    pressRight(app);

    // The file browser root element should have children (our test files)
    FfonElement *fbRoot = app->ffon[fbIdx];
    TEST_ASSERT_NOT_NULL(fbRoot);
    TEST_ASSERT_EQUAL(FFON_OBJECT, fbRoot->type);

    // Check that children exist (alpha.txt, beta.txt, subdir)
    FfonObject *obj = fbRoot->data.object;
    TEST_ASSERT_NOT_NULL(obj);
    TEST_ASSERT_TRUE_MESSAGE(obj->count >= 3,
        "Expected at least 3 items (alpha.txt, beta.txt, subdir) in file browser");
}

// ============================================================
// Test: Search mode via Tab
// ============================================================

void test_search_mode_tab(void) {
    // Press Tab to enter search mode
    pressTab(app);
    TEST_ASSERT_EQUAL(COORDINATE_SIMPLE_SEARCH, app->currentCoordinate);

    // Type a search query
    typeText(app, "alpha");

    // Filtered list should be smaller than total
    TEST_ASSERT_TRUE(app->filteredListCount <= app->totalListCount);

    // Escape back to operator mode
    pressEscape(app);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);
}

// ============================================================
// Test: Provider active context changes with navigation
// ============================================================

void test_provider_active_changes_with_navigation(void) {
    Provider *first = providerGetActive(app);
    TEST_ASSERT_NOT_NULL(first);

    // Move down one provider
    pressDown(app);

    Provider *second = providerGetActive(app);
    TEST_ASSERT_NOT_NULL(second);

    // Adjacent providers should be different
    TEST_ASSERT_NOT_EQUAL(first, second);

    // Move back up
    pressUp(app);
    Provider *backToFirst = providerGetActive(app);
    TEST_ASSERT_EQUAL(first, backToFirst);
}

// ============================================================
// Test: Navigate into file browser subdirectory
// ============================================================

void test_navigate_into_subdirectory(void) {
    // Find and enter file browser
    int fbIdx = -1;
    for (int i = 0; i < app->ffonCount; i++) {
        if (strcmp(app->providers[i]->name, "filebrowser") == 0) {
            fbIdx = i;
            break;
        }
    }
    TEST_ASSERT_NOT_EQUAL(-1, fbIdx);

    while (app->currentId.ids[0] != fbIdx) pressDown(app);
    pressRight(app);  // enter file browser

    // Find and navigate to "subdir" entry
    FfonObject *obj = app->ffon[fbIdx]->data.object;
    int subdirIdx = -1;
    for (int i = 0; i < obj->count; i++) {
        if (obj->elements[i]->type == FFON_OBJECT &&
            strcmp(obj->elements[i]->data.object->key, "subdir") == 0) {
            subdirIdx = i;
            break;
        }
    }

    if (subdirIdx >= 0) {
        // Navigate to subdir
        while (app->currentId.ids[1] != subdirIdx) {
            pressDown(app);
        }

        // Enter the subdirectory
        pressRight(app);
        TEST_ASSERT_EQUAL(3, app->currentId.depth);

        // Go back
        pressLeft(app);
        TEST_ASSERT_EQUAL(2, app->currentId.depth);
    }
}

// ============================================================
// Test: Provider state is preserved across navigation
// ============================================================

void test_provider_state_preserved(void) {
    // Find file browser index
    int fbIdx = -1;
    for (int i = 0; i < app->ffonCount; i++) {
        if (strcmp(app->providers[i]->name, "filebrowser") == 0) {
            fbIdx = i;
            break;
        }
    }
    TEST_ASSERT_NOT_EQUAL(-1, fbIdx);

    // Enter file browser and count children
    while (app->currentId.ids[0] != fbIdx) pressDown(app);
    pressRight(app);

    FfonObject *obj = app->ffon[fbIdx]->data.object;
    int childCountBefore = obj->count;
    TEST_ASSERT_TRUE(childCountBefore >= 3);  // alpha.txt, beta.txt, subdir

    // Navigate back to root
    pressLeft(app);

    // Navigate to a different provider
    pressDown(app);

    // Come back to file browser
    pressUp(app);

    // The ffon tree for this provider should still have the same children
    FfonObject *objAfter = app->ffon[fbIdx]->data.object;
    TEST_ASSERT_EQUAL(childCountBefore, objAfter->count);

    // Re-entering should work and show the same content
    pressRight(app);
    TEST_ASSERT_EQUAL(2, app->currentId.depth);
    TEST_ASSERT_EQUAL(fbIdx, app->currentId.ids[0]);
}

// ============================================================
// Test: File creation via command mode
// ============================================================

void test_file_creation_via_command(void) {
    // Find and enter file browser
    int fbIdx = -1;
    for (int i = 0; i < app->ffonCount; i++) {
        if (strcmp(app->providers[i]->name, "filebrowser") == 0) {
            fbIdx = i;
            break;
        }
    }
    TEST_ASSERT_NOT_EQUAL(-1, fbIdx);

    while (app->currentId.ids[0] != fbIdx) pressDown(app);
    pressRight(app);

    // Enter insert mode with Ctrl+I (creates new item)
    pressCtrl(app, SDLK_I);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_INSERT, app->currentCoordinate);

    // Type "- newfile.txt" (dash prefix = file)
    typeText(app, "- newfile.txt");

    // Press Enter to commit
    pressEnter(app);

    // Should be back in operator mode
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);

    // Verify the file was created on disk
    TEST_ASSERT_TRUE_MESSAGE(fileExists(tmpDir, "newfile.txt"),
        "Expected newfile.txt to be created in temp directory");
}

// ============================================================
// Test: Directory creation via command mode
// ============================================================

void test_directory_creation_via_command(void) {
    int fbIdx = -1;
    for (int i = 0; i < app->ffonCount; i++) {
        if (strcmp(app->providers[i]->name, "filebrowser") == 0) {
            fbIdx = i;
            break;
        }
    }
    TEST_ASSERT_NOT_EQUAL(-1, fbIdx);

    while (app->currentId.ids[0] != fbIdx) pressDown(app);
    pressRight(app);

    // Enter insert mode
    pressCtrl(app, SDLK_I);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_INSERT, app->currentCoordinate);

    // Type "+ newdir" (plus prefix = directory)
    typeText(app, "+ newdir");

    // Press Enter to commit
    pressEnter(app);

    // Verify the directory was created on disk
    char dirPath[512];
    snprintf(dirPath, sizeof(dirPath), "%s/newdir", tmpDir);
    struct stat st;
    TEST_ASSERT_EQUAL_MESSAGE(0, stat(dirPath, &st), "Expected newdir to exist");
    TEST_ASSERT_TRUE_MESSAGE(S_ISDIR(st.st_mode), "Expected newdir to be a directory");
}

// ============================================================
// Test: Escape from various modes returns to operator
// ============================================================

void test_escape_returns_to_operator(void) {
    // From search mode
    pressTab(app);
    TEST_ASSERT_EQUAL(COORDINATE_SIMPLE_SEARCH, app->currentCoordinate);
    pressEscape(app);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);

    // From insert mode (at root level where Ctrl+I enters operator insert)
    int fbIdx = -1;
    for (int i = 0; i < app->ffonCount; i++) {
        if (strcmp(app->providers[i]->name, "filebrowser") == 0) {
            fbIdx = i;
            break;
        }
    }
    if (fbIdx >= 0) {
        while (app->currentId.ids[0] != fbIdx) pressDown(app);
        pressRight(app);
        pressCtrl(app, SDLK_I);
        TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_INSERT, app->currentCoordinate);
        pressEscape(app);
        TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);
    }
}

// ============================================================
// Test: File deletion
// ============================================================

void test_file_deletion(void) {
    // Create a file to delete
    createFile(tmpDir, "deleteme.txt");
    TEST_ASSERT_TRUE(fileExists(tmpDir, "deleteme.txt"));

    // Find and enter file browser
    int fbIdx = -1;
    for (int i = 0; i < app->ffonCount; i++) {
        if (strcmp(app->providers[i]->name, "filebrowser") == 0) {
            fbIdx = i;
            break;
        }
    }
    TEST_ASSERT_NOT_EQUAL(-1, fbIdx);

    while (app->currentId.ids[0] != fbIdx) pressDown(app);
    pressRight(app);

    // Refresh to see the new file
    pressKey(app, SDLK_F5, 0);

    // Find "deleteme.txt" in the listing
    FfonObject *obj = app->ffon[fbIdx]->data.object;
    int deleteIdx = -1;
    for (int i = 0; i < obj->count; i++) {
        const char *name = NULL;
        if (obj->elements[i]->type == FFON_STRING)
            name = obj->elements[i]->data.string;
        else if (obj->elements[i]->type == FFON_OBJECT)
            name = obj->elements[i]->data.object->key;
        if (name && strcmp(name, "deleteme.txt") == 0) {
            deleteIdx = i;
            break;
        }
    }

    if (deleteIdx >= 0) {
        // Navigate to it
        while (app->currentId.ids[1] != deleteIdx) {
            pressDown(app);
        }

        // Delete with Ctrl+D (handleDelete)
        handleFileDelete(app);

        // Verify the file is gone
        TEST_ASSERT_FALSE_MESSAGE(fileExists(tmpDir, "deleteme.txt"),
            "Expected deleteme.txt to be deleted");
    }
}

// ============================================================
// Test: Esc from scroll search chains back to operator
// ============================================================

void test_scroll_search_esc_chain(void) {
    // Navigate into a provider so Tab enters scroll mode
    pressTab(app);
    pressTab(app);
    TEST_ASSERT_EQUAL(COORDINATE_SCROLL, app->currentCoordinate);

    // Ctrl+F enters scroll search
    pressCtrl(app, SDLK_F);
    TEST_ASSERT_EQUAL(COORDINATE_SCROLL_SEARCH, app->currentCoordinate);

    // Esc -> scroll
    pressEscape(app);
    TEST_ASSERT_EQUAL(COORDINATE_SCROLL, app->currentCoordinate);

    // Esc -> simple search
    pressEscape(app);
    TEST_ASSERT_EQUAL(COORDINATE_SIMPLE_SEARCH, app->currentCoordinate);

    // Esc -> operator general (was broken: looped back to scroll)
    pressEscape(app);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);
}

// ============================================================
// Test: Multiple mode transitions
// ============================================================

void test_mode_transitions(void) {
    // Start in operator general
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);

    // Tab -> search
    pressTab(app);
    TEST_ASSERT_EQUAL(COORDINATE_SIMPLE_SEARCH, app->currentCoordinate);

    // Tab again from search -> scroll mode (locks to current search result)
    pressTab(app);
    TEST_ASSERT_EQUAL(COORDINATE_SCROLL, app->currentCoordinate);

    // Escape from scroll -> back to search
    pressEscape(app);
    TEST_ASSERT_EQUAL(COORDINATE_SIMPLE_SEARCH, app->currentCoordinate);

    // Escape from search -> back to operator
    pressEscape(app);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);
}

// ============================================================
// Helper: find provider index by name
// ============================================================

static int findProviderIndex(const char *name) {
    for (int i = 0; i < app->ffonCount; i++) {
        if (app->providers[i] && strcmp(app->providers[i]->name, name) == 0)
            return i;
    }
    return -1;
}

// Helper: navigate to a provider at root level (depth 1)
static void navigateToProvider(int targetIdx) {
    // Go to root if needed
    while (app->currentId.depth > 1) pressLeft(app);
    // Navigate to target
    while (app->currentId.ids[0] != targetIdx) {
        if (app->currentId.ids[0] < targetIdx) pressDown(app);
        else pressUp(app);
    }
}

// Helper: find child index by name in the current FFON object
static int findChildIndex(FfonObject *obj, const char *name) {
    for (int i = 0; i < obj->count; i++) {
        const char *childName = NULL;
        if (obj->elements[i]->type == FFON_STRING)
            childName = obj->elements[i]->data.string;
        else if (obj->elements[i]->type == FFON_OBJECT)
            childName = obj->elements[i]->data.object->key;
        if (childName) {
            char *extracted = providerTagExtractContent(childName);
            if (extracted && strcmp(extracted, name) == 0) {
                free(extracted);
                return i;
            }
            free(extracted);
        }
    }
    return -1;
}

// ============================================================
// Test: Full workflow — filebrowser + sales demo + save/load
// ============================================================

void test_full_workflow(void) {
    // Create Downloads subdirectory in temp dir
    createDir(tmpDir, "Downloads");
    char downloadsDir[512];
    snprintf(downloadsDir, sizeof(downloadsDir), "%s/Downloads", tmpDir);

    // Set save folder to our Downloads directory
    snprintf(app->saveFolderPath, sizeof(app->saveFolderPath), "%s", downloadsDir);

    int fbIdx = findProviderIndex("filebrowser");
    int sdIdx = findProviderIndex("sales demo");
    TEST_ASSERT_NOT_EQUAL(-1, fbIdx);
    TEST_ASSERT_NOT_EQUAL(-1, sdIdx);

    // ---- Step 1: Navigate to file browser, enter it ----
    navigateToProvider(fbIdx);
    pressRight(app);
    TEST_ASSERT_EQUAL(2, app->currentId.depth);

    // Refresh to pick up the newly created Downloads directory
    pressKey(app, SDLK_F5, 0);

    // ---- Step 2: Navigate into Downloads subdirectory ----
    FfonObject *fbObj = app->ffon[fbIdx]->data.object;
    int dlIdx = findChildIndex(fbObj, "Downloads");
    TEST_ASSERT_NOT_EQUAL_MESSAGE(-1, dlIdx, "Downloads dir not found in file browser");
    while (app->currentId.ids[1] != dlIdx) pressDown(app);
    pressRight(app);
    TEST_ASSERT_EQUAL(3, app->currentId.depth);

    // ---- Step 3: Create a file in Downloads ----
    pressCtrl(app, SDLK_I);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_INSERT, app->currentCoordinate);
    typeText(app, "- report.txt");
    pressEnter(app);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);
    TEST_ASSERT_TRUE_MESSAGE(fileExists(downloadsDir, "report.txt"),
        "Expected report.txt to be created in Downloads");

    // ---- Step 4: Delete the file (programmatic, at the OS level) ----
    {
        char reportPath[512];
        snprintf(reportPath, sizeof(reportPath), "%s/report.txt", downloadsDir);
        remove(reportPath);
        TEST_ASSERT_FALSE_MESSAGE(fileExists(downloadsDir, "report.txt"),
            "Expected report.txt to be deleted");
    }

    // ---- Step 5: Navigate back to root ----
    while (app->currentId.depth > 1) pressLeft(app);
    TEST_ASSERT_EQUAL(1, app->currentId.depth);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);

    // ---- Step 6: Navigate to sales demo provider ----
    navigateToProvider(sdIdx);
    pressRight(app);
    TEST_ASSERT_EQUAL(2, app->currentId.depth);
    TEST_ASSERT_EQUAL_STRING("sales demo", app->providers[sdIdx]->name);

    // ---- Step 7: Save (Ctrl+S) — triggers save-as since no currentSavePath ----
    TEST_ASSERT_EQUAL('\0', app->currentSavePath[0]);
    pressCtrl(app, SDLK_S);

    // Save-as flow auto-navigates to filebrowser at Downloads and enters insert mode
    TEST_ASSERT_TRUE_MESSAGE(app->pendingFileBrowserSaveAs,
        "Expected pendingFileBrowserSaveAs after Ctrl+S with empty save path");
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_INSERT, app->currentCoordinate);

    // Type filename and press Enter to commit save
    typeText(app, "myconfig");
    pressEnter(app);

    // Should return to sales demo provider
    TEST_ASSERT_FALSE(app->pendingFileBrowserSaveAs);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);
    TEST_ASSERT_EQUAL(sdIdx, app->currentId.ids[0]);

    // Verify the file was saved
    TEST_ASSERT_TRUE_MESSAGE(fileExists(downloadsDir, "myconfig.json"),
        "Expected myconfig.json to be saved in Downloads");

    // currentSavePath should now be set
    TEST_ASSERT_NOT_EQUAL('\0', app->currentSavePath[0]);

    // ---- Step 8: Save-as (Ctrl+Shift+S) ----
    pressCtrlShift(app, SDLK_S);
    TEST_ASSERT_TRUE_MESSAGE(app->pendingFileBrowserSaveAs,
        "Expected pendingFileBrowserSaveAs after Ctrl+Shift+S");
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_INSERT, app->currentCoordinate);

    typeText(app, "myconfig2");
    pressEnter(app);

    TEST_ASSERT_FALSE(app->pendingFileBrowserSaveAs);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);
    TEST_ASSERT_EQUAL(sdIdx, app->currentId.ids[0]);
    TEST_ASSERT_TRUE_MESSAGE(fileExists(downloadsDir, "myconfig2.json"),
        "Expected myconfig2.json to be saved in Downloads");

    // ---- Step 9: Load (Ctrl+O) — opens file browser to select a .json file ----
    pressCtrl(app, SDLK_O);
    TEST_ASSERT_TRUE_MESSAGE(app->pendingFileBrowserOpen,
        "Expected pendingFileBrowserOpen after Ctrl+O");
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);

    // We're now in the filebrowser at Downloads — find myconfig.json
    int currentDepth = app->currentId.depth;
    int fbRootIdx = app->currentId.ids[0];
    int pCount;
    FfonElement **pArr = getFfonAtId(app->ffon, app->ffonCount, &app->currentId, &pCount);
    TEST_ASSERT_NOT_NULL(pArr);

    // Find myconfig.json in the listing
    // Build parent ID to access the FFON object
    IdArray loadParentId;
    idArrayCopy(&loadParentId, &app->currentId);
    idArrayPop(&loadParentId);
    int loadParentCount;
    FfonElement **loadParentArr = getFfonAtId(app->ffon, app->ffonCount, &loadParentId, &loadParentCount);
    int loadParentIdx = loadParentId.ids[loadParentId.depth - 1];
    FfonObject *loadObj = loadParentArr[loadParentIdx]->data.object;

    int configIdx = findChildIndex(loadObj, "myconfig.json");
    TEST_ASSERT_NOT_EQUAL_MESSAGE(-1, configIdx, "myconfig.json not found for loading");
    while (app->currentId.ids[app->currentId.depth - 1] != configIdx) pressDown(app);

    // Press Enter to select the file → loads it into sales demo
    pressEnter(app);

    // Should return to sales demo provider
    TEST_ASSERT_FALSE(app->pendingFileBrowserOpen);
    TEST_ASSERT_EQUAL(sdIdx, app->currentId.ids[0]);

    // ---- Step 10: Navigate back to file browser → Downloads → create file ----
    while (app->currentId.depth > 1) pressLeft(app);
    navigateToProvider(fbIdx);

    // Reset filebrowser path to tmpDir (save/load flows reset it to "/")
    Provider *fb = app->providers[fbIdx];
    if (fb->setCurrentPath) fb->setCurrentPath(fb, tmpDir);

    pressRight(app);

    // Refresh to see tmpDir contents
    pressKey(app, SDLK_F5, 0);

    // Navigate into Downloads again
    fbObj = app->ffon[fbIdx]->data.object;
    dlIdx = findChildIndex(fbObj, "Downloads");
    TEST_ASSERT_NOT_EQUAL_MESSAGE(-1, dlIdx,
        "Downloads not found in file browser after returning from save/load");
    while (app->currentId.ids[1] != dlIdx) pressDown(app);
    pressRight(app);

    // Create a final file
    pressCtrl(app, SDLK_I);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_INSERT, app->currentCoordinate);
    typeText(app, "- final.txt");
    pressEnter(app);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);
    TEST_ASSERT_TRUE_MESSAGE(fileExists(downloadsDir, "final.txt"),
        "Expected final.txt to be created in Downloads after full workflow");
}

// ============================================================
// Test: Web browser Enter on URL bar commits the input
// ============================================================

void test_webbrowser_enter_commits_url(void) {
    int wbIdx = findProviderIndex("webbrowser");
    TEST_ASSERT_NOT_EQUAL_MESSAGE(-1, wbIdx, "web browser provider not found");

    // Navigate to web browser and enter it
    navigateToProvider(wbIdx);
    pressRight(app);
    TEST_ASSERT_EQUAL(2, app->currentId.depth);

    // The first element should be the URL bar <input>
    FfonObject *wbObj = app->ffon[wbIdx]->data.object;
    TEST_ASSERT_NOT_NULL(wbObj);
    TEST_ASSERT_TRUE(wbObj->count >= 1);

    FfonElement *urlBar = wbObj->elements[0];
    TEST_ASSERT_NOT_NULL(urlBar);
    // URL bar is a string element with <input> tag
    const char *urlBarStr = (urlBar->type == FFON_STRING) ?
        urlBar->data.string : urlBar->data.object->key;
    TEST_ASSERT_TRUE_MESSAGE(providerTagHasInput(urlBarStr),
        "First element of web browser should be an <input> URL bar");

    // Navigate to the URL bar element (should be at index 0)
    app->currentId.ids[app->currentId.depth - 1] = 0;
    createListCurrentLayer(app);

    // No provider error before pressing Enter
    TEST_ASSERT_EQUAL('\0', app->providers[wbIdx]->errorMessage[0]);

    // Press Enter on the URL bar in operator mode — this should trigger commit.
    // The default URL "https://" is invalid, so wbCommit will try to fetch
    // and fail, setting the error message. This proves the commit path was
    // taken (without the fix, Enter would be a no-op).
    pressEnter(app);

    // The error message proves wbCommit was called and curl attempted the fetch
    TEST_ASSERT_EQUAL_STRING_MESSAGE("failed to fetch URL",
        app->errorMessage,
        "Enter on web browser URL bar should trigger commit (fetch attempt)");
}

// ============================================================
// Test: File browser Enter on file does NOT commit (no rename)
// ============================================================

void test_filebrowser_enter_does_not_commit_input(void) {
    int fbIdx = findProviderIndex("filebrowser");
    TEST_ASSERT_NOT_EQUAL(-1, fbIdx);

    // Navigate to file browser and enter it
    navigateToProvider(fbIdx);
    pressRight(app);
    TEST_ASSERT_EQUAL(2, app->currentId.depth);

    // Find alpha.txt in the listing
    FfonObject *fbObj = app->ffon[fbIdx]->data.object;
    int alphaIdx = findChildIndex(fbObj, "alpha.txt");
    TEST_ASSERT_NOT_EQUAL_MESSAGE(-1, alphaIdx, "alpha.txt not found");

    // Navigate to alpha.txt
    while (app->currentId.ids[1] != alphaIdx) pressDown(app);

    // Press Enter — should NOT trigger a commit/rename, the file should keep its name
    pressEnter(app);

    // The file should still be named alpha.txt on disk
    TEST_ASSERT_TRUE_MESSAGE(fileExists(tmpDir, "alpha.txt"),
        "alpha.txt should still exist after pressing Enter (no accidental rename)");
}

// ============================================================
// Test: List item label has type prefix for filebrowser file items
// ============================================================

void test_list_item_label_has_prefix(void) {
    int fbIdx = findProviderIndex("filebrowser");
    TEST_ASSERT_NOT_EQUAL(-1, fbIdx);

    navigateToProvider(fbIdx);
    pressRight(app);
    TEST_ASSERT_EQUAL(2, app->currentId.depth);

    // Navigate to alpha.txt (an <input> file item)
    FfonObject *fbObj = app->ffon[fbIdx]->data.object;
    int alphaIdx = findChildIndex(fbObj, "alpha.txt");
    TEST_ASSERT_NOT_EQUAL_MESSAGE(-1, alphaIdx, "alpha.txt not found");
    while (app->currentId.ids[1] != alphaIdx) pressDown(app);

    // Explicitly rebuild list and sync listIndex
    createListCurrentLayer(app);
    app->listIndex = app->currentId.ids[app->currentId.depth - 1];

    TEST_ASSERT_NOT_NULL_MESSAGE(app->totalListCurrentLayer, "List should be populated");
    TEST_ASSERT_TRUE_MESSAGE(app->listIndex >= 0 && app->listIndex < app->totalListCount,
        "listIndex out of range");

    const char *label = app->totalListCurrentLayer[app->listIndex].label;
    TEST_ASSERT_NOT_NULL_MESSAGE(label, "List item should have a label");

    // Filebrowser files have <input> tag → list prefix "-i"
    // The label is used as input to labelToSpeech which produces the spoken announcement
    char failMsg[512];
    snprintf(failMsg, sizeof(failMsg), "Expected label to start with '-i ', got: '%s'", label);
    TEST_ASSERT_TRUE_MESSAGE(strncmp(label, "-i ", 3) == 0, failMsg);
    TEST_ASSERT_TRUE_MESSAGE(strstr(label, "alpha.txt") != NULL, "Label should contain filename");
}

// ============================================================
// Test: handleI populates inputBuffer with the editable value
// ============================================================

void test_handleI_populates_input_buffer(void) {
    int fbIdx = findProviderIndex("filebrowser");
    TEST_ASSERT_NOT_EQUAL(-1, fbIdx);

    navigateToProvider(fbIdx);
    pressRight(app);

    FfonObject *fbObj = app->ffon[fbIdx]->data.object;
    int alphaIdx = findChildIndex(fbObj, "alpha.txt");
    TEST_ASSERT_NOT_EQUAL_MESSAGE(-1, alphaIdx, "alpha.txt not found");
    while (app->currentId.ids[1] != alphaIdx) pressDown(app);

    pressKey(app, SDLK_I, 0);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_INSERT, app->currentCoordinate);

    // inputBuffer is what accesskitSpeakModeChange uses as context for the announcement
    char failMsg[512];
    snprintf(failMsg, sizeof(failMsg), "Expected inputBuffer='alpha.txt', got: '%s'", app->inputBuffer);
    TEST_ASSERT_EQUAL_STRING_MESSAGE("alpha.txt", app->inputBuffer, failMsg);
}

// ============================================================
// Test: Undo/redo of filesystem file creation
// ============================================================

void test_undo_file_creation(void) {
    int fbIdx = -1;
    for (int i = 0; i < app->ffonCount; i++) {
        if (strcmp(app->providers[i]->name, "filebrowser") == 0) {
            fbIdx = i;
            break;
        }
    }
    TEST_ASSERT_NOT_EQUAL(-1, fbIdx);

    while (app->currentId.ids[0] != fbIdx) pressDown(app);
    pressRight(app);

    // Create a file via insert mode
    pressCtrl(app, SDLK_I);
    typeText(app, "- undotest.txt");
    pressEnter(app);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);
    TEST_ASSERT_TRUE_MESSAGE(fileExists(tmpDir, "undotest.txt"),
        "File should exist after creation");

    // Undo should delete the file
    pressCtrl(app, SDLK_Z);
    TEST_ASSERT_FALSE_MESSAGE(fileExists(tmpDir, "undotest.txt"),
        "File should be deleted after undo");

    // Redo should re-create the file
    pressCtrlShift(app, SDLK_Z);
    TEST_ASSERT_TRUE_MESSAGE(fileExists(tmpDir, "undotest.txt"),
        "File should be re-created after redo");
}

// ============================================================
// Test: Undo/redo of filesystem directory creation
// ============================================================

void test_undo_directory_creation(void) {
    int fbIdx = -1;
    for (int i = 0; i < app->ffonCount; i++) {
        if (strcmp(app->providers[i]->name, "filebrowser") == 0) {
            fbIdx = i;
            break;
        }
    }
    TEST_ASSERT_NOT_EQUAL(-1, fbIdx);

    while (app->currentId.ids[0] != fbIdx) pressDown(app);
    pressRight(app);
    int depthBeforeCreate = app->currentId.depth;

    // Create a directory via insert mode
    pressCtrl(app, SDLK_I);
    typeText(app, "+ undodir");
    pressEnter(app);
    TEST_ASSERT_EQUAL(COORDINATE_OPERATOR_GENERAL, app->currentCoordinate);
    // After creating a directory, we navigate into it (depth increases)
    TEST_ASSERT_EQUAL_MESSAGE(depthBeforeCreate + 1, app->currentId.depth,
        "Should be inside the new directory after creation");

    char dirPath[512];
    snprintf(dirPath, sizeof(dirPath), "%s/undodir", tmpDir);
    struct stat st;
    TEST_ASSERT_EQUAL_MESSAGE(0, stat(dirPath, &st), "Directory should exist after creation");
    TEST_ASSERT_TRUE(S_ISDIR(st.st_mode));

    // Undo should delete the directory and navigate back to parent
    pressCtrl(app, SDLK_Z);
    TEST_ASSERT_NOT_EQUAL_MESSAGE(0, stat(dirPath, &st),
        "Directory should be deleted after undo");
    TEST_ASSERT_EQUAL_MESSAGE(depthBeforeCreate, app->currentId.depth,
        "Should be back at parent level after undo");

    // Second undo should not crash (no stale TASK_INSERT entry)
    pressCtrl(app, SDLK_Z);

    // Redo should re-create the directory and navigate into it
    pressCtrlShift(app, SDLK_Z);
    TEST_ASSERT_EQUAL_MESSAGE(0, stat(dirPath, &st),
        "Directory should be re-created after redo");
    TEST_ASSERT_TRUE(S_ISDIR(st.st_mode));
    TEST_ASSERT_EQUAL_MESSAGE(depthBeforeCreate + 1, app->currentId.depth,
        "Should be inside the directory after redo");

    // Undo again to clean up
    pressCtrl(app, SDLK_Z);
    TEST_ASSERT_NOT_EQUAL_MESSAGE(0, stat(dirPath, &st),
        "Directory should be deleted after second undo");
}

// ============================================================

int main(void) {
    UNITY_BEGIN();
    RUN_TEST(test_initial_state);
    RUN_TEST(test_navigate_between_providers);
    RUN_TEST(test_enter_provider_and_navigate_back);
    RUN_TEST(test_filebrowser_shows_temp_files);
    RUN_TEST(test_search_mode_tab);
    RUN_TEST(test_provider_active_changes_with_navigation);
    RUN_TEST(test_navigate_into_subdirectory);
    RUN_TEST(test_provider_state_preserved);
    RUN_TEST(test_file_creation_via_command);
    RUN_TEST(test_directory_creation_via_command);
    RUN_TEST(test_escape_returns_to_operator);
    RUN_TEST(test_file_deletion);
    RUN_TEST(test_scroll_search_esc_chain);
    RUN_TEST(test_mode_transitions);
    RUN_TEST(test_full_workflow);
    RUN_TEST(test_webbrowser_enter_commits_url);
    RUN_TEST(test_filebrowser_enter_does_not_commit_input);
    RUN_TEST(test_list_item_label_has_prefix);
    RUN_TEST(test_handleI_populates_input_buffer);
    RUN_TEST(test_undo_file_creation);
    RUN_TEST(test_undo_directory_creation);
    return UNITY_END();
}

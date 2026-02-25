/*
 * Tests for filebrowser library functions.
 * Functions under test: filebrowserCreateFile, filebrowserCreateDirectory,
 *                       filebrowserListDirectory, filebrowserRename,
 *                       filebrowserDelete, filebrowserCopy,
 *                       filebrowserCleanupClipboardCache
 */

#include <unity.h>
#include <filebrowser.h>
#include <provider_tags.h>
#include <ffon.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>

static char tmpDir[256];

static bool fileExists(const char *dir, const char *name) {
    char path[512];
    snprintf(path, sizeof(path), "%s/%s", dir, name);
    struct stat st;
    return stat(path, &st) == 0;
}

static bool isDirectory(const char *dir, const char *name) {
    char path[512];
    snprintf(path, sizeof(path), "%s/%s", dir, name);
    struct stat st;
    return stat(path, &st) == 0 && S_ISDIR(st.st_mode);
}

void setUp(void) {
    snprintf(tmpDir, sizeof(tmpDir), "/tmp/sicompass_fb_test_XXXXXX");
    mkdtemp(tmpDir);
}

void tearDown(void) {
    char cmd[512];
    snprintf(cmd, sizeof(cmd), "rm -rf %s", tmpDir);
    system(cmd);
}

// --- filebrowserCreateFile ---

void test_createFile_success(void) {
    bool result = filebrowserCreateFile(tmpDir, "newfile.txt");
    TEST_ASSERT_TRUE(result);
    TEST_ASSERT_TRUE(fileExists(tmpDir, "newfile.txt"));
}

void test_createFile_already_exists(void) {
    filebrowserCreateFile(tmpDir, "existing.txt");
    // Creating again may or may not fail depending on implementation
    // but should not crash
    filebrowserCreateFile(tmpDir, "existing.txt");
    TEST_ASSERT_TRUE(fileExists(tmpDir, "existing.txt"));
}

// --- filebrowserCreateDirectory ---

void test_createDirectory_success(void) {
    bool result = filebrowserCreateDirectory(tmpDir, "newdir");
    TEST_ASSERT_TRUE(result);
    TEST_ASSERT_TRUE(isDirectory(tmpDir, "newdir"));
}

// --- filebrowserListDirectory ---

void test_listDirectory_empty(void) {
    int count;
    FfonElement **elems = filebrowserListDirectory(tmpDir, false, false,
                                                    FILEBROWSER_SORT_ALPHA, &count);
    // Empty dir should return 0 elements (or NULL)
    if (elems) {
        for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
        free(elems);
    }
    TEST_ASSERT_EQUAL_INT(0, count);
}

void test_listDirectory_with_files(void) {
    filebrowserCreateFile(tmpDir, "alpha.txt");
    filebrowserCreateFile(tmpDir, "beta.txt");

    int count;
    FfonElement **elems = filebrowserListDirectory(tmpDir, false, false,
                                                    FILEBROWSER_SORT_ALPHA, &count);
    TEST_ASSERT_NOT_NULL(elems);
    TEST_ASSERT_EQUAL_INT(2, count);

    // Verify elements have <input> tags
    for (int i = 0; i < count; i++) {
        if (elems[i]->type == FFON_STRING) {
            TEST_ASSERT_TRUE(providerTagHasInput(elems[i]->data.string));
        } else {
            TEST_ASSERT_TRUE(providerTagHasInput(elems[i]->data.object->key));
        }
    }

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_listDirectory_with_subdirectory(void) {
    filebrowserCreateDirectory(tmpDir, "subdir");
    filebrowserCreateFile(tmpDir, "file.txt");

    int count;
    FfonElement **elems = filebrowserListDirectory(tmpDir, false, false,
                                                    FILEBROWSER_SORT_ALPHA, &count);
    TEST_ASSERT_NOT_NULL(elems);
    TEST_ASSERT_EQUAL_INT(2, count);

    // One should be an object (directory), one a string (file)
    bool hasObject = false, hasString = false;
    for (int i = 0; i < count; i++) {
        if (elems[i]->type == FFON_OBJECT) hasObject = true;
        if (elems[i]->type == FFON_STRING) hasString = true;
    }
    TEST_ASSERT_TRUE(hasObject);
    TEST_ASSERT_TRUE(hasString);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_listDirectory_alphabetical_sort(void) {
    filebrowserCreateFile(tmpDir, "cherry.txt");
    filebrowserCreateFile(tmpDir, "apple.txt");
    filebrowserCreateFile(tmpDir, "banana.txt");

    int count;
    FfonElement **elems = filebrowserListDirectory(tmpDir, false, false,
                                                    FILEBROWSER_SORT_ALPHA, &count);
    TEST_ASSERT_NOT_NULL(elems);
    TEST_ASSERT_EQUAL_INT(3, count);

    // Extract names and verify alphabetical order
    char *name0 = providerTagExtractContent(elems[0]->data.string);
    char *name1 = providerTagExtractContent(elems[1]->data.string);
    char *name2 = providerTagExtractContent(elems[2]->data.string);
    TEST_ASSERT_TRUE(strcasecmp(name0, name1) <= 0);
    TEST_ASSERT_TRUE(strcasecmp(name1, name2) <= 0);
    free(name0); free(name1); free(name2);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_listDirectory_with_properties(void) {
    filebrowserCreateFile(tmpDir, "testfile.txt");

    int count;
    FfonElement **elems = filebrowserListDirectory(tmpDir, false, true,
                                                    FILEBROWSER_SORT_ALPHA, &count);
    TEST_ASSERT_NOT_NULL(elems);
    TEST_ASSERT_EQUAL_INT(1, count);
    // With showProperties=true, element text should have content before <input> tag
    if (elems[0]->type == FFON_STRING) {
        TEST_ASSERT_TRUE(providerTagHasInput(elems[0]->data.string));
    }

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_listDirectory_nonexistent(void) {
    int count;
    FfonElement **elems = filebrowserListDirectory("/nonexistent/path/xyz",
                                                    false, false,
                                                    FILEBROWSER_SORT_ALPHA, &count);
    TEST_ASSERT_NULL(elems);
    TEST_ASSERT_EQUAL_INT(0, count);
}

// --- filebrowserRename ---

void test_rename_file(void) {
    filebrowserCreateFile(tmpDir, "old.txt");
    bool result = filebrowserRename(tmpDir, "old.txt", "new.txt");
    TEST_ASSERT_TRUE(result);
    TEST_ASSERT_FALSE(fileExists(tmpDir, "old.txt"));
    TEST_ASSERT_TRUE(fileExists(tmpDir, "new.txt"));
}

void test_rename_nonexistent(void) {
    bool result = filebrowserRename(tmpDir, "nonexistent.txt", "new.txt");
    TEST_ASSERT_FALSE(result);
}

void test_rename_directory(void) {
    filebrowserCreateDirectory(tmpDir, "olddir");
    bool result = filebrowserRename(tmpDir, "olddir", "newdir");
    TEST_ASSERT_TRUE(result);
    TEST_ASSERT_FALSE(isDirectory(tmpDir, "olddir"));
    TEST_ASSERT_TRUE(isDirectory(tmpDir, "newdir"));
}

// --- filebrowserDelete ---

void test_delete_file(void) {
    filebrowserCreateFile(tmpDir, "todelete.txt");
    bool result = filebrowserDelete(tmpDir, "todelete.txt");
    TEST_ASSERT_TRUE(result);
    TEST_ASSERT_FALSE(fileExists(tmpDir, "todelete.txt"));
}

void test_delete_directory(void) {
    filebrowserCreateDirectory(tmpDir, "deldir");
    bool result = filebrowserDelete(tmpDir, "deldir");
    TEST_ASSERT_TRUE(result);
    TEST_ASSERT_FALSE(isDirectory(tmpDir, "deldir"));
}

void test_delete_nonexistent(void) {
    bool result = filebrowserDelete(tmpDir, "nonexistent");
    TEST_ASSERT_FALSE(result);
}

// --- filebrowserCopy ---

void test_copy_file(void) {
    filebrowserCreateFile(tmpDir, "source.txt");
    // Write some content to verify copy
    char srcPath[512];
    snprintf(srcPath, sizeof(srcPath), "%s/source.txt", tmpDir);
    FILE *fp = fopen(srcPath, "w");
    fprintf(fp, "test content");
    fclose(fp);

    bool result = filebrowserCopy(tmpDir, "source.txt", tmpDir, "copy.txt");
    TEST_ASSERT_TRUE(result);
    TEST_ASSERT_TRUE(fileExists(tmpDir, "source.txt"));  // original still exists
    TEST_ASSERT_TRUE(fileExists(tmpDir, "copy.txt"));
}

void test_copy_directory(void) {
    filebrowserCreateDirectory(tmpDir, "srcdir");
    // Create a file inside the directory
    char subDir[512];
    snprintf(subDir, sizeof(subDir), "%s/srcdir", tmpDir);
    filebrowserCreateFile(subDir, "inner.txt");

    bool result = filebrowserCopy(tmpDir, "srcdir", tmpDir, "cpdir");
    TEST_ASSERT_TRUE(result);
    TEST_ASSERT_TRUE(isDirectory(tmpDir, "srcdir"));
    TEST_ASSERT_TRUE(isDirectory(tmpDir, "cpdir"));
}

// --- filebrowserCleanupClipboardCache ---

void test_cleanupClipboardCache_no_crash(void) {
    filebrowserCleanupClipboardCache();  // should not crash even with no cache
}

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_createFile_success);
    RUN_TEST(test_createFile_already_exists);

    RUN_TEST(test_createDirectory_success);

    RUN_TEST(test_listDirectory_empty);
    RUN_TEST(test_listDirectory_with_files);
    RUN_TEST(test_listDirectory_with_subdirectory);
    RUN_TEST(test_listDirectory_alphabetical_sort);
    RUN_TEST(test_listDirectory_with_properties);
    RUN_TEST(test_listDirectory_nonexistent);

    RUN_TEST(test_rename_file);
    RUN_TEST(test_rename_nonexistent);
    RUN_TEST(test_rename_directory);

    RUN_TEST(test_delete_file);
    RUN_TEST(test_delete_directory);
    RUN_TEST(test_delete_nonexistent);

    RUN_TEST(test_copy_file);
    RUN_TEST(test_copy_directory);

    RUN_TEST(test_cleanupClipboardCache_no_crash);

    return UNITY_END();
}

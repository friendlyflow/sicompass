/*
 * Tests for settings provider library.
 * Functions under test: settingsProviderCreate, settingsAddSection,
 *                       settingsAddSectionRadio, fetch, onRadioChange,
 *                       path management
 */

#include <unity.h>
#include <settings_provider.h>
#include <provider_tags.h>
#include <ffon.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <test_compat.h>

// Callback tracking
static int callbackCount;
static char lastCallbackKey[256];
static char lastCallbackValue[256];
static char callbackKeys[16][256];
static char callbackValues[16][256];

static void testApplyCallback(const char *key, const char *value, void *userdata) {
    (void)userdata;
    if (callbackCount < 16) {
        strncpy(callbackKeys[callbackCount], key, 255);
        strncpy(callbackValues[callbackCount], value, 255);
    }
    strncpy(lastCallbackKey, key, 255);
    strncpy(lastCallbackValue, value, 255);
    callbackCount++;
}

static char tmpDir[256];

void setUp(void) {
    callbackCount = 0;
    memset(lastCallbackKey, 0, sizeof(lastCallbackKey));
    memset(lastCallbackValue, 0, sizeof(lastCallbackValue));
    memset(callbackKeys, 0, sizeof(callbackKeys));
    memset(callbackValues, 0, sizeof(callbackValues));

    // Set XDG_CONFIG_HOME to a temp dir so tests don't touch real config

#ifdef _WIN32
    snprintf(tmpDir, sizeof(tmpDir), "%s\\sicompass_settings_test",
             getenv("TEMP") ? getenv("TEMP") : "C:\\Temp");
    _mkdir(tmpDir);
#else
    snprintf(tmpDir, sizeof(tmpDir), "/tmp/sicompass_settings_test_XXXXXX");
    mkdtemp(tmpDir);
#endif
    setenv("XDG_CONFIG_HOME", tmpDir, 1);
}

void tearDown(void) {
    char cmd[512];
#ifdef _WIN32
    snprintf(cmd, sizeof(cmd), "rmdir /s /q \"%s\"", tmpDir);
#else
    snprintf(cmd, sizeof(cmd), "rm -rf %s", tmpDir);
#endif
    system(cmd);
#ifndef _WIN32
    unsetenv("XDG_CONFIG_HOME");
#endif
}

// --- settingsProviderCreate ---

void test_create_with_callback(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);
    TEST_ASSERT_NOT_NULL(p);
    TEST_ASSERT_EQUAL_STRING("settings", p->name);
    TEST_ASSERT_NOT_NULL(p->fetch);
    TEST_ASSERT_NOT_NULL(p->init);
    TEST_ASSERT_NOT_NULL(p->pushPath);
    TEST_ASSERT_NOT_NULL(p->popPath);
    TEST_ASSERT_NOT_NULL(p->getCurrentPath);
    TEST_ASSERT_NOT_NULL(p->onRadioChange);
    free(p->state);
    free(p);
}

void test_create_with_null_callback(void) {
    Provider *p = settingsProviderCreate(NULL, NULL);
    TEST_ASSERT_NOT_NULL(p);
    TEST_ASSERT_EQUAL_STRING("settings", p->name);
    free(p->state);
    free(p);
}

// --- settingsAddSection ---

void test_addSection_one(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSection(p, "file browser");

    // Verify via fetch: should return sicompass + "file browser"
    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(3, count);
    TEST_ASSERT_EQUAL_STRING("sicompass", elems[1]->data.object->key);
    TEST_ASSERT_EQUAL_STRING("file browser", elems[2]->data.object->key);

    // "file browser" section has "no settings" placeholder
    TEST_ASSERT_EQUAL_INT(1, elems[2]->data.object->count);
    TEST_ASSERT_EQUAL_STRING("no settings",
        elems[2]->data.object->elements[0]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_addSection_null_provider(void) {
    settingsAddSection(NULL, "test");  // should not crash
}

void test_addSection_null_name(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);
    settingsAddSection(p, NULL);  // should not crash

    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(2, count);  // only sicompass, no extra section

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

// --- settingsAddSectionRadio ---

void test_addSectionRadio(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    const char *options[] = { "alphabetical", "chronological" };
    settingsAddSectionRadio(p, "file browser", "global sorting", "sortOrder",
                            options, 2, "alphabetical");

    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(3, count);  // sicompass + file browser

    // file browser section should have a radio group
    FfonObject *fbSection = elems[2]->data.object;
    TEST_ASSERT_EQUAL_STRING("file browser", fbSection->key);
    TEST_ASSERT_EQUAL_INT(1, fbSection->count);

    FfonElement *radioElem = fbSection->elements[0];
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, radioElem->type);
    TEST_ASSERT_TRUE(providerTagHasRadio(radioElem->data.object->key));

    // Verify options
    FfonObject *radioGroup = radioElem->data.object;
    TEST_ASSERT_EQUAL_INT(2, radioGroup->count);

    // "alphabetical" should be checked (default)
    TEST_ASSERT_TRUE(providerTagHasChecked(radioGroup->elements[0]->data.string));

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_addSectionRadio_auto_creates_section(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    const char *options[] = { "a", "b" };
    settingsAddSectionRadio(p, "auto section", "radio", "key",
                            options, 2, "a");

    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(3, count);  // sicompass + auto section
    TEST_ASSERT_EQUAL_STRING("auto section", elems[2]->data.object->key);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

// --- Fetch: default color scheme ---

void test_fetch_default_color_scheme(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(2, count);

    FfonObject *sicompass = elems[1]->data.object;
    TEST_ASSERT_EQUAL_STRING("sicompass", sicompass->key);
    TEST_ASSERT_EQUAL_INT(1, sicompass->count);

    // Radio group for color scheme
    FfonElement *radioElem = sicompass->elements[0];
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, radioElem->type);
    TEST_ASSERT_TRUE(providerTagHasRadio(radioElem->data.object->key));

    FfonObject *radioGroup = radioElem->data.object;
    TEST_ASSERT_EQUAL_INT(2, radioGroup->count);
    // Default is dark, so dark should be checked
    TEST_ASSERT_TRUE(providerTagHasChecked(radioGroup->elements[0]->data.string));
    TEST_ASSERT_FALSE(providerTagHasChecked(radioGroup->elements[1]->data.string));

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

// --- Path management ---

void test_path_push_pop(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    TEST_ASSERT_EQUAL_STRING("/", p->getCurrentPath(p));

    p->pushPath(p, "sicompass");
    TEST_ASSERT_EQUAL_STRING("/sicompass", p->getCurrentPath(p));

    p->pushPath(p, "color scheme");
    TEST_ASSERT_EQUAL_STRING("/sicompass/color scheme", p->getCurrentPath(p));

    p->popPath(p);
    TEST_ASSERT_EQUAL_STRING("/sicompass", p->getCurrentPath(p));

    p->popPath(p);
    TEST_ASSERT_EQUAL_STRING("/", p->getCurrentPath(p));

    free(p->state);
    free(p);
}

void test_path_popPath_at_root(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);
    p->popPath(p);
    TEST_ASSERT_EQUAL_STRING("/", p->getCurrentPath(p));
    free(p->state);
    free(p);
}

// --- onRadioChange ---

void test_onRadioChange_color_scheme(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    p->onRadioChange(p, "color scheme", "light");

    TEST_ASSERT_EQUAL_INT(1, callbackCount);
    TEST_ASSERT_EQUAL_STRING("colorScheme", lastCallbackKey);
    TEST_ASSERT_EQUAL_STRING("light", lastCallbackValue);

    // Verify fetch now reflects the change
    int count;
    FfonElement **elems = p->fetch(p, &count);
    FfonObject *radioGroup = elems[1]->data.object->elements[0]->data.object;
    // dark should not be checked, light should be checked
    TEST_ASSERT_FALSE(providerTagHasChecked(radioGroup->elements[0]->data.string));
    TEST_ASSERT_TRUE(providerTagHasChecked(radioGroup->elements[1]->data.string));

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_onRadioChange_custom_radio(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    const char *options[] = { "alpha", "chrono" };
    settingsAddSectionRadio(p, "file browser", "global sorting", "sortOrder",
                            options, 2, "alpha");

    p->onRadioChange(p, "global sorting", "chrono");

    TEST_ASSERT_EQUAL_INT(1, callbackCount);
    TEST_ASSERT_EQUAL_STRING("sortOrder", lastCallbackKey);
    TEST_ASSERT_EQUAL_STRING("chrono", lastCallbackValue);

    free(p->state);
    free(p);
}

void test_onRadioChange_null_callback(void) {
    Provider *p = settingsProviderCreate(NULL, NULL);
    // Should not crash even with NULL callback
    p->onRadioChange(p, "color scheme", "light");
    free(p->state);
    free(p);
}

// --- Init callback ---

void test_init_calls_callback_for_defaults(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    const char *options[] = { "alpha", "chrono" };
    settingsAddSectionRadio(p, "file browser", "global sorting", "sortOrder",
                            options, 2, "alpha");

    p->init(p);

    // Should have called callback for colorScheme and sortOrder
    TEST_ASSERT_EQUAL_INT(2, callbackCount);
    TEST_ASSERT_EQUAL_STRING("colorScheme", callbackKeys[0]);
    TEST_ASSERT_EQUAL_STRING("dark", callbackValues[0]);
    TEST_ASSERT_EQUAL_STRING("sortOrder", callbackKeys[1]);
    TEST_ASSERT_EQUAL_STRING("alpha", callbackValues[1]);

    free(p->state);
    free(p);
}

// --- settingsAddSectionText ---

void test_addSectionText(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSectionText(p, "sales demo", "save folder", "saveFolder", "Downloads");

    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(3, count);  // sicompass + sales demo

    // sales demo section should have an object with the label
    FfonObject *sdSection = elems[2]->data.object;
    TEST_ASSERT_EQUAL_STRING("sales demo", sdSection->key);
    TEST_ASSERT_EQUAL_INT(1, sdSection->count);

    // Text entry should be a flat string "save folder: <input>Downloads</input>"
    FfonElement *textElem = sdSection->elements[0];
    TEST_ASSERT_EQUAL_INT(FFON_STRING, textElem->type);
    TEST_ASSERT_TRUE(providerTagHasInput(textElem->data.string));
    char *content = providerTagExtractContent(textElem->data.string);
    TEST_ASSERT_EQUAL_STRING("Downloads", content);
    free(content);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_addSectionText_auto_creates_section(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSectionText(p, "new section", "label", "key", "value");

    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(3, count);  // sicompass + new section
    TEST_ASSERT_EQUAL_STRING("new section", elems[2]->data.object->key);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_init_calls_callback_for_text_entries(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSectionText(p, "sales demo", "save folder", "saveFolder", "Downloads");

    p->init(p);

    // Should have called callback for colorScheme and saveFolder
    TEST_ASSERT_EQUAL_INT(2, callbackCount);
    TEST_ASSERT_EQUAL_STRING("colorScheme", callbackKeys[0]);
    TEST_ASSERT_EQUAL_STRING("saveFolder", callbackKeys[1]);
    TEST_ASSERT_EQUAL_STRING("Downloads", callbackValues[1]);

    free(p->state);
    free(p);
}

void test_commitEdit_text_entry(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSectionText(p, "sales demo", "save folder", "saveFolder", "Downloads");

    // Navigate into the text entry path
    p->pushPath(p, "sales demo");
    p->pushPath(p, "save folder");

    // Simulate editing the text entry
    TEST_ASSERT_NOT_NULL(p->commitEdit);
    bool result = p->commitEdit(p, "Downloads", "Documents");
    TEST_ASSERT_TRUE(result);

    // Callback should have been fired
    TEST_ASSERT_EQUAL_INT(1, callbackCount);
    TEST_ASSERT_EQUAL_STRING("saveFolder", lastCallbackKey);
    TEST_ASSERT_EQUAL_STRING("Documents", lastCallbackValue);

    // Fetch should reflect updated value
    int count;
    FfonElement **elems = p->fetch(p, &count);
    FfonObject *sdSection = elems[2]->data.object;
    FfonElement *textElem = sdSection->elements[0];
    char *content = providerTagExtractContent(textElem->data.string);
    TEST_ASSERT_EQUAL_STRING("Documents", content);
    free(content);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_commitEdit_wrong_path_returns_false(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSectionText(p, "sales demo", "save folder", "saveFolder", "Downloads");

    // Don't navigate to the right path
    p->pushPath(p, "sicompass");

    bool result = p->commitEdit(p, "Downloads", "Documents");
    TEST_ASSERT_FALSE(result);
    TEST_ASSERT_EQUAL_INT(0, callbackCount);

    free(p->state);
    free(p);
}

void test_section_with_radio_and_text(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    const char *options[] = { "a", "b" };
    settingsAddSectionRadio(p, "mixed", "radio group", "radioKey", options, 2, "a");
    settingsAddSectionText(p, "mixed", "text field", "textKey", "hello");

    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(3, count);  // sicompass + mixed

    FfonObject *section = elems[2]->data.object;
    TEST_ASSERT_EQUAL_STRING("mixed", section->key);
    TEST_ASSERT_EQUAL_INT(2, section->count);  // radio + text

    // First child: radio group
    TEST_ASSERT_TRUE(providerTagHasRadio(section->elements[0]->data.object->key));
    // Second child: flat text entry string
    TEST_ASSERT_EQUAL_INT(FFON_STRING, section->elements[1]->type);
    TEST_ASSERT_TRUE(providerTagHasInput(section->elements[1]->data.string));

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

// --- settingsAddPrioritySection ---

void test_prioritySection_renders_first(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSection(p, "other section");
    settingsAddPrioritySection(p, "programs");

    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(4, count);  // programs + sicompass + other section
    TEST_ASSERT_EQUAL_STRING("programs", elems[1]->data.object->key);
    TEST_ASSERT_EQUAL_STRING("sicompass", elems[2]->data.object->key);
    TEST_ASSERT_EQUAL_STRING("other section", elems[3]->data.object->key);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_prioritySection_not_duplicated(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddPrioritySection(p, "programs");
    settingsAddSectionCheckbox(p, "programs", "tutorial", "enable_tutorial", true);

    int count;
    FfonElement **elems = p->fetch(p, &count);
    // Only 2: programs (priority) + sicompass — not duplicated
    TEST_ASSERT_EQUAL_INT(3, count);
    TEST_ASSERT_EQUAL_STRING("programs", elems[1]->data.object->key);
    TEST_ASSERT_EQUAL_STRING("sicompass", elems[2]->data.object->key);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

// --- settingsAddSectionCheckbox ---

void test_checkbox_renders_checked(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSectionCheckbox(p, "programs", "tutorial", "enable_tutorial", true);

    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(3, count);  // sicompass + programs

    FfonObject *section = elems[2]->data.object;
    TEST_ASSERT_EQUAL_STRING("programs", section->key);
    TEST_ASSERT_EQUAL_INT(1, section->count);

    FfonElement *cbElem = section->elements[0];
    TEST_ASSERT_EQUAL_INT(FFON_STRING, cbElem->type);
    TEST_ASSERT_TRUE(providerTagHasCheckboxChecked(cbElem->data.string));

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_checkbox_renders_unchecked(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSectionCheckbox(p, "programs", "tutorial", "enable_tutorial", false);

    int count;
    FfonElement **elems = p->fetch(p, &count);

    FfonObject *section = elems[2]->data.object;
    FfonElement *cbElem = section->elements[0];
    TEST_ASSERT_TRUE(providerTagHasCheckbox(cbElem->data.string));
    TEST_ASSERT_FALSE(providerTagHasCheckboxChecked(cbElem->data.string));

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_onCheckboxChange_updates_state(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSectionCheckbox(p, "programs", "tutorial", "enable_tutorial", true);

    // Simulate unchecking
    p->onCheckboxChange(p, "tutorial", false);

    TEST_ASSERT_EQUAL_INT(1, callbackCount);
    TEST_ASSERT_EQUAL_STRING("enable_tutorial", lastCallbackKey);
    TEST_ASSERT_EQUAL_STRING("false", lastCallbackValue);

    // Fetch should now show unchecked
    int count;
    FfonElement **elems = p->fetch(p, &count);
    FfonObject *section = elems[2]->data.object;
    FfonElement *cbElem = section->elements[0];
    TEST_ASSERT_TRUE(providerTagHasCheckbox(cbElem->data.string));
    TEST_ASSERT_FALSE(providerTagHasCheckboxChecked(cbElem->data.string));

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_init_calls_callback_for_checkbox_entries(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSectionCheckbox(p, "programs", "tutorial", "enable_tutorial", true);
    settingsAddSectionCheckbox(p, "programs", "file browser", "enable_file browser", false);

    p->init(p);

    // colorScheme + 2 checkbox entries
    TEST_ASSERT_EQUAL_INT(3, callbackCount);
    TEST_ASSERT_EQUAL_STRING("colorScheme", callbackKeys[0]);
    TEST_ASSERT_EQUAL_STRING("enable_tutorial", callbackKeys[1]);
    TEST_ASSERT_EQUAL_STRING("true", callbackValues[1]);
    TEST_ASSERT_EQUAL_STRING("enable_file browser", callbackKeys[2]);
    TEST_ASSERT_EQUAL_STRING("false", callbackValues[2]);

    free(p->state);
    free(p);
}

// --- settingsSetCheckboxState ---

void test_setCheckboxState_updates_without_callback(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSectionCheckbox(p, "sicompass", "maximized", "maximized", false);

    callbackCount = 0;
    settingsSetCheckboxState(p, "maximized", true);

    // Should NOT have called the apply callback
    TEST_ASSERT_EQUAL_INT(0, callbackCount);

    // Fetch should show checked
    int count;
    FfonElement **elems = p->fetch(p, &count);
    FfonObject *section = elems[1]->data.object;  // sicompass section
    // Find the checkbox element
    bool foundChecked = false;
    for (int i = 0; i < section->count; i++) {
        if (section->elements[i]->type == FFON_STRING &&
            providerTagHasCheckboxChecked(section->elements[i]->data.string)) {
            foundChecked = true;
            break;
        }
    }
    TEST_ASSERT_TRUE(foundChecked);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_setCheckboxState_no_change_skips(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSectionCheckbox(p, "sicompass", "maximized", "maximized", false);

    callbackCount = 0;
    settingsSetCheckboxState(p, "maximized", false);  // already false

    TEST_ASSERT_EQUAL_INT(0, callbackCount);

    free(p->state);
    free(p);
}

// --- settingsRemoveSection ---

void test_removeSection_removes_from_fetch(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSection(p, "file browser");
    settingsRemoveSection(p, "file browser");

    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(2, count);  // only sicompass remains
    TEST_ASSERT_EQUAL_STRING("sicompass", elems[1]->data.object->key);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_removeSection_removes_radio_entries(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    const char *options[] = { "alpha", "chrono" };
    settingsAddSectionRadio(p, "file browser", "sorting", "sortOrder",
                            options, 2, "alpha");
    settingsRemoveSection(p, "file browser");

    // Re-add the section empty to verify radio entries are gone
    settingsAddSection(p, "file browser");
    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(3, count);
    FfonObject *fb = elems[2]->data.object;
    TEST_ASSERT_EQUAL_STRING("file browser", fb->key);
    TEST_ASSERT_EQUAL_INT(1, fb->count);
    TEST_ASSERT_EQUAL_STRING("no settings", fb->elements[0]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_removeSection_removes_text_entries(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSectionText(p, "sales demo", "save folder", "saveFolder", "Downloads");
    settingsRemoveSection(p, "sales demo");

    // Re-add section to verify text entries are gone
    settingsAddSection(p, "sales demo");
    int count;
    FfonElement **elems = p->fetch(p, &count);
    FfonObject *sd = elems[2]->data.object;
    TEST_ASSERT_EQUAL_STRING("sales demo", sd->key);
    TEST_ASSERT_EQUAL_INT(1, sd->count);
    TEST_ASSERT_EQUAL_STRING("no settings", sd->elements[0]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_removeSection_removes_checkbox_entries(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSectionCheckbox(p, "programs", "tutorial", "enable_tutorial", true);
    settingsRemoveSection(p, "programs");

    // Re-add section to verify checkbox entries are gone
    settingsAddSection(p, "programs");
    int count;
    FfonElement **elems = p->fetch(p, &count);
    FfonObject *section = elems[2]->data.object;
    TEST_ASSERT_EQUAL_STRING("programs", section->key);
    TEST_ASSERT_EQUAL_INT(1, section->count);
    TEST_ASSERT_EQUAL_STRING("no settings", section->elements[0]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_removeSection_null_params(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);
    settingsRemoveSection(NULL, "test");      // should not crash
    settingsRemoveSection(p, NULL);           // should not crash
    free(p->state);
    free(p);
}

void test_removeSection_nonexistent(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    settingsAddSection(p, "file browser");
    settingsRemoveSection(p, "nonexistent");

    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(3, count);  // sicompass + file browser still present

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

void test_removeSection_leaves_other_sections(void) {
    Provider *p = settingsProviderCreate(testApplyCallback, NULL);

    const char *options[] = { "a", "b" };
    settingsAddSectionRadio(p, "section A", "radio", "key", options, 2, "a");
    settingsAddSectionText(p, "section B", "label", "textKey", "value");
    settingsRemoveSection(p, "section A");

    int count;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(3, count);  // sicompass + section B
    TEST_ASSERT_EQUAL_STRING("section B", elems[2]->data.object->key);
    // section B should still have its text entry
    FfonObject *sectionB = elems[2]->data.object;
    TEST_ASSERT_EQUAL_INT(1, sectionB->count);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, sectionB->elements[0]->type);
    TEST_ASSERT_TRUE(providerTagHasInput(sectionB->elements[0]->data.string));

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    free(p->state);
    free(p);
}

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_create_with_callback);
    RUN_TEST(test_create_with_null_callback);

    RUN_TEST(test_addSection_one);
    RUN_TEST(test_addSection_null_provider);
    RUN_TEST(test_addSection_null_name);

    RUN_TEST(test_addSectionRadio);
    RUN_TEST(test_addSectionRadio_auto_creates_section);

    RUN_TEST(test_fetch_default_color_scheme);

    RUN_TEST(test_path_push_pop);
    RUN_TEST(test_path_popPath_at_root);

    RUN_TEST(test_onRadioChange_color_scheme);
    RUN_TEST(test_onRadioChange_custom_radio);
    RUN_TEST(test_onRadioChange_null_callback);

    RUN_TEST(test_init_calls_callback_for_defaults);

    RUN_TEST(test_addSectionText);
    RUN_TEST(test_addSectionText_auto_creates_section);
    RUN_TEST(test_init_calls_callback_for_text_entries);
    RUN_TEST(test_commitEdit_text_entry);
    RUN_TEST(test_commitEdit_wrong_path_returns_false);
    RUN_TEST(test_section_with_radio_and_text);

    RUN_TEST(test_prioritySection_renders_first);
    RUN_TEST(test_prioritySection_not_duplicated);
    RUN_TEST(test_checkbox_renders_checked);
    RUN_TEST(test_checkbox_renders_unchecked);
    RUN_TEST(test_onCheckboxChange_updates_state);
    RUN_TEST(test_init_calls_callback_for_checkbox_entries);
    RUN_TEST(test_setCheckboxState_updates_without_callback);
    RUN_TEST(test_setCheckboxState_no_change_skips);

    RUN_TEST(test_removeSection_removes_from_fetch);
    RUN_TEST(test_removeSection_removes_radio_entries);
    RUN_TEST(test_removeSection_removes_text_entries);
    RUN_TEST(test_removeSection_removes_checkbox_entries);
    RUN_TEST(test_removeSection_null_params);
    RUN_TEST(test_removeSection_nonexistent);
    RUN_TEST(test_removeSection_leaves_other_sections);

    return UNITY_END();
}

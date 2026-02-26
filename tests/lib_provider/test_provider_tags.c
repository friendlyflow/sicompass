/*
 * Tests for provider tag functions.
 * Functions under test: all functions in provider_tags.h
 */

#include <unity.h>
#include <provider_tags.h>
#include <stdlib.h>
#include <string.h>

void setUp(void) {}
void tearDown(void) {}

// === Input tags ===

void test_hasInput_with_tags(void) {
    TEST_ASSERT_TRUE(providerTagHasInput("<input>test</input>"));
}

void test_hasInput_without_tags(void) {
    TEST_ASSERT_FALSE(providerTagHasInput("no tags here"));
}

void test_hasInput_null(void) {
    TEST_ASSERT_FALSE(providerTagHasInput(NULL));
}

void test_hasInput_open_only(void) {
    TEST_ASSERT_FALSE(providerTagHasInput("<input>no close"));
}

void test_extractContent_normal(void) {
    char *content = providerTagExtractContent("<input>hello</input>");
    TEST_ASSERT_EQUAL_STRING("hello", content);
    free(content);
}

void test_extractContent_with_prefix(void) {
    char *content = providerTagExtractContent("prefix <input>val</input> suffix");
    TEST_ASSERT_EQUAL_STRING("val", content);
    free(content);
}

void test_extractContent_null(void) {
    TEST_ASSERT_NULL(providerTagExtractContent(NULL));
}

void test_extractContent_no_tags(void) {
    TEST_ASSERT_NULL(providerTagExtractContent("no tags"));
}

void test_stripDisplay_input(void) {
    char *result = providerTagStripDisplay("<input>filename.txt</input>");
    TEST_ASSERT_EQUAL_STRING("filename.txt", result);
    free(result);
}

void test_stripDisplay_no_tags(void) {
    char *result = providerTagStripDisplay("plain text");
    TEST_ASSERT_EQUAL_STRING("plain text", result);
    free(result);
}

void test_stripDisplay_null(void) {
    TEST_ASSERT_NULL(providerTagStripDisplay(NULL));
}

void test_formatKey_normal(void) {
    char *result = providerTagFormatKey("content");
    TEST_ASSERT_EQUAL_STRING("<input>content</input>", result);
    free(result);
}

void test_formatKey_null(void) {
    TEST_ASSERT_NULL(providerTagFormatKey(NULL));
}

// === Radio tags ===

void test_hasRadio_with_tag(void) {
    TEST_ASSERT_TRUE(providerTagHasRadio("<radio>group</radio>"));
}

void test_hasRadio_open_only(void) {
    TEST_ASSERT_TRUE(providerTagHasRadio("<radio>group"));
}

void test_hasRadio_without_tag(void) {
    TEST_ASSERT_FALSE(providerTagHasRadio("no radio"));
}

void test_hasRadio_null(void) {
    TEST_ASSERT_FALSE(providerTagHasRadio(NULL));
}

void test_extractRadioContent_with_close(void) {
    char *content = providerTagExtractRadioContent("<radio>group name</radio>");
    TEST_ASSERT_EQUAL_STRING("group name", content);
    free(content);
}

void test_extractRadioContent_without_close(void) {
    char *content = providerTagExtractRadioContent("<radio>group name");
    TEST_ASSERT_EQUAL_STRING("group name", content);
    free(content);
}

void test_extractRadioContent_null(void) {
    TEST_ASSERT_NULL(providerTagExtractRadioContent(NULL));
}

void test_stripDisplay_radio(void) {
    char *result = providerTagStripDisplay("<radio>color scheme</radio>");
    TEST_ASSERT_EQUAL_STRING("color scheme", result);
    free(result);
}

// === Checked tags ===

void test_hasChecked_with_tag(void) {
    TEST_ASSERT_TRUE(providerTagHasChecked("<checked>item"));
}

void test_hasChecked_without_tag(void) {
    TEST_ASSERT_FALSE(providerTagHasChecked("item"));
}

void test_hasChecked_null(void) {
    TEST_ASSERT_FALSE(providerTagHasChecked(NULL));
}

void test_extractCheckedContent_with_close(void) {
    char *content = providerTagExtractCheckedContent("<checked>dark</checked>");
    TEST_ASSERT_EQUAL_STRING("dark", content);
    free(content);
}

void test_extractCheckedContent_without_close(void) {
    char *content = providerTagExtractCheckedContent("<checked>dark");
    TEST_ASSERT_EQUAL_STRING("dark", content);
    free(content);
}

void test_extractCheckedContent_null(void) {
    TEST_ASSERT_NULL(providerTagExtractCheckedContent(NULL));
}

void test_formatCheckedKey(void) {
    char *result = providerTagFormatCheckedKey("dark");
    TEST_ASSERT_EQUAL_STRING("<checked>dark", result);
    free(result);
}

void test_formatCheckedKey_null(void) {
    TEST_ASSERT_NULL(providerTagFormatCheckedKey(NULL));
}

void test_stripDisplay_checked(void) {
    char *result = providerTagStripDisplay("<checked>dark</checked>");
    TEST_ASSERT_EQUAL_STRING("dark", result);
    free(result);
}

// === Checkbox tags ===

void test_hasCheckbox_unchecked(void) {
    TEST_ASSERT_TRUE(providerTagHasCheckbox("<checkbox>item"));
}

void test_hasCheckbox_checked_returns_false(void) {
    TEST_ASSERT_FALSE(providerTagHasCheckbox("<checkbox checked>item"));
}

void test_hasCheckbox_null(void) {
    TEST_ASSERT_FALSE(providerTagHasCheckbox(NULL));
}

void test_hasCheckboxChecked_with_tag(void) {
    TEST_ASSERT_TRUE(providerTagHasCheckboxChecked("<checkbox checked>item"));
}

void test_hasCheckboxChecked_unchecked_returns_false(void) {
    TEST_ASSERT_FALSE(providerTagHasCheckboxChecked("<checkbox>item"));
}

void test_hasCheckboxChecked_null(void) {
    TEST_ASSERT_FALSE(providerTagHasCheckboxChecked(NULL));
}

void test_extractCheckboxContent(void) {
    char *content = providerTagExtractCheckboxContent("<checkbox>task 1");
    TEST_ASSERT_EQUAL_STRING("task 1", content);
    free(content);
}

void test_extractCheckboxCheckedContent(void) {
    char *content = providerTagExtractCheckboxCheckedContent("<checkbox checked>task 1");
    TEST_ASSERT_EQUAL_STRING("task 1", content);
    free(content);
}

void test_formatCheckboxKey(void) {
    char *result = providerTagFormatCheckboxKey("task 1");
    TEST_ASSERT_EQUAL_STRING("<checkbox>task 1", result);
    free(result);
}

void test_formatCheckboxCheckedKey(void) {
    char *result = providerTagFormatCheckboxCheckedKey("task 1");
    TEST_ASSERT_EQUAL_STRING("<checkbox checked>task 1", result);
    free(result);
}

void test_stripDisplay_checkbox_unchecked(void) {
    char *result = providerTagStripDisplay("<checkbox>task</checkbox>");
    TEST_ASSERT_EQUAL_STRING("task", result);
    free(result);
}

void test_stripDisplay_checkbox_checked(void) {
    char *result = providerTagStripDisplay("<checkbox checked>task</checkbox>");
    TEST_ASSERT_EQUAL_STRING("task", result);
    free(result);
}

// === Link tags ===

void test_hasLink_with_tags(void) {
    TEST_ASSERT_TRUE(providerTagHasLink("<link>path/to/file</link>"));
}

void test_hasLink_without_tags(void) {
    TEST_ASSERT_FALSE(providerTagHasLink("no link"));
}

void test_hasLink_null(void) {
    TEST_ASSERT_FALSE(providerTagHasLink(NULL));
}

void test_extractLinkContent(void) {
    char *content = providerTagExtractLinkContent("<link>assets/sf.json</link>");
    TEST_ASSERT_EQUAL_STRING("assets/sf.json", content);
    free(content);
}

void test_extractLinkContent_null(void) {
    TEST_ASSERT_NULL(providerTagExtractLinkContent(NULL));
}

void test_extractLinkContent_no_tags(void) {
    TEST_ASSERT_NULL(providerTagExtractLinkContent("no tags"));
}

void test_stripDisplay_link(void) {
    char *result = providerTagStripDisplay("<link>assets/sf.json</link>");
    TEST_ASSERT_EQUAL_STRING("assets/sf.json", result);
    free(result);
}

// === Image tags ===

void test_hasImage_with_tags(void) {
    TEST_ASSERT_TRUE(providerTagHasImage("<image>photo.jpg</image>"));
}

void test_hasImage_without_tags(void) {
    TEST_ASSERT_FALSE(providerTagHasImage("no image"));
}

void test_hasImage_null(void) {
    TEST_ASSERT_FALSE(providerTagHasImage(NULL));
}

void test_extractImageContent(void) {
    char *content = providerTagExtractImageContent("<image>textures/texture.jpg</image>");
    TEST_ASSERT_EQUAL_STRING("textures/texture.jpg", content);
    free(content);
}

void test_extractImageContent_null(void) {
    TEST_ASSERT_NULL(providerTagExtractImageContent(NULL));
}

void test_stripDisplay_image(void) {
    char *result = providerTagStripDisplay("<image>photo.jpg</image>");
    TEST_ASSERT_EQUAL_STRING("photo.jpg", result);
    free(result);
}

// === Opt tags ===

void test_hasOpt_with_tag(void) {
    TEST_ASSERT_TRUE(providerTagHasOpt("<opt></opt>some content"));
}

void test_hasOpt_without_tag(void) {
    TEST_ASSERT_FALSE(providerTagHasOpt("no opt"));
}

void test_hasOpt_null(void) {
    TEST_ASSERT_FALSE(providerTagHasOpt(NULL));
}

void test_hasOneOpt_with_tag(void) {
    TEST_ASSERT_TRUE(providerTagHasOneOpt("<one-opt></one-opt>content"));
}

void test_hasOneOpt_without_tag(void) {
    TEST_ASSERT_FALSE(providerTagHasOneOpt("no tag"));
}

void test_hasOneOpt_null(void) {
    TEST_ASSERT_FALSE(providerTagHasOneOpt(NULL));
}

void test_stripOneOpt_with_tag(void) {
    char *result = providerTagStripOneOpt("<one-opt></one-opt>content here");
    TEST_ASSERT_EQUAL_STRING("content here", result);
    free(result);
}

void test_stripOneOpt_without_tag(void) {
    char *result = providerTagStripOneOpt("no tag");
    TEST_ASSERT_EQUAL_STRING("no tag", result);
    free(result);
}

void test_stripOneOpt_null(void) {
    TEST_ASSERT_NULL(providerTagStripOneOpt(NULL));
}

// === Button tags ===

void test_hasButton_with_tags(void) {
    TEST_ASSERT_TRUE(providerTagHasButton("<button>funcName</button>Display"));
}

void test_hasButton_without_tags(void) {
    TEST_ASSERT_FALSE(providerTagHasButton("no button"));
}

void test_hasButton_null(void) {
    TEST_ASSERT_FALSE(providerTagHasButton(NULL));
}

void test_extractButtonFunctionName(void) {
    char *name = providerTagExtractButtonFunctionName("<button>addItem</button>Add Item");
    TEST_ASSERT_EQUAL_STRING("addItem", name);
    free(name);
}

void test_extractButtonFunctionName_null(void) {
    TEST_ASSERT_NULL(providerTagExtractButtonFunctionName(NULL));
}

void test_extractButtonDisplayText(void) {
    char *text = providerTagExtractButtonDisplayText("<button>addItem</button>Add Item");
    TEST_ASSERT_EQUAL_STRING("Add Item", text);
    free(text);
}

void test_extractButtonDisplayText_null(void) {
    TEST_ASSERT_NULL(providerTagExtractButtonDisplayText(NULL));
}

void test_stripDisplay_button(void) {
    char *result = providerTagStripDisplay("<button>func</button>Display Text");
    TEST_ASSERT_EQUAL_STRING("Display Text", result);
    free(result);
}

// === Escape sequences ===

void test_escaped_input_not_detected(void) {
    TEST_ASSERT_FALSE(providerTagHasInput("\\<input>text\\</input>"));
}

void test_escaped_radio_not_detected(void) {
    TEST_ASSERT_FALSE(providerTagHasRadio("\\<radio>group"));
}

void test_escaped_checked_not_detected(void) {
    TEST_ASSERT_FALSE(providerTagHasChecked("\\<checked>option"));
}

void test_escaped_checkbox_not_detected(void) {
    TEST_ASSERT_FALSE(providerTagHasCheckbox("\\<checkbox>label"));
}

void test_escaped_checkbox_checked_not_detected(void) {
    TEST_ASSERT_FALSE(providerTagHasCheckboxChecked("\\<checkbox checked>label"));
}

void test_escaped_link_not_detected(void) {
    TEST_ASSERT_FALSE(providerTagHasLink("\\<link>file.json\\</link>"));
}

void test_escaped_image_not_detected(void) {
    TEST_ASSERT_FALSE(providerTagHasImage("\\<image>pic.jpg\\</image>"));
}

void test_escaped_button_not_detected(void) {
    TEST_ASSERT_FALSE(providerTagHasButton("\\<button>fn\\</button>text"));
}

void test_stripDisplay_unescapes_input(void) {
    char *result = providerTagStripDisplay("\\<input>content\\</input>");
    TEST_ASSERT_EQUAL_STRING("<input>content</input>", result);
    free(result);
}

void test_stripDisplay_unescapes_checkbox(void) {
    char *result = providerTagStripDisplay("\\<checkbox>label");
    TEST_ASSERT_EQUAL_STRING("<checkbox>label", result);
    free(result);
}

void test_stripDisplay_unescapes_mixed(void) {
    // Real tag + escaped text in same string
    char *result = providerTagStripDisplay("prefix \\<b\\> <input>editable</input> suffix");
    TEST_ASSERT_EQUAL_STRING("prefix <b> editable suffix", result);
    free(result);
}

void test_stripDisplay_no_escape_passes_through(void) {
    // Normal text without escapes still works
    char *result = providerTagStripDisplay("plain text");
    TEST_ASSERT_EQUAL_STRING("plain text", result);
    free(result);
}

void test_real_tags_still_work(void) {
    // Unescaped tags still detected
    TEST_ASSERT_TRUE(providerTagHasInput("<input>test</input>"));
    TEST_ASSERT_TRUE(providerTagHasRadio("<radio>group"));
    TEST_ASSERT_TRUE(providerTagHasCheckbox("<checkbox>label"));
    TEST_ASSERT_TRUE(providerTagHasLink("<link>f.json</link>"));
    TEST_ASSERT_TRUE(providerTagHasImage("<image>p.jpg</image>"));
    TEST_ASSERT_TRUE(providerTagHasButton("<button>fn</button>text"));
}

int main(void) {
    UNITY_BEGIN();

    // Input tags
    RUN_TEST(test_hasInput_with_tags);
    RUN_TEST(test_hasInput_without_tags);
    RUN_TEST(test_hasInput_null);
    RUN_TEST(test_hasInput_open_only);
    RUN_TEST(test_extractContent_normal);
    RUN_TEST(test_extractContent_with_prefix);
    RUN_TEST(test_extractContent_null);
    RUN_TEST(test_extractContent_no_tags);
    RUN_TEST(test_stripDisplay_input);
    RUN_TEST(test_stripDisplay_no_tags);
    RUN_TEST(test_stripDisplay_null);
    RUN_TEST(test_formatKey_normal);
    RUN_TEST(test_formatKey_null);

    // Radio tags
    RUN_TEST(test_hasRadio_with_tag);
    RUN_TEST(test_hasRadio_open_only);
    RUN_TEST(test_hasRadio_without_tag);
    RUN_TEST(test_hasRadio_null);
    RUN_TEST(test_extractRadioContent_with_close);
    RUN_TEST(test_extractRadioContent_without_close);
    RUN_TEST(test_extractRadioContent_null);
    RUN_TEST(test_stripDisplay_radio);

    // Checked tags
    RUN_TEST(test_hasChecked_with_tag);
    RUN_TEST(test_hasChecked_without_tag);
    RUN_TEST(test_hasChecked_null);
    RUN_TEST(test_extractCheckedContent_with_close);
    RUN_TEST(test_extractCheckedContent_without_close);
    RUN_TEST(test_extractCheckedContent_null);
    RUN_TEST(test_formatCheckedKey);
    RUN_TEST(test_formatCheckedKey_null);
    RUN_TEST(test_stripDisplay_checked);

    // Checkbox tags
    RUN_TEST(test_hasCheckbox_unchecked);
    RUN_TEST(test_hasCheckbox_checked_returns_false);
    RUN_TEST(test_hasCheckbox_null);
    RUN_TEST(test_hasCheckboxChecked_with_tag);
    RUN_TEST(test_hasCheckboxChecked_unchecked_returns_false);
    RUN_TEST(test_hasCheckboxChecked_null);
    RUN_TEST(test_extractCheckboxContent);
    RUN_TEST(test_extractCheckboxCheckedContent);
    RUN_TEST(test_formatCheckboxKey);
    RUN_TEST(test_formatCheckboxCheckedKey);
    RUN_TEST(test_stripDisplay_checkbox_unchecked);
    RUN_TEST(test_stripDisplay_checkbox_checked);

    // Link tags
    RUN_TEST(test_hasLink_with_tags);
    RUN_TEST(test_hasLink_without_tags);
    RUN_TEST(test_hasLink_null);
    RUN_TEST(test_extractLinkContent);
    RUN_TEST(test_extractLinkContent_null);
    RUN_TEST(test_extractLinkContent_no_tags);
    RUN_TEST(test_stripDisplay_link);

    // Image tags
    RUN_TEST(test_hasImage_with_tags);
    RUN_TEST(test_hasImage_without_tags);
    RUN_TEST(test_hasImage_null);
    RUN_TEST(test_extractImageContent);
    RUN_TEST(test_extractImageContent_null);
    RUN_TEST(test_stripDisplay_image);

    // Opt tags
    RUN_TEST(test_hasOpt_with_tag);
    RUN_TEST(test_hasOpt_without_tag);
    RUN_TEST(test_hasOpt_null);
    RUN_TEST(test_hasOneOpt_with_tag);
    RUN_TEST(test_hasOneOpt_without_tag);
    RUN_TEST(test_hasOneOpt_null);
    RUN_TEST(test_stripOneOpt_with_tag);
    RUN_TEST(test_stripOneOpt_without_tag);
    RUN_TEST(test_stripOneOpt_null);

    // Button tags
    RUN_TEST(test_hasButton_with_tags);
    RUN_TEST(test_hasButton_without_tags);
    RUN_TEST(test_hasButton_null);
    RUN_TEST(test_extractButtonFunctionName);
    RUN_TEST(test_extractButtonFunctionName_null);
    RUN_TEST(test_extractButtonDisplayText);
    RUN_TEST(test_extractButtonDisplayText_null);
    RUN_TEST(test_stripDisplay_button);

    // Escape sequences
    RUN_TEST(test_escaped_input_not_detected);
    RUN_TEST(test_escaped_radio_not_detected);
    RUN_TEST(test_escaped_checked_not_detected);
    RUN_TEST(test_escaped_checkbox_not_detected);
    RUN_TEST(test_escaped_checkbox_checked_not_detected);
    RUN_TEST(test_escaped_link_not_detected);
    RUN_TEST(test_escaped_image_not_detected);
    RUN_TEST(test_escaped_button_not_detected);
    RUN_TEST(test_stripDisplay_unescapes_input);
    RUN_TEST(test_stripDisplay_unescapes_checkbox);
    RUN_TEST(test_stripDisplay_unescapes_mixed);
    RUN_TEST(test_stripDisplay_no_escape_passes_through);
    RUN_TEST(test_real_tags_still_work);

    return UNITY_END();
}

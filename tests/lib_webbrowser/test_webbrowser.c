#include <unity.h>
#include <webbrowser.h>
#include <ffon.h>
#include <string.h>
#include <stdlib.h>

void setUp(void) {}
void tearDown(void) {}

// Helper to wrap HTML body content in a full document
static char* wrapHtml(const char *body) {
    size_t len = strlen(body) + 64;
    char *html = malloc(len);
    snprintf(html, len, "<html><body>%s</body></html>", body);
    return html;
}

// --- Basic element tests ---

void test_simple_paragraph(void) {
    char *html = wrapHtml("<p>Hello world</p>");
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    TEST_ASSERT_EQUAL_INT(1, count);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, elems[0]->type);
    TEST_ASSERT_EQUAL_STRING("Hello world", elems[0]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_heading_creates_object(void) {
    char *html = wrapHtml("<h1>Title</h1>");
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    TEST_ASSERT_EQUAL_INT(1, count);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, elems[0]->type);
    TEST_ASSERT_EQUAL_STRING("Title", elems[0]->data.object->key);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_heading_with_paragraph_child(void) {
    char *html = wrapHtml("<h2>Section</h2><p>Text</p>");
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    TEST_ASSERT_EQUAL_INT(1, count);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, elems[0]->type);
    TEST_ASSERT_EQUAL_STRING("Section", elems[0]->data.object->key);
    TEST_ASSERT_EQUAL_INT(1, elems[0]->data.object->count);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, elems[0]->data.object->elements[0]->type);
    TEST_ASSERT_EQUAL_STRING("Text", elems[0]->data.object->elements[0]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_heading_nesting_h2_h3(void) {
    char *html = wrapHtml("<h2>A</h2><h3>B</h3><p>C</p>");
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    // Root: Object("A") containing Object("B") containing String("C")
    TEST_ASSERT_EQUAL_INT(1, count);
    FfonObject *a = elems[0]->data.object;
    TEST_ASSERT_EQUAL_STRING("A", a->key);
    TEST_ASSERT_EQUAL_INT(1, a->count);

    FfonObject *b = a->elements[0]->data.object;
    TEST_ASSERT_EQUAL_STRING("B", b->key);
    TEST_ASSERT_EQUAL_INT(1, b->count);
    TEST_ASSERT_EQUAL_STRING("C", b->elements[0]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_heading_sibling_same_level(void) {
    char *html = wrapHtml("<h2>A</h2><p>X</p><h2>B</h2><p>Y</p>");
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    TEST_ASSERT_EQUAL_INT(2, count);
    TEST_ASSERT_EQUAL_STRING("A", elems[0]->data.object->key);
    TEST_ASSERT_EQUAL_STRING("X", elems[0]->data.object->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("B", elems[1]->data.object->key);
    TEST_ASSERT_EQUAL_STRING("Y", elems[1]->data.object->elements[0]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_heading_pop_on_equal(void) {
    // h2 > h3, h3 — two h3 siblings under h2
    char *html = wrapHtml("<h2>A</h2><h3>B</h3><h3>C</h3>");
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    TEST_ASSERT_EQUAL_INT(1, count);
    FfonObject *a = elems[0]->data.object;
    TEST_ASSERT_EQUAL_STRING("A", a->key);
    TEST_ASSERT_EQUAL_INT(2, a->count);
    TEST_ASSERT_EQUAL_STRING("B", a->elements[0]->data.object->key);
    TEST_ASSERT_EQUAL_STRING("C", a->elements[1]->data.object->key);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_heading_deep_nesting(void) {
    char *html = wrapHtml(
        "<h2>L2</h2><h3>L3</h3><h4>L4</h4><h5>L5</h5><h6>L6</h6><p>leaf</p>");
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    TEST_ASSERT_EQUAL_INT(1, count);
    FfonObject *cur = elems[0]->data.object;
    TEST_ASSERT_EQUAL_STRING("L2", cur->key);
    cur = cur->elements[0]->data.object;
    TEST_ASSERT_EQUAL_STRING("L3", cur->key);
    cur = cur->elements[0]->data.object;
    TEST_ASSERT_EQUAL_STRING("L4", cur->key);
    cur = cur->elements[0]->data.object;
    TEST_ASSERT_EQUAL_STRING("L5", cur->key);
    cur = cur->elements[0]->data.object;
    TEST_ASSERT_EQUAL_STRING("L6", cur->key);
    TEST_ASSERT_EQUAL_INT(1, cur->count);
    TEST_ASSERT_EQUAL_STRING("leaf", cur->elements[0]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_heading_skip_level(void) {
    // h2 then h4 (skipping h3) — h4 nests under h2
    char *html = wrapHtml("<h2>A</h2><h4>B</h4><p>C</p>");
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    TEST_ASSERT_EQUAL_INT(1, count);
    FfonObject *a = elems[0]->data.object;
    TEST_ASSERT_EQUAL_STRING("A", a->key);
    TEST_ASSERT_EQUAL_INT(1, a->count);
    FfonObject *b = a->elements[0]->data.object;
    TEST_ASSERT_EQUAL_STRING("B", b->key);
    TEST_ASSERT_EQUAL_INT(1, b->count);
    TEST_ASSERT_EQUAL_STRING("C", b->elements[0]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

// --- List tests ---

void test_unordered_list(void) {
    char *html = wrapHtml("<ul><li>Apple</li><li>Banana</li></ul>");
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    TEST_ASSERT_EQUAL_INT(1, count);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, elems[0]->type);
    TEST_ASSERT_EQUAL_STRING("list", elems[0]->data.object->key);
    TEST_ASSERT_EQUAL_INT(2, elems[0]->data.object->count);
    TEST_ASSERT_EQUAL_STRING("Apple", elems[0]->data.object->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("Banana", elems[0]->data.object->elements[1]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_ordered_list(void) {
    char *html = wrapHtml("<ol><li>First</li><li>Second</li></ol>");
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    TEST_ASSERT_EQUAL_INT(1, count);
    TEST_ASSERT_EQUAL_STRING("ordered list", elems[0]->data.object->key);
    TEST_ASSERT_EQUAL_INT(2, elems[0]->data.object->count);
    TEST_ASSERT_EQUAL_STRING("1. First", elems[0]->data.object->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("2. Second", elems[0]->data.object->elements[1]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

// --- Table test ---

void test_table(void) {
    char *html = wrapHtml("<table><tr><td>A</td><td>B</td></tr><tr><td>C</td><td>D</td></tr></table>");
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    TEST_ASSERT_EQUAL_INT(1, count);
    TEST_ASSERT_EQUAL_STRING("table", elems[0]->data.object->key);
    TEST_ASSERT_EQUAL_INT(2, elems[0]->data.object->count);
    TEST_ASSERT_EQUAL_STRING("A | B", elems[0]->data.object->elements[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("C | D", elems[0]->data.object->elements[1]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

// --- Skip tests ---

void test_script_and_style_skipped(void) {
    char *html = wrapHtml(
        "<script>var x=1;</script><style>.a{}</style><p>Visible</p>");
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    TEST_ASSERT_EQUAL_INT(1, count);
    TEST_ASSERT_EQUAL_STRING("Visible", elems[0]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_empty_html(void) {
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon("<html><body></body></html>",
                                               "https://example.com", &count);
    TEST_ASSERT_EQUAL_INT(0, count);
    free(elems);
}

// --- Link test ---

void test_link_in_paragraph(void) {
    char *html = wrapHtml("<p>See <a href=\"https://x.com\">here</a></p>");
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    TEST_ASSERT_EQUAL_INT(1, count);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, elems[0]->type);
    // Should contain <link> tag
    TEST_ASSERT_NOT_NULL(strstr(elems[0]->data.string, "<link>https://x.com</link>"));
    TEST_ASSERT_NOT_NULL(strstr(elems[0]->data.string, "here"));

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

// --- Complex structure test ---

void test_full_document_structure(void) {
    char *html = wrapHtml(
        "<h2>Installation</h2>"
        "<p>Run the installer</p>"
        "<h3>Linux</h3>"
        "<p>Use apt</p>"
        "<h3>macOS</h3>"
        "<p>Use brew</p>"
        "<h2>Usage</h2>"
        "<p>Run the program</p>"
    );
    int count = 0;
    FfonElement **elems = webbrowserHtmlToFfon(html, "https://example.com", &count);
    free(html);

    TEST_ASSERT_EQUAL_INT(2, count);

    // Installation
    FfonObject *install = elems[0]->data.object;
    TEST_ASSERT_EQUAL_STRING("Installation", install->key);
    TEST_ASSERT_EQUAL_INT(3, install->count); // "Run the installer", Linux, macOS

    TEST_ASSERT_EQUAL_STRING("Run the installer", install->elements[0]->data.string);

    FfonObject *linux_obj = install->elements[1]->data.object;
    TEST_ASSERT_EQUAL_STRING("Linux", linux_obj->key);
    TEST_ASSERT_EQUAL_INT(1, linux_obj->count);
    TEST_ASSERT_EQUAL_STRING("Use apt", linux_obj->elements[0]->data.string);

    FfonObject *macos_obj = install->elements[2]->data.object;
    TEST_ASSERT_EQUAL_STRING("macOS", macos_obj->key);
    TEST_ASSERT_EQUAL_INT(1, macos_obj->count);
    TEST_ASSERT_EQUAL_STRING("Use brew", macos_obj->elements[0]->data.string);

    // Usage
    FfonObject *usage = elems[1]->data.object;
    TEST_ASSERT_EQUAL_STRING("Usage", usage->key);
    TEST_ASSERT_EQUAL_INT(1, usage->count);
    TEST_ASSERT_EQUAL_STRING("Run the program", usage->elements[0]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_null_and_empty_input(void) {
    int count = 99;
    FfonElement **elems = webbrowserHtmlToFfon(NULL, "https://example.com", &count);
    TEST_ASSERT_NULL(elems);
    TEST_ASSERT_EQUAL_INT(0, count);

    count = 99;
    elems = webbrowserHtmlToFfon("", "https://example.com", &count);
    TEST_ASSERT_NULL(elems);
    TEST_ASSERT_EQUAL_INT(0, count);
}

int main(void) {
    UNITY_BEGIN();
    RUN_TEST(test_simple_paragraph);
    RUN_TEST(test_heading_creates_object);
    RUN_TEST(test_heading_with_paragraph_child);
    RUN_TEST(test_heading_nesting_h2_h3);
    RUN_TEST(test_heading_sibling_same_level);
    RUN_TEST(test_heading_pop_on_equal);
    RUN_TEST(test_heading_deep_nesting);
    RUN_TEST(test_heading_skip_level);
    RUN_TEST(test_unordered_list);
    RUN_TEST(test_ordered_list);
    RUN_TEST(test_table);
    RUN_TEST(test_script_and_style_skipped);
    RUN_TEST(test_empty_html);
    RUN_TEST(test_link_in_paragraph);
    RUN_TEST(test_full_document_structure);
    RUN_TEST(test_null_and_empty_input);
    return UNITY_END();
}

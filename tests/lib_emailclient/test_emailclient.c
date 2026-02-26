#include <unity.h>
#include <provider_interface.h>
#include <provider_tags.h>
#include <ffon.h>
#include <stdlib.h>
#include <string.h>

void setUp(void) {}
void tearDown(void) {}

// --- Factory registration ---

void test_factory_creates_provider(void) {
    Provider *p = providerFactoryCreate("email client");
    TEST_ASSERT_NOT_NULL(p);
    TEST_ASSERT_EQUAL_STRING("emailclient", p->name);
}

void test_factory_returns_singleton(void) {
    Provider *p1 = providerFactoryCreate("email client");
    Provider *p2 = providerFactoryCreate("email client");
    TEST_ASSERT_EQUAL_PTR(p1, p2);
}

// --- Provider function pointers ---

void test_provider_has_required_functions(void) {
    Provider *p = providerFactoryCreate("email client");
    TEST_ASSERT_NOT_NULL(p->fetch);
    TEST_ASSERT_NOT_NULL(p->init);
    TEST_ASSERT_NOT_NULL(p->pushPath);
    TEST_ASSERT_NOT_NULL(p->popPath);
    TEST_ASSERT_NOT_NULL(p->getCurrentPath);
    TEST_ASSERT_NOT_NULL(p->getCommands);
    TEST_ASSERT_NOT_NULL(p->handleCommand);
}

void test_provider_has_commit(void) {
    Provider *p = providerFactoryCreate("email client");
    TEST_ASSERT_NOT_NULL(p->commitEdit);
}

// --- Path management ---

void test_init_sets_root_path(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    TEST_ASSERT_EQUAL_STRING("/", p->getCurrentPath(p));
}

void test_push_pop_path(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    p->pushPath(p, "INBOX");
    TEST_ASSERT_EQUAL_STRING("/INBOX", p->getCurrentPath(p));
    p->popPath(p);
    TEST_ASSERT_EQUAL_STRING("/", p->getCurrentPath(p));
}

void test_push_two_levels(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    p->pushPath(p, "INBOX");
    p->pushPath(p, "Test Subject");
    TEST_ASSERT_EQUAL_STRING("/INBOX/Test Subject", p->getCurrentPath(p));
    p->popPath(p);
    TEST_ASSERT_EQUAL_STRING("/INBOX", p->getCurrentPath(p));
    p->popPath(p);
    TEST_ASSERT_EQUAL_STRING("/", p->getCurrentPath(p));
}

// --- Fetch with no config ---

void test_fetch_unconfigured_returns_help_message(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);

    int count = 0;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(1, count);
    TEST_ASSERT_NOT_NULL(elems);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, elems[0]->type);
    TEST_ASSERT_NOT_NULL(strstr(elems[0]->data.string, "configure"));

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

// --- Commands ---

void test_get_commands_returns_expected(void) {
    Provider *p = providerFactoryCreate("email client");
    int count = 0;
    const char **cmds = p->getCommands(p, &count);
    TEST_ASSERT_TRUE(count >= 2);
    bool hasCompose = false, hasRefresh = false;
    for (int i = 0; i < count; i++) {
        if (strcmp(cmds[i], "compose") == 0) hasCompose = true;
        if (strcmp(cmds[i], "refresh") == 0) hasRefresh = true;
    }
    TEST_ASSERT_TRUE(hasCompose);
    TEST_ASSERT_TRUE(hasRefresh);
}

void test_handle_command_compose_returns_input(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "compose", "", FFON_STRING,
                                       err, sizeof(err));
    TEST_ASSERT_NOT_NULL(r);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, r->type);
    TEST_ASSERT_TRUE(providerTagHasInput(r->data.string));
    ffonElementDestroy(r);
}

void test_handle_command_refresh_returns_null(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "refresh", "", FFON_STRING,
                                       err, sizeof(err));
    TEST_ASSERT_NULL(r);
}

void test_handle_command_set_imap_url(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "set imap url",
        "imaps://imap.example.com", FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
}

void test_handle_command_set_smtp_url(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "set smtp url",
        "smtps://smtp.example.com", FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
}

void test_handle_command_set_username(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "set username",
        "user@example.com", FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
}

void test_handle_command_set_password(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "set password",
        "secret123", FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
}

void test_handle_command_unknown_sets_error(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "nonexistent", "", FFON_STRING,
                                       err, sizeof(err));
    TEST_ASSERT_NULL(r);
    TEST_ASSERT_NOT_EQUAL(0, strlen(err));
}

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_factory_creates_provider);
    RUN_TEST(test_factory_returns_singleton);

    RUN_TEST(test_provider_has_required_functions);
    RUN_TEST(test_provider_has_commit);

    RUN_TEST(test_init_sets_root_path);
    RUN_TEST(test_push_pop_path);
    RUN_TEST(test_push_two_levels);

    RUN_TEST(test_fetch_unconfigured_returns_help_message);

    RUN_TEST(test_get_commands_returns_expected);
    RUN_TEST(test_handle_command_compose_returns_input);
    RUN_TEST(test_handle_command_refresh_returns_null);
    RUN_TEST(test_handle_command_set_imap_url);
    RUN_TEST(test_handle_command_set_smtp_url);
    RUN_TEST(test_handle_command_set_username);
    RUN_TEST(test_handle_command_set_password);
    RUN_TEST(test_handle_command_unknown_sets_error);

    return UNITY_END();
}

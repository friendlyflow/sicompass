#include <unity.h>
#include <provider_interface.h>
#include <provider_tags.h>
#include <chatclient.h>
#include <ffon.h>
#include <stdlib.h>
#include <string.h>

void setUp(void) {}
void tearDown(void) {}

// --- Factory registration ---

void test_factory_creates_provider(void) {
    Provider *p = providerFactoryCreate("chat client");
    TEST_ASSERT_NOT_NULL(p);
    TEST_ASSERT_EQUAL_STRING("chatclient", p->name);
}

void test_factory_returns_singleton(void) {
    Provider *p1 = providerFactoryCreate("chat client");
    Provider *p2 = providerFactoryCreate("chat client");
    TEST_ASSERT_EQUAL_PTR(p1, p2);
}

// --- Provider function pointers ---

void test_provider_has_required_functions(void) {
    Provider *p = providerFactoryCreate("chat client");
    TEST_ASSERT_NOT_NULL(p->fetch);
    TEST_ASSERT_NOT_NULL(p->init);
    TEST_ASSERT_NOT_NULL(p->pushPath);
    TEST_ASSERT_NOT_NULL(p->popPath);
    TEST_ASSERT_NOT_NULL(p->getCurrentPath);
    TEST_ASSERT_NOT_NULL(p->getCommands);
    TEST_ASSERT_NOT_NULL(p->handleCommand);
}

void test_provider_has_commit(void) {
    Provider *p = providerFactoryCreate("chat client");
    TEST_ASSERT_NOT_NULL(p->commitEdit);
}

// --- Path management ---

void test_init_sets_root_path(void) {
    Provider *p = providerFactoryCreate("chat client");
    p->init(p);
    TEST_ASSERT_EQUAL_STRING("/", p->getCurrentPath(p));
}

void test_push_pop_path(void) {
    Provider *p = providerFactoryCreate("chat client");
    p->init(p);
    p->pushPath(p, "General");
    TEST_ASSERT_EQUAL_STRING("/General", p->getCurrentPath(p));
    p->popPath(p);
    TEST_ASSERT_EQUAL_STRING("/", p->getCurrentPath(p));
}

// --- Fetch with no config ---

void test_fetch_unconfigured_returns_help_message(void) {
    Provider *p = providerFactoryCreate("chat client");
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
    Provider *p = providerFactoryCreate("chat client");
    int count = 0;
    const char **cmds = p->getCommands(p, &count);
    TEST_ASSERT_TRUE(count >= 2);
    bool hasSend = false, hasRefresh = false;
    for (int i = 0; i < count; i++) {
        if (strcmp(cmds[i], "send message") == 0) hasSend = true;
        if (strcmp(cmds[i], "refresh") == 0) hasRefresh = true;
    }
    TEST_ASSERT_TRUE(hasSend);
    TEST_ASSERT_TRUE(hasRefresh);
}

void test_handle_command_send_message_returns_input(void) {
    Provider *p = providerFactoryCreate("chat client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "send message", "", FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NOT_NULL(r);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, r->type);
    TEST_ASSERT_TRUE(providerTagHasInput(r->data.string));
    ffonElementDestroy(r);
}

void test_handle_command_refresh_returns_null(void) {
    Provider *p = providerFactoryCreate("chat client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "refresh", "", FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
}

void test_handle_command_set_homeserver(void) {
    Provider *p = providerFactoryCreate("chat client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "set homeserver",
        "https://matrix.example.com", FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
}

void test_handle_command_set_access_token(void) {
    Provider *p = providerFactoryCreate("chat client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "set access token",
        "syt_test_token", FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
}

void test_handle_command_set_username(void) {
    Provider *p = providerFactoryCreate("chat client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "set username",
        "testuser", FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
}

void test_handle_command_set_password(void) {
    Provider *p = providerFactoryCreate("chat client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "set password",
        "testpass", FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
}

void test_get_commands_includes_login_and_register(void) {
    Provider *p = providerFactoryCreate("chat client");
    int count = 0;
    const char **cmds = p->getCommands(p, &count);
    TEST_ASSERT_TRUE(count >= 4);
    bool hasLogin = false, hasRegister = false;
    for (int i = 0; i < count; i++) {
        if (strcmp(cmds[i], "login") == 0) hasLogin = true;
        if (strcmp(cmds[i], "register") == 0) hasRegister = true;
    }
    TEST_ASSERT_TRUE(hasLogin);
    TEST_ASSERT_TRUE(hasRegister);
}

void test_handle_command_login_without_credentials(void) {
    Provider *p = providerFactoryCreate("chat client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "login", NULL, FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
    TEST_ASSERT_TRUE(strlen(err) > 0);
}

void test_handle_command_register_without_credentials(void) {
    Provider *p = providerFactoryCreate("chat client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "register", NULL, FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
    TEST_ASSERT_TRUE(strlen(err) > 0);
}

void test_login_null_homeserver_fails(void) {
    ChatAuthResult r = chatclientLogin(NULL, "user", "pass");
    TEST_ASSERT_FALSE(r.success);
    TEST_ASSERT_TRUE(strlen(r.error) > 0);
}

void test_login_empty_username_fails(void) {
    ChatAuthResult r = chatclientLogin("https://example.com", "", "pass");
    TEST_ASSERT_FALSE(r.success);
    TEST_ASSERT_TRUE(strlen(r.error) > 0);
}

void test_register_null_homeserver_fails(void) {
    ChatAuthResult r = chatclientRegister(NULL, "user", "pass");
    TEST_ASSERT_FALSE(r.success);
    TEST_ASSERT_TRUE(strlen(r.error) > 0);
}

void test_register_empty_password_fails(void) {
    ChatAuthResult r = chatclientRegister("https://example.com", "user", "");
    TEST_ASSERT_FALSE(r.success);
    TEST_ASSERT_TRUE(strlen(r.error) > 0);
}

void test_register_complete_null_session_fails(void) {
    ChatAuthResult r = chatclientRegisterComplete("https://example.com", NULL, "user", "pass");
    TEST_ASSERT_FALSE(r.success);
    TEST_ASSERT_TRUE(strlen(r.error) > 0);
}

void test_register_complete_empty_session_fails(void) {
    ChatAuthResult r = chatclientRegisterComplete("https://example.com", "", "user", "pass");
    TEST_ASSERT_FALSE(r.success);
    TEST_ASSERT_TRUE(strlen(r.error) > 0);
}

void test_get_commands_includes_complete_registration(void) {
    Provider *p = providerFactoryCreate("chat client");
    int count = 0;
    const char **cmds = p->getCommands(p, &count);
    TEST_ASSERT_TRUE(count >= 5);
    bool found = false;
    for (int i = 0; i < count; i++) {
        if (strcmp(cmds[i], "complete registration") == 0) found = true;
    }
    TEST_ASSERT_TRUE(found);
}

void test_handle_command_complete_registration_no_session(void) {
    Provider *p = providerFactoryCreate("chat client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "complete registration", NULL, FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
    TEST_ASSERT_TRUE(strlen(err) > 0);
}

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_factory_creates_provider);
    RUN_TEST(test_factory_returns_singleton);

    RUN_TEST(test_provider_has_required_functions);
    RUN_TEST(test_provider_has_commit);

    RUN_TEST(test_init_sets_root_path);
    RUN_TEST(test_push_pop_path);

    RUN_TEST(test_fetch_unconfigured_returns_help_message);

    RUN_TEST(test_get_commands_returns_expected);
    RUN_TEST(test_handle_command_send_message_returns_input);
    RUN_TEST(test_handle_command_refresh_returns_null);
    RUN_TEST(test_handle_command_set_homeserver);
    RUN_TEST(test_handle_command_set_access_token);
    RUN_TEST(test_handle_command_set_username);
    RUN_TEST(test_handle_command_set_password);
    RUN_TEST(test_get_commands_includes_login_and_register);
    RUN_TEST(test_handle_command_login_without_credentials);
    RUN_TEST(test_handle_command_register_without_credentials);
    RUN_TEST(test_login_null_homeserver_fails);
    RUN_TEST(test_login_empty_username_fails);
    RUN_TEST(test_register_null_homeserver_fails);
    RUN_TEST(test_register_empty_password_fails);
    RUN_TEST(test_register_complete_null_session_fails);
    RUN_TEST(test_register_complete_empty_session_fails);
    RUN_TEST(test_get_commands_includes_complete_registration);
    RUN_TEST(test_handle_command_complete_registration_no_session);

    return UNITY_END();
}

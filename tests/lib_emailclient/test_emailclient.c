#include <unity.h>
#include <provider_interface.h>
#include <provider_tags.h>
#include <emailclient_oauth2.h>
#include <ffon.h>
#include <stdlib.h>
#include <string.h>

// --- Linker wraps for mocking ---

static OAuth2TokenResult g_mockAuthResult;
static OAuth2TokenResult g_mockRefreshResult;

OAuth2TokenResult __wrap_emailclientOAuth2Authorize(const char *clientId,
                                                      const char *clientSecret,
                                                      int timeoutSeconds) {
    (void)clientId;
    (void)clientSecret;
    (void)timeoutSeconds;
    return g_mockAuthResult;
}

OAuth2TokenResult __wrap_emailclientOAuth2RefreshToken(const char *clientId,
                                                        const char *clientSecret,
                                                        const char *refreshToken) {
    (void)clientId;
    (void)clientSecret;
    (void)refreshToken;
    return g_mockRefreshResult;
}

char* __wrap_providerGetMainConfigPath(void) {
    return NULL;
}

#ifdef _WIN32
/* No linker wrapping on Windows — __real_* calls go directly to the real functions */
#define __real_emailclientOAuth2Authorize   emailclientOAuth2Authorize
#define __real_emailclientOAuth2RefreshToken emailclientOAuth2RefreshToken
#else
extern OAuth2TokenResult __real_emailclientOAuth2Authorize(const char *clientId,
                                                            const char *clientSecret,
                                                            int timeoutSeconds);
extern OAuth2TokenResult __real_emailclientOAuth2RefreshToken(const char *clientId,
                                                               const char *clientSecret,
                                                               const char *refreshToken);
#endif

void setUp(void) {
    memset(&g_mockAuthResult, 0, sizeof(g_mockAuthResult));
    memset(&g_mockRefreshResult, 0, sizeof(g_mockRefreshResult));
}
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

void test_provider_has_no_cache_set(void) {
    Provider *p = providerFactoryCreate("email client");
    TEST_ASSERT_TRUE(p->noCache);
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

// --- OAuth2 commands ---

void test_get_commands_includes_login_and_logout(void) {
    Provider *p = providerFactoryCreate("email client");
    int count = 0;
    const char **cmds = p->getCommands(p, &count);
    TEST_ASSERT_EQUAL_INT(4, count);
    bool hasLogin = false, hasLogout = false;
    for (int i = 0; i < count; i++) {
        if (strcmp(cmds[i], "login") == 0) hasLogin = true;
        if (strcmp(cmds[i], "logout") == 0) hasLogout = true;
    }
    TEST_ASSERT_TRUE(hasLogin);
    TEST_ASSERT_TRUE(hasLogout);
}

void test_handle_command_login_without_credentials(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    // Clear any credentials loaded from settings so we test the unconfigured path
    p->handleCommand(p, "set client id", "", FFON_STRING, NULL, 0);
    p->handleCommand(p, "set client secret", "", FFON_STRING, NULL, 0);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "login", NULL, FFON_STRING,
                                       err, sizeof(err));
    TEST_ASSERT_NULL(r);
    TEST_ASSERT_TRUE(strlen(err) > 0);
}

void test_handle_command_logout(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "logout", NULL, FFON_STRING,
                                       err, sizeof(err));
    TEST_ASSERT_NOT_NULL(r);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, r->type);
    TEST_ASSERT_NOT_NULL(strstr(r->data.string, "logged out"));
    ffonElementDestroy(r);
}

void test_handle_command_set_client_id(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "set client id",
        "test-id.apps.googleusercontent.com", FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
}

void test_handle_command_set_client_secret(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "set client secret",
        "GOCSPX-test-secret", FFON_STRING, err, sizeof(err));
    TEST_ASSERT_NULL(r);
}

// --- OAuth2 login mocked ---

void test_handle_command_login_success(void) {
#ifndef _WIN32
    /* Requires --wrap=emailclientOAuth2Authorize linker mock (Linux only) */
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    p->handleCommand(p, "set client id", "test-id.apps.googleusercontent.com",
                     FFON_STRING, NULL, 0);
    p->handleCommand(p, "set client secret", "GOCSPX-test-secret",
                     FFON_STRING, NULL, 0);

    g_mockAuthResult.success = true;
    strncpy(g_mockAuthResult.accessToken, "mock-access-token",
            sizeof(g_mockAuthResult.accessToken) - 1);
    strncpy(g_mockAuthResult.refreshToken, "mock-refresh-token",
            sizeof(g_mockAuthResult.refreshToken) - 1);
    g_mockAuthResult.expiresIn = 3600;

    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "login", NULL, FFON_STRING,
                                       err, sizeof(err));
    TEST_ASSERT_NOT_NULL(r);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, r->type);
    TEST_ASSERT_NOT_NULL(strstr(r->data.string, "successful"));
    TEST_ASSERT_EQUAL_STRING("", err);
    ffonElementDestroy(r);
#endif
}

void test_handle_command_login_oauth_fails(void) {
#ifndef _WIN32
    /* Requires --wrap=emailclientOAuth2Authorize linker mock (Linux only) */
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    p->handleCommand(p, "set client id", "test-id.apps.googleusercontent.com",
                     FFON_STRING, NULL, 0);
    p->handleCommand(p, "set client secret", "GOCSPX-test-secret",
                     FFON_STRING, NULL, 0);

    g_mockAuthResult.success = false;
    strncpy(g_mockAuthResult.error, "user_denied",
            sizeof(g_mockAuthResult.error) - 1);

    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "login", NULL, FFON_STRING,
                                       err, sizeof(err));
    TEST_ASSERT_NULL(r);
    TEST_ASSERT_NOT_NULL(strstr(err, "OAuth2 failed"));
    TEST_ASSERT_NOT_NULL(strstr(err, "user_denied"));
#endif
}

// --- OAuth2 function validation ---

void test_oauth2_authorize_null_params_fails(void) {
    OAuth2TokenResult r = __real_emailclientOAuth2Authorize(NULL, "secret", 1);
    TEST_ASSERT_FALSE(r.success);
    TEST_ASSERT_TRUE(strlen(r.error) > 0);
}

void test_oauth2_authorize_empty_secret_fails(void) {
    OAuth2TokenResult r = __real_emailclientOAuth2Authorize("id", "", 1);
    TEST_ASSERT_FALSE(r.success);
    TEST_ASSERT_TRUE(strlen(r.error) > 0);
}

void test_oauth2_refresh_empty_token_fails(void) {
    OAuth2TokenResult r = __real_emailclientOAuth2RefreshToken("id", "secret", "");
    TEST_ASSERT_FALSE(r.success);
    TEST_ASSERT_TRUE(strlen(r.error) > 0);
}

void test_oauth2_refresh_null_token_fails(void) {
    OAuth2TokenResult r = __real_emailclientOAuth2RefreshToken("id", "secret", NULL);
    TEST_ASSERT_FALSE(r.success);
    TEST_ASSERT_TRUE(strlen(r.error) > 0);
}

int main(void) {
    UNITY_BEGIN();

    RUN_TEST(test_factory_creates_provider);
    RUN_TEST(test_factory_returns_singleton);

    RUN_TEST(test_provider_has_required_functions);
    RUN_TEST(test_provider_has_commit);
    RUN_TEST(test_provider_has_no_cache_set);

    RUN_TEST(test_fetch_unconfigured_returns_help_message);

    RUN_TEST(test_init_sets_root_path);
    RUN_TEST(test_push_pop_path);
    RUN_TEST(test_push_two_levels);

    RUN_TEST(test_get_commands_returns_expected);
    RUN_TEST(test_handle_command_compose_returns_input);
    RUN_TEST(test_handle_command_refresh_returns_null);
    RUN_TEST(test_handle_command_set_imap_url);
    RUN_TEST(test_handle_command_set_smtp_url);
    RUN_TEST(test_handle_command_set_username);
    RUN_TEST(test_handle_command_set_password);
    RUN_TEST(test_handle_command_unknown_sets_error);

    RUN_TEST(test_get_commands_includes_login_and_logout);
    RUN_TEST(test_handle_command_login_without_credentials);
    RUN_TEST(test_handle_command_logout);
    RUN_TEST(test_handle_command_set_client_id);
    RUN_TEST(test_handle_command_set_client_secret);

    RUN_TEST(test_handle_command_login_success);
    RUN_TEST(test_handle_command_login_oauth_fails);

    RUN_TEST(test_oauth2_authorize_null_params_fails);
    RUN_TEST(test_oauth2_authorize_empty_secret_fails);
    RUN_TEST(test_oauth2_refresh_empty_token_fails);
    RUN_TEST(test_oauth2_refresh_null_token_fails);

    return UNITY_END();
}

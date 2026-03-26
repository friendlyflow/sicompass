#include <unity.h>
#include <provider_interface.h>
#include <provider_tags.h>
#include <emailclient.h>
#include <emailclient_idle.h>
#include <emailclient_oauth2.h>
#include <ffon.h>
#include <stdlib.h>
#include <string.h>

// --- Linker wraps for mocking ---

static OAuth2TokenResult g_mockAuthResult;
static OAuth2TokenResult g_mockRefreshResult;

// Mock state for IMAP operations
static EmailFolder *g_mockFolders = NULL;
static int g_mockFolderCount = 0;
static EmailHeader *g_mockHeaders = NULL;
static int g_mockHeaderCount = 0;
static EmailMessage *g_mockMessage = NULL;
static EmailMessage *g_mockMessageByMsgId = NULL;
static int g_fetchMessageByMsgIdCallCount = 0;
static int g_listFoldersCallCount = 0;
static int g_listMessagesCallCount = 0;

EmailFolder* __wrap_emailclientListFolders(EmailClientConfig *config,
                                            int *outCount) {
    (void)config;
    g_listFoldersCallCount++;
    if (!g_mockFolders) { *outCount = 0; return NULL; }
    *outCount = g_mockFolderCount;
    EmailFolder *copy = calloc(g_mockFolderCount, sizeof(EmailFolder));
    memcpy(copy, g_mockFolders, g_mockFolderCount * sizeof(EmailFolder));
    return copy;
}

void __wrap_emailclientFreeFolders(EmailFolder *folders, int count) {
    (void)count;
    free(folders);
}

EmailHeader* __wrap_emailclientListMessages(EmailClientConfig *config,
                                             const char *folder, int limit,
                                             int *outCount) {
    (void)config; (void)folder; (void)limit;
    g_listMessagesCallCount++;
    if (!g_mockHeaders) { *outCount = 0; return NULL; }
    *outCount = g_mockHeaderCount;
    EmailHeader *copy = calloc(g_mockHeaderCount, sizeof(EmailHeader));
    memcpy(copy, g_mockHeaders, g_mockHeaderCount * sizeof(EmailHeader));
    return copy;
}

void __wrap_emailclientFreeHeaders(EmailHeader *headers, int count) {
    (void)count;
    free(headers);
}

EmailMessage* __wrap_emailclientFetchMessage(EmailClientConfig *config,
                                              const char *folder, int uid) {
    (void)config; (void)folder; (void)uid;
    if (!g_mockMessage) return NULL;
    EmailMessage *copy = malloc(sizeof(EmailMessage));
    memcpy(copy, g_mockMessage, sizeof(EmailMessage));
    return copy;
}

void __wrap_emailclientFreeMessage(EmailMessage *msg) {
    free(msg);
}

EmailMessage* __wrap_emailclientFetchMessageByMessageId(
    EmailClientConfig *config, const char *folder, const char *messageId) {
    (void)config; (void)folder; (void)messageId;
    g_fetchMessageByMsgIdCallCount++;
    if (!g_mockMessageByMsgId) return NULL;
    EmailMessage *copy = malloc(sizeof(EmailMessage));
    memcpy(copy, g_mockMessageByMsgId, sizeof(EmailMessage));
    return copy;
}

bool __wrap_emailclientIdleStart(const EmailClientConfig *config,
                                  const char *folder,
                                  EmailIdleNotifyFn notifyFn, void *userdata) {
    (void)config; (void)folder; (void)notifyFn; (void)userdata;
    return true;
}

void __wrap_emailclientIdleStop(void) {}

void __wrap_emailclientGlobalInit(void) {}
void __wrap_emailclientGlobalCleanup(void) {}

bool __wrap_emailclientSendMessage(EmailClientConfig *config,
                                    const char *to, const char *subject,
                                    const char *body) {
    (void)config; (void)to; (void)subject; (void)body;
    return true;
}

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
    g_mockFolders = NULL;
    g_mockFolderCount = 0;
    g_mockHeaders = NULL;
    g_mockHeaderCount = 0;
    g_mockMessage = NULL;
    g_mockMessageByMsgId = NULL;
    g_fetchMessageByMsgIdCallCount = 0;
    g_listFoldersCallCount = 0;
    g_listMessagesCallCount = 0;
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
    bool hasCompose = false;
    for (int i = 0; i < count; i++) {
        if (strcmp(cmds[i], "compose") == 0) hasCompose = true;
    }
    TEST_ASSERT_TRUE(hasCompose);
}

void test_handle_command_compose_returns_null(void) {
    // compose command returns NULL — the compose object is always present
    // at root level and the app layer navigates to it
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    char err[256] = "";
    FfonElement *r = p->handleCommand(p, "compose", "", FFON_STRING,
                                       err, sizeof(err));
    TEST_ASSERT_NULL(r);
    TEST_ASSERT_EQUAL_STRING("", err);
}

void test_provider_has_needs_refresh(void) {
    Provider *p = providerFactoryCreate("email client");
    TEST_ASSERT_FALSE(p->needsRefresh);
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
    TEST_ASSERT_EQUAL_INT(3, count);
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

void test_commit_stores_to_field(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    p->pushPath(p, "compose");
    p->pushPath(p, "To");
    bool ok = p->commitEdit(p, "", "user@example.com");
    TEST_ASSERT_TRUE(ok);
    p->popPath(p);
    p->popPath(p);
}

void test_commit_stores_subject_field(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    p->pushPath(p, "compose");
    p->pushPath(p, "Subject");
    bool ok = p->commitEdit(p, "", "Test Subject");
    TEST_ASSERT_TRUE(ok);
    p->popPath(p);
    p->popPath(p);
}

void test_commit_stores_body_field(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    p->pushPath(p, "compose");
    p->pushPath(p, "Body");
    bool ok = p->commitEdit(p, "", "Hello world");
    TEST_ASSERT_TRUE(ok);
    p->popPath(p);
    p->popPath(p);
}

void test_commit_unknown_field_returns_false(void) {
    Provider *p = providerFactoryCreate("email client");
    p->init(p);
    p->pushPath(p, "compose");
    p->pushPath(p, "Unknown");
    bool ok = p->commitEdit(p, "", "value");
    TEST_ASSERT_FALSE(ok);
    p->popPath(p);
    p->popPath(p);
}

void test_provider_has_button_press(void) {
    Provider *p = providerFactoryCreate("email client");
    TEST_ASSERT_NOT_NULL(p->onButtonPress);
}

// --- Helper: configure provider with mock credentials ---
static void configureProvider(Provider *p) {
    p->init(p);
    char err[256] = "";
    p->handleCommand(p, "set imap url", "imaps://mock", FFON_STRING,
                     err, sizeof(err));
    p->handleCommand(p, "set username", "user@test", FFON_STRING,
                     err, sizeof(err));
    p->handleCommand(p, "set password", "pass", FFON_STRING,
                     err, sizeof(err));
}

// --- Fetch / navigation tests ---

void test_fetch_root_lists_folders(void) {
    Provider *p = providerFactoryCreate("email client");
    configureProvider(p);

    EmailFolder folders[] = {{"INBOX"}, {"Sent"}};
    g_mockFolders = folders;
    g_mockFolderCount = 2;

    int count = 0;
    FfonElement **elems = p->fetch(p, &count);
    // INBOX + compose + Sent = 3
    TEST_ASSERT_EQUAL_INT(3, count);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, elems[0]->type);
    TEST_ASSERT_EQUAL_STRING("INBOX", elems[0]->data.object->key);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_fetch_folder_lists_messages(void) {
    Provider *p = providerFactoryCreate("email client");
    configureProvider(p);

    // First navigate to root to set up folder mappings
    EmailFolder folders[] = {{"INBOX"}};
    g_mockFolders = folders;
    g_mockFolderCount = 1;
    int count = 0;
    FfonElement **elems = p->fetch(p, &count);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    // Navigate into INBOX
    p->pushPath(p, "INBOX");
    EmailHeader headers[] = {{.uid = 1, .from = "alice@test", .subject = "Hello", .date = "Mon"}};
    g_mockHeaders = headers;
    g_mockHeaderCount = 1;

    elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(1, count);
    TEST_ASSERT_EQUAL_INT(FFON_OBJECT, elems[0]->type);
    TEST_ASSERT_NOT_NULL(strstr(elems[0]->data.object->key, "Hello"));

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    p->popPath(p);
}

void test_fetch_message_does_not_fetch_history_eagerly(void) {
    Provider *p = providerFactoryCreate("email client");
    configureProvider(p);

    // Set up folder mappings
    EmailFolder folders[] = {{"INBOX"}};
    g_mockFolders = folders;
    g_mockFolderCount = 1;
    int count = 0;
    FfonElement **elems = p->fetch(p, &count);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    // Navigate into INBOX, set up message mappings
    p->pushPath(p, "INBOX");
    EmailHeader headers[] = {{.uid = 42, .from = "alice@test", .subject = "Test", .date = "Mon"}};
    g_mockHeaders = headers;
    g_mockHeaderCount = 1;
    elems = p->fetch(p, &count);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    // Navigate into the message
    p->pushPath(p, "Test \xe2\x80\x94 alice@test");
    EmailMessage msg = {
        .uid = 42, .from = "alice@test", .to = "bob@test",
        .subject = "Test", .date = "Mon, 1 Jan 2025",
        .messageId = "<msg1@test>", .inReplyTo = "<msg0@test>",
        .references = "<msg0@test>",
        .body = "hello"
    };
    g_mockMessage = &msg;
    g_fetchMessageByMsgIdCallCount = 0;

    elems = p->fetch(p, &count);
    // Should NOT have called fetchMessageByMessageId at depth 2
    TEST_ASSERT_EQUAL_INT(0, g_fetchMessageByMsgIdCallCount);
    // Should have a History object in the results
    bool hasHistory = false;
    for (int i = 0; i < count; i++) {
        if (elems[i]->type == FFON_OBJECT && strcmp(elems[i]->data.object->key, "History") == 0)
            hasHistory = true;
    }
    TEST_ASSERT_TRUE(hasHistory);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    p->popPath(p);
    p->popPath(p);
}

void test_fetch_history_lazily_on_navigation(void) {
    Provider *p = providerFactoryCreate("email client");
    configureProvider(p);

    // Set up folder mappings
    EmailFolder folders[] = {{"INBOX"}};
    g_mockFolders = folders;
    g_mockFolderCount = 1;
    int count = 0;
    FfonElement **elems = p->fetch(p, &count);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    // Navigate into INBOX
    p->pushPath(p, "INBOX");
    EmailHeader headers[] = {{.uid = 42, .from = "alice@test", .subject = "Test", .date = "Mon"}};
    g_mockHeaders = headers;
    g_mockHeaderCount = 1;
    elems = p->fetch(p, &count);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    // Navigate into the message (stores references)
    p->pushPath(p, "Test \xe2\x80\x94 alice@test");
    EmailMessage msg = {
        .uid = 42, .from = "alice@test", .to = "bob@test",
        .subject = "Test", .date = "Mon, 1 Jan 2025",
        .messageId = "<msg1@test>", .inReplyTo = "<msg0@test>",
        .references = "<msg0@test>",
        .body = "hello"
    };
    g_mockMessage = &msg;
    elems = p->fetch(p, &count);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    // Navigate into History — should now fetch referenced messages
    p->pushPath(p, "History");
    EmailMessage refMsg = {
        .uid = 10, .from = "bob@test", .to = "alice@test",
        .subject = "Original", .date = "Sun, 31 Dec 2024",
        .messageId = "<msg0@test>",
        .body = "original message"
    };
    g_mockMessageByMsgId = &refMsg;
    g_fetchMessageByMsgIdCallCount = 0;

    elems = p->fetch(p, &count);
    // NOW fetchMessageByMessageId should have been called
    TEST_ASSERT_TRUE(g_fetchMessageByMsgIdCallCount > 0);
    TEST_ASSERT_TRUE(count > 0);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    p->popPath(p);
    p->popPath(p);
    p->popPath(p);
}

void test_folder_cache_avoids_refetch(void) {
    Provider *p = providerFactoryCreate("email client");
    configureProvider(p);

    EmailFolder folders[] = {{"INBOX"}};
    g_mockFolders = folders;
    g_mockFolderCount = 1;

    // First fetch — calls emailclientListFolders
    int count = 0;
    FfonElement **elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(1, g_listFoldersCallCount);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    // Second fetch at root — served from cache, no IMAP call
    elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(1, g_listFoldersCallCount);
    TEST_ASSERT_TRUE(count > 0);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
}

void test_envelope_cache_avoids_refetch(void) {
    Provider *p = providerFactoryCreate("email client");
    configureProvider(p);

    EmailFolder folders[] = {{"INBOX"}};
    g_mockFolders = folders;
    g_mockFolderCount = 1;
    int count = 0;
    FfonElement **elems = p->fetch(p, &count);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    p->pushPath(p, "INBOX");
    EmailHeader headers[] = {{.uid = 1, .from = "a@test", .subject = "Hi", .date = "Mon"}};
    g_mockHeaders = headers;
    g_mockHeaderCount = 1;

    // First fetch — calls emailclientListMessages
    elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(1, g_listMessagesCallCount);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    // Second fetch — served from cache
    elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(1, g_listMessagesCallCount);
    TEST_ASSERT_EQUAL_INT(1, count);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    p->popPath(p);
}

void test_fetch_body_returns_body_lines(void) {
    Provider *p = providerFactoryCreate("email client");
    configureProvider(p);

    EmailFolder folders[] = {{"INBOX"}};
    g_mockFolders = folders;
    g_mockFolderCount = 1;
    int count = 0;
    FfonElement **elems = p->fetch(p, &count);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    p->pushPath(p, "INBOX");
    EmailHeader headers[] = {{.uid = 7, .from = "a@test", .subject = "Hi", .date = "Mon"}};
    g_mockHeaders = headers;
    g_mockHeaderCount = 1;
    elems = p->fetch(p, &count);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    // Navigate into message
    p->pushPath(p, "Hi \xe2\x80\x94 a@test");
    EmailMessage msg = {
        .uid = 7, .from = "a@test", .to = "b@test",
        .subject = "Hi", .date = "Mon, 1 Jan 2025",
        .body = "line one\nline two\nline three"
    };
    g_mockMessage = &msg;
    elems = p->fetch(p, &count);
    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);

    // Navigate into Body — should return body lines
    p->pushPath(p, "Body");
    elems = p->fetch(p, &count);
    TEST_ASSERT_EQUAL_INT(3, count);
    TEST_ASSERT_EQUAL_INT(FFON_STRING, elems[0]->type);
    TEST_ASSERT_EQUAL_STRING("line one", elems[0]->data.string);
    TEST_ASSERT_EQUAL_STRING("line two", elems[1]->data.string);
    TEST_ASSERT_EQUAL_STRING("line three", elems[2]->data.string);

    for (int i = 0; i < count; i++) ffonElementDestroy(elems[i]);
    free(elems);
    p->popPath(p);
    p->popPath(p);
    p->popPath(p);
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
    RUN_TEST(test_handle_command_compose_returns_null);
    RUN_TEST(test_provider_has_needs_refresh);
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

    RUN_TEST(test_commit_stores_to_field);
    RUN_TEST(test_commit_stores_subject_field);
    RUN_TEST(test_commit_stores_body_field);
    RUN_TEST(test_commit_unknown_field_returns_false);
    RUN_TEST(test_provider_has_button_press);

    RUN_TEST(test_oauth2_authorize_null_params_fails);
    RUN_TEST(test_oauth2_authorize_empty_secret_fails);
    RUN_TEST(test_oauth2_refresh_empty_token_fails);
    RUN_TEST(test_oauth2_refresh_null_token_fails);

    RUN_TEST(test_fetch_root_lists_folders);
    RUN_TEST(test_fetch_folder_lists_messages);
    RUN_TEST(test_fetch_message_does_not_fetch_history_eagerly);
    RUN_TEST(test_fetch_history_lazily_on_navigation);
    RUN_TEST(test_folder_cache_avoids_refetch);
    RUN_TEST(test_envelope_cache_avoids_refetch);
    RUN_TEST(test_fetch_body_returns_body_lines);

    return UNITY_END();
}

#include <emailclient.h>
#include <emailclient_oauth2.h>
#include <emailclient_provider.h>
#include <provider_tags.h>
#include <platform.h>
#include <ffon.h>
#include <json-c/json.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <time.h>

// Shared config loaded from settings
static EmailClientConfig g_config = {
    "", "", "", "", "", "", "", "", 0
};

// Map display names back to UIDs for message fetch
#define MAX_MESSAGES 512

typedef struct {
    char display[512];
    int uid;
} MessageMapping;

static MessageMapping g_msgMappings[MAX_MESSAGES];
static int g_msgMappingCount = 0;

// Map display names back to folder names
#define MAX_FOLDERS 128

typedef struct {
    char displayName[256];
    char folderName[256];
} FolderMapping;

static FolderMapping g_folderMappings[MAX_FOLDERS];
static int g_folderMappingCount = 0;

static void ecStoreFolderMappings(const EmailFolder *folders, int count) {
    g_folderMappingCount = 0;
    for (int i = 0; i < count && i < MAX_FOLDERS; i++) {
        strncpy(g_folderMappings[i].displayName, folders[i].name,
                sizeof(g_folderMappings[i].displayName) - 1);
        g_folderMappings[i].displayName[sizeof(g_folderMappings[i].displayName) - 1] = '\0';
        strncpy(g_folderMappings[i].folderName, folders[i].name,
                sizeof(g_folderMappings[i].folderName) - 1);
        g_folderMappings[i].folderName[sizeof(g_folderMappings[i].folderName) - 1] = '\0';
        g_folderMappingCount++;
    }
}

static const char* ecLookupFolderName(const char *displayName) {
    for (int i = 0; i < g_folderMappingCount; i++) {
        if (strcmp(g_folderMappings[i].displayName, displayName) == 0)
            return g_folderMappings[i].folderName;
    }
    return NULL;
}

static int ecLookupUid(const char *display) {
    for (int i = 0; i < g_msgMappingCount; i++) {
        if (strcmp(g_msgMappings[i].display, display) == 0)
            return g_msgMappings[i].uid;
    }
    return -1;
}

// Count slashes to determine path depth
// "/" = 0, "/INBOX" = 1, "/INBOX/subject" = 2
static int pathDepth(const char *path) {
    if (!path || strcmp(path, "/") == 0) return 0;
    int depth = 0;
    for (const char *p = path + 1; *p; p++) {
        if (*p == '/') depth++;
    }
    return depth + 1;
}

// Extract the first path segment after "/"
static void pathFirstSegment(const char *path, char *out, int outSize) {
    out[0] = '\0';
    if (!path || path[0] != '/' || path[1] == '\0') return;
    const char *start = path + 1;
    const char *slash = strchr(start, '/');
    int len = slash ? (int)(slash - start) : (int)strlen(start);
    if (len >= outSize) len = outSize - 1;
    memcpy(out, start, len);
    out[len] = '\0';
}

// Extract the second path segment (after second "/")
static void pathSecondSegment(const char *path, char *out, int outSize) {
    out[0] = '\0';
    if (!path || path[0] != '/') return;
    const char *start = path + 1;
    const char *slash = strchr(start, '/');
    if (!slash) return;
    start = slash + 1;
    slash = strchr(start, '/');
    int len = slash ? (int)(slash - start) : (int)strlen(start);
    if (len >= outSize) len = outSize - 1;
    memcpy(out, start, len);
    out[len] = '\0';
}

// Persist OAuth tokens to settings.json
static void ecSaveOAuthTokens(void) {
    char *configPath = providerGetMainConfigPath();
    if (!configPath) return;

    json_object *root = json_object_from_file(configPath);
    if (!root) root = json_object_new_object();

    json_object *section = NULL;
    if (!json_object_object_get_ex(root, "email client", &section)) {
        section = json_object_new_object();
        json_object_object_add(root, "email client", section);
    }
    json_object_object_del(section, "emailOAuthAccessToken");
    json_object_object_add(section, "emailOAuthAccessToken",
                           json_object_new_string(g_config.oauthAccessToken));
    json_object_object_del(section, "emailOAuthRefreshToken");
    json_object_object_add(section, "emailOAuthRefreshToken",
                           json_object_new_string(g_config.oauthRefreshToken));
    json_object_object_del(section, "emailTokenExpiry");
    json_object_object_add(section, "emailTokenExpiry",
                           json_object_new_int64(g_config.tokenExpiry));

    json_object_to_file_ext(configPath, root, JSON_C_TO_STRING_PRETTY);
    json_object_put(root);
    free(configPath);
}

static FfonElement** ecFetch(const char *path, int *outCount) {
    if (!g_config.imapUrl[0] || !g_config.username[0]) {
        *outCount = 1;
        FfonElement **elems = malloc(sizeof(FfonElement*));
        elems[0] = ffonElementCreateString(
            "configure IMAP URL, username, and password in settings");
        return elems;
    }

    int depth = pathDepth(path);

    if (depth == 0) {
        // Root: list folders
        int folderCount = 0;
        EmailFolder *folders = emailclientListFolders(&g_config, &folderCount);
        if (!folders || folderCount == 0) {
            emailclientFreeFolders(folders, folderCount);
            *outCount = 1;
            FfonElement **elems = malloc(sizeof(FfonElement*));
            elems[0] = ffonElementCreateString("no folders found");
            return elems;
        }

        FfonElement **elems = malloc(folderCount * sizeof(FfonElement*));
        for (int i = 0; i < folderCount; i++) {
            elems[i] = ffonElementCreateObject(folders[i].name);
        }
        ecStoreFolderMappings(folders, folderCount);
        emailclientFreeFolders(folders, folderCount);
        *outCount = folderCount;
        return elems;
    }

    char folderSegment[256];
    pathFirstSegment(path, folderSegment, sizeof(folderSegment));
    const char *folder = ecLookupFolderName(folderSegment);
    if (!folder) folder = folderSegment;

    if (depth == 1) {
        // Inside a folder: list messages
        int msgCount = 0;
        EmailHeader *headers = emailclientListMessages(&g_config, folder,
                                                        50, &msgCount);
        if (!headers || msgCount == 0) {
            emailclientFreeHeaders(headers, msgCount);
            *outCount = 1;
            FfonElement **elems = malloc(sizeof(FfonElement*));
            elems[0] = ffonElementCreateString("no messages");
            return elems;
        }

        FfonElement **elems = malloc(msgCount * sizeof(FfonElement*));
        g_msgMappingCount = 0;
        for (int i = 0; i < msgCount && i < MAX_MESSAGES; i++) {
            char display[1024];
            if (headers[i].subject[0]) {
                snprintf(display, sizeof(display), "%s — %s",
                         headers[i].subject, headers[i].from);
            } else {
                snprintf(display, sizeof(display), "(no subject) — %s",
                         headers[i].from);
            }
            elems[i] = ffonElementCreateObject(display);

            // Store mapping
            strncpy(g_msgMappings[g_msgMappingCount].display, display,
                    sizeof(g_msgMappings[g_msgMappingCount].display) - 1);
            g_msgMappings[g_msgMappingCount].display[
                sizeof(g_msgMappings[g_msgMappingCount].display) - 1] = '\0';
            g_msgMappings[g_msgMappingCount].uid = headers[i].uid;
            g_msgMappingCount++;
        }
        emailclientFreeHeaders(headers, msgCount);
        *outCount = msgCount;
        return elems;
    }

    if (depth == 2) {
        // Inside a message: show body
        char msgSegment[512];
        pathSecondSegment(path, msgSegment, sizeof(msgSegment));
        int uid = ecLookupUid(msgSegment);
        if (uid < 0) {
            *outCount = 1;
            FfonElement **elems = malloc(sizeof(FfonElement*));
            elems[0] = ffonElementCreateString("message not found");
            return elems;
        }

        EmailMessage *msg = emailclientFetchMessage(&g_config, folder, uid);
        if (!msg) {
            *outCount = 1;
            FfonElement **elems = malloc(sizeof(FfonElement*));
            elems[0] = ffonElementCreateString("failed to fetch message");
            return elems;
        }

        // Display: from, date, subject, blank, body lines
        // Count body lines
        int bodyLines = 1;
        for (const char *p = msg->body; *p; p++) {
            if (*p == '\n') bodyLines++;
        }

        int totalElems = 3 + bodyLines; // from, date, subject, body lines
        FfonElement **elems = malloc(totalElems * sizeof(FfonElement*));
        int idx = 0;

        char buf[1024];
        snprintf(buf, sizeof(buf), "From: %s", msg->from);
        elems[idx++] = ffonElementCreateString(buf);
        snprintf(buf, sizeof(buf), "Date: %s", msg->date);
        elems[idx++] = ffonElementCreateString(buf);
        snprintf(buf, sizeof(buf), "Subject: %s", msg->subject);
        elems[idx++] = ffonElementCreateString(buf);

        // Split body by newlines
        char *body = msg->body;
        while (*body) {
            char *eol = strchr(body, '\n');
            if (eol) {
                int len = eol - body;
                // Trim \r
                if (len > 0 && body[len - 1] == '\r') len--;
                char line[4096];
                if (len >= (int)sizeof(line)) len = sizeof(line) - 1;
                memcpy(line, body, len);
                line[len] = '\0';
                elems[idx++] = ffonElementCreateString(line);
                body = eol + 1;
            } else {
                elems[idx++] = ffonElementCreateString(body);
                break;
            }
        }

        emailclientFreeMessage(msg);
        *outCount = idx;
        return elems;
    }

    *outCount = 0;
    return NULL;
}

static bool ecCommit(const char *path, const char *oldName,
                      const char *newName) {
    (void)path;
    (void)oldName;
    (void)newName;
    return false;
}

static const char *ec_commands[] = {"compose", "refresh", "login", "logout"};

static const char** ecGetCommands(int *outCount) {
    *outCount = 4;
    return ec_commands;
}

static FfonElement* ecHandleCommand(const char *path, const char *command,
                                     const char *elementKey, int elementType,
                                     char *errorMsg, int errorMsgSize) {
    (void)path;
    (void)elementType;

    if (strcmp(command, "compose") == 0) {
        return ffonElementCreateString("<input></input>");
    }
    if (strcmp(command, "refresh") == 0) {
        return NULL;
    }
    if (strcmp(command, "login") == 0) {
        if (!g_config.clientId[0] || !g_config.clientSecret[0]) {
            if (errorMsg && errorMsgSize > 0)
                snprintf(errorMsg, errorMsgSize,
                         "set client ID and client secret in settings first");
            return NULL;
        }
        OAuth2TokenResult auth = emailclientOAuth2Authorize(
            g_config.clientId, g_config.clientSecret, 120);
        if (auth.success) {
            strncpy(g_config.oauthAccessToken, auth.accessToken,
                    sizeof(g_config.oauthAccessToken) - 1);
            g_config.oauthAccessToken[sizeof(g_config.oauthAccessToken) - 1] = '\0';
            strncpy(g_config.oauthRefreshToken, auth.refreshToken,
                    sizeof(g_config.oauthRefreshToken) - 1);
            g_config.oauthRefreshToken[sizeof(g_config.oauthRefreshToken) - 1] = '\0';
            g_config.tokenExpiry = time(NULL) + auth.expiresIn;
            ecSaveOAuthTokens();
            return ffonElementCreateString("Google OAuth2 login successful");
        }
        if (errorMsg && errorMsgSize > 0)
            snprintf(errorMsg, errorMsgSize, "OAuth2 failed: %s", auth.error);
        return NULL;
    }
    if (strcmp(command, "logout") == 0) {
        g_config.oauthAccessToken[0] = '\0';
        g_config.oauthRefreshToken[0] = '\0';
        g_config.tokenExpiry = 0;
        ecSaveOAuthTokens();
        return ffonElementCreateString("logged out");
    }
    // Internal commands for settings dispatch
    if (strcmp(command, "set imap url") == 0) {
        if (elementKey) {
            strncpy(g_config.imapUrl, elementKey,
                    sizeof(g_config.imapUrl) - 1);
            g_config.imapUrl[sizeof(g_config.imapUrl) - 1] = '\0';
        }
        return NULL;
    }
    if (strcmp(command, "set smtp url") == 0) {
        if (elementKey) {
            strncpy(g_config.smtpUrl, elementKey,
                    sizeof(g_config.smtpUrl) - 1);
            g_config.smtpUrl[sizeof(g_config.smtpUrl) - 1] = '\0';
        }
        return NULL;
    }
    if (strcmp(command, "set username") == 0) {
        if (elementKey) {
            strncpy(g_config.username, elementKey,
                    sizeof(g_config.username) - 1);
            g_config.username[sizeof(g_config.username) - 1] = '\0';
        }
        return NULL;
    }
    if (strcmp(command, "set password") == 0) {
        if (elementKey) {
            strncpy(g_config.password, elementKey,
                    sizeof(g_config.password) - 1);
            g_config.password[sizeof(g_config.password) - 1] = '\0';
        }
        return NULL;
    }
    if (strcmp(command, "set client id") == 0) {
        if (elementKey) {
            strncpy(g_config.clientId, elementKey,
                    sizeof(g_config.clientId) - 1);
            g_config.clientId[sizeof(g_config.clientId) - 1] = '\0';
        }
        return NULL;
    }
    if (strcmp(command, "set client secret") == 0) {
        if (elementKey) {
            strncpy(g_config.clientSecret, elementKey,
                    sizeof(g_config.clientSecret) - 1);
            g_config.clientSecret[sizeof(g_config.clientSecret) - 1] = '\0';
        }
        return NULL;
    }

    if (errorMsg && errorMsgSize > 0)
        snprintf(errorMsg, errorMsgSize, "unknown command: %s", command);
    return NULL;
}

static bool ecExecuteCommand(const char *path, const char *command,
                              const char *selection) {
    (void)path;
    (void)command;
    (void)selection;
    return true;
}

// Provider singleton
static Provider *g_provider = NULL;
static void (*g_originalInit)(struct Provider *self) = NULL;

static void ecInit(struct Provider *self) {
    if (g_originalInit) g_originalInit(self);
    emailclientGlobalInit();

    // Load config from settings.json
    char *configPath = providerGetMainConfigPath();
    if (configPath) {
        json_object *root = json_object_from_file(configPath);
        if (root) {
            json_object *section;
            if (json_object_object_get_ex(root, "email client", &section)) {
                json_object *val;
                if (json_object_object_get_ex(section, "emailImapUrl", &val)) {
                    const char *s = json_object_get_string(val);
                    if (s) {
                        strncpy(g_config.imapUrl, s,
                                sizeof(g_config.imapUrl) - 1);
                        g_config.imapUrl[sizeof(g_config.imapUrl) - 1] = '\0';
                    }
                }
                if (json_object_object_get_ex(section, "emailSmtpUrl", &val)) {
                    const char *s = json_object_get_string(val);
                    if (s) {
                        strncpy(g_config.smtpUrl, s,
                                sizeof(g_config.smtpUrl) - 1);
                        g_config.smtpUrl[sizeof(g_config.smtpUrl) - 1] = '\0';
                    }
                }
                if (json_object_object_get_ex(section, "emailUsername", &val)) {
                    const char *s = json_object_get_string(val);
                    if (s) {
                        strncpy(g_config.username, s,
                                sizeof(g_config.username) - 1);
                        g_config.username[sizeof(g_config.username) - 1] = '\0';
                    }
                }
                if (json_object_object_get_ex(section, "emailPassword", &val)) {
                    const char *s = json_object_get_string(val);
                    if (s) {
                        strncpy(g_config.password, s,
                                sizeof(g_config.password) - 1);
                        g_config.password[sizeof(g_config.password) - 1] = '\0';
                    }
                }
                if (json_object_object_get_ex(section, "emailClientId", &val)) {
                    const char *s = json_object_get_string(val);
                    if (s) {
                        strncpy(g_config.clientId, s,
                                sizeof(g_config.clientId) - 1);
                        g_config.clientId[sizeof(g_config.clientId) - 1] = '\0';
                    }
                }
                if (json_object_object_get_ex(section, "emailClientSecret", &val)) {
                    const char *s = json_object_get_string(val);
                    if (s) {
                        strncpy(g_config.clientSecret, s,
                                sizeof(g_config.clientSecret) - 1);
                        g_config.clientSecret[sizeof(g_config.clientSecret) - 1] = '\0';
                    }
                }
                if (json_object_object_get_ex(section, "emailOAuthAccessToken", &val)) {
                    const char *s = json_object_get_string(val);
                    if (s) {
                        strncpy(g_config.oauthAccessToken, s,
                                sizeof(g_config.oauthAccessToken) - 1);
                        g_config.oauthAccessToken[sizeof(g_config.oauthAccessToken) - 1] = '\0';
                    }
                }
                if (json_object_object_get_ex(section, "emailOAuthRefreshToken", &val)) {
                    const char *s = json_object_get_string(val);
                    if (s) {
                        strncpy(g_config.oauthRefreshToken, s,
                                sizeof(g_config.oauthRefreshToken) - 1);
                        g_config.oauthRefreshToken[sizeof(g_config.oauthRefreshToken) - 1] = '\0';
                    }
                }
                if (json_object_object_get_ex(section, "emailTokenExpiry", &val)) {
                    g_config.tokenExpiry = json_object_get_int64(val);
                }
            }
            json_object_put(root);
        }
        free(configPath);
    }
}

static void ecCleanup(struct Provider *self) {
    (void)self;
    emailclientGlobalCleanup();
}

Provider* emailclientGetProvider(void) {
    if (!g_provider) {
        static ProviderOps ops = {
            .name = "emailclient",
            .displayName = "email client",
            .fetch = ecFetch,
            .commit = ecCommit,
            .createDirectory = NULL,
            .createFile = NULL,
            .deleteItem = NULL,
            .copyItem = NULL,
            .getCommands = ecGetCommands,
            .handleCommand = ecHandleCommand,
            .getCommandListItems = NULL,
            .executeCommand = ecExecuteCommand,
            .collectDeepSearchItems = NULL,
        };
        g_provider = providerCreate(&ops);
        g_originalInit = g_provider->init;
        g_provider->init = ecInit;
        g_provider->cleanup = ecCleanup;
        g_provider->noCache = true;
    }
    return g_provider;
}

__attribute__((constructor))
static void emailclientRegisterFactory(void) {
    providerFactoryRegister("email client", emailclientGetProvider);
}

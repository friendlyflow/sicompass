#include <win_compat.h>
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

// Set after send — compose/reply fetch returns confirmation instead of form
static bool g_composeSent = false;

// Pending compose state (persists across commit calls within one compose)
static struct {
    char to[256];
    char subject[512];
    char body[8192];
    char replyFolder[256]; // folder of message being replied to
    int replyUid;          // UID of message being replied to
} g_pendingCompose;

// Split body text into FFON string elements by newlines.
// Caller must free the returned array (elements are owned by caller).
static void addBodyLines(FfonObject *parent, const char *body) {
    const char *p = body;
    while (*p) {
        const char *eol = strchr(p, '\n');
        if (eol) {
            int len = (int)(eol - p);
            if (len > 0 && p[len - 1] == '\r') len--;
            char line[4096];
            if (len >= (int)sizeof(line)) len = sizeof(line) - 1;
            memcpy(line, p, len);
            line[len] = '\0';
            ffonObjectAddElement(parent, ffonElementCreateString(line));
            p = eol + 1;
        } else {
            ffonObjectAddElement(parent, ffonElementCreateString(p));
            break;
        }
    }
}

// Build a History FFON object from References header.
// Follows the chain of Message-IDs and fetches each referenced message.
static FfonElement* ecBuildHistory(const char *folder,
                                    const char *references) {
    if (!references || !references[0]) return NULL;

    FfonElement *history = ffonElementCreateObject("History");

    // Parse space-separated Message-IDs from References header
    // References: <id1> <id2> <id3> (oldest to newest)
    const char *p = references;
    int count = 0;
    while (*p && count < 10) {
        while (*p == ' ') p++;
        if (*p != '<') { p++; continue; }

        const char *end = strchr(p, '>');
        if (!end) break;

        int len = (int)(end - p + 1); // include < and >
        char msgId[256];
        if (len >= (int)sizeof(msgId)) len = sizeof(msgId) - 1;
        memcpy(msgId, p, len);
        msgId[len] = '\0';

        EmailMessage *ref = emailclientFetchMessageByMessageId(
            &g_config, folder, msgId);
        if (ref) {
            char key[1024];
            snprintf(key, sizeof(key), "From: %s — Subject: %s",
                     ref->from, ref->subject);
            FfonElement *refObj = ffonElementCreateObject(key);
            addBodyLines(refObj->data.object, ref->body);
            ffonObjectAddElement(history->data.object, refObj);
            emailclientFreeMessage(ref);
            count++;
        }

        p = end + 1;
    }

    if (history->data.object->count == 0) {
        ffonElementDestroy(history);
        return NULL;
    }
    return history;
}

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

// Build compose form elements (shared between ecFetch and ecHandleCommand).
// If replyFolder and replyUid are valid, pre-fill from the original message.
static FfonElement** ecBuildComposeForm(const char *replyFolder,
                                         int replyUid, int *outCount) {
    int maxElems = 6; // From, To, Subject, Body, Send, History
    FfonElement **elems = malloc(maxElems * sizeof(FfonElement*));
    int idx = 0;

    // Pre-fill from original message only if fields are still empty
    // (first call after memset in ecHandleCommand)
    // Pre-fill To from original sender only if fields are still empty
    if (replyFolder && replyUid > 0 && !g_pendingCompose.to[0]) {
        EmailMessage *msg = emailclientFetchMessage(
            &g_config, replyFolder, replyUid);
        if (msg) {
            strncpy(g_pendingCompose.to, msg->from,
                    sizeof(g_pendingCompose.to) - 1);
            g_pendingCompose.to[sizeof(g_pendingCompose.to) - 1] = '\0';

            const char *subj = msg->subject;
            if (strncasecmp(subj, "Re: ", 4) != 0)
                snprintf(g_pendingCompose.subject,
                         sizeof(g_pendingCompose.subject),
                         "Re: %s", subj);
            else
                strncpy(g_pendingCompose.subject, subj,
                        sizeof(g_pendingCompose.subject) - 1);
            g_pendingCompose.subject[
                sizeof(g_pendingCompose.subject) - 1] = '\0';

            emailclientFreeMessage(msg);
        }
    }

    // Always reflect g_pendingCompose values in inputs
    // From is always the user's own address
    char fromBuf[300], toBuf[300], subjBuf[560], bodyBuf[8300];
    snprintf(fromBuf, sizeof(fromBuf), "From: %s", g_config.username);
    elems[idx++] = ffonElementCreateString(fromBuf);
    snprintf(toBuf, sizeof(toBuf),
             "To: <input>%s</input>", g_pendingCompose.to);
    elems[idx++] = ffonElementCreateString(toBuf);
    snprintf(subjBuf, sizeof(subjBuf),
             "Subject: <input>%s</input>", g_pendingCompose.subject);
    elems[idx++] = ffonElementCreateString(subjBuf);
    snprintf(bodyBuf, sizeof(bodyBuf),
             "Body: <input>%s</input>", g_pendingCompose.body);
    elems[idx++] = ffonElementCreateString(bodyBuf);
    elems[idx++] = ffonElementCreateString(
        "<button>send</button>Send");

    // Thread history for reply
    if (replyFolder && replyUid > 0) {
        EmailMessage *msg = emailclientFetchMessage(
            &g_config, replyFolder, replyUid);
        if (msg) {
            FfonElement *history = ecBuildHistory(replyFolder, msg->references);
            if (history) elems[idx++] = history;
            emailclientFreeMessage(msg);
        }
    }

    *outCount = idx;
    return elems;
}

// Check if any path segment equals the given name
static bool pathContainsSegment(const char *path, const char *name) {
    if (!path || path[0] != '/') return false;
    const char *p = path + 1;
    while (*p) {
        const char *slash = strchr(p, '/');
        int len = slash ? (int)(slash - p) : (int)strlen(p);
        if (len == (int)strlen(name) && strncmp(p, name, len) == 0)
            return true;
        if (!slash) break;
        p = slash + 1;
    }
    return false;
}

static FfonElement** ecFetch(const char *path, int *outCount) {
    // Handle compose path at any depth — the compose object's children
    // are served here because noCache=true causes re-fetch on navigation.
    if (pathContainsSegment(path, "compose") ||
        pathContainsSegment(path, "reply")) {
        if (g_composeSent) {
            *outCount = 1;
            FfonElement **elems = malloc(sizeof(FfonElement*));
            elems[0] = ffonElementCreateString("message sent");
            return elems;
        }
        return ecBuildComposeForm(
            g_pendingCompose.replyFolder[0] ? g_pendingCompose.replyFolder : NULL,
            g_pendingCompose.replyUid, outCount);
    }

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
            *outCount = 2;
            FfonElement **elems = malloc(2 * sizeof(FfonElement*));
            elems[0] = ffonElementCreateString("no folders found");
            elems[1] = ffonElementCreateObject("compose");
            return elems;
        }

        FfonElement **elems = malloc((folderCount + 1) * sizeof(FfonElement*));
        int idx = 0;
        // Insert compose right after INBOX
        bool composeInserted = false;
        for (int i = 0; i < folderCount; i++) {
            elems[idx++] = ffonElementCreateObject(folders[i].name);
            if (!composeInserted &&
                strcasecmp(folders[i].name, "INBOX") == 0) {
                elems[idx++] = ffonElementCreateObject("compose");
                composeInserted = true;
            }
        }
        if (!composeInserted)
            elems[idx++] = ffonElementCreateObject("compose");
        ecStoreFolderMappings(folders, folderCount);
        emailclientFreeFolders(folders, folderCount);
        *outCount = idx;
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
        // Inside a message: structured view
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

        // Build structured view: From, To, Date, Subject, Body{}, History{}
        int maxElems = 6; // from, to, date, subject, body obj, history obj
        FfonElement **elems = malloc(maxElems * sizeof(FfonElement*));
        int idx = 0;

        char buf[1024];
        snprintf(buf, sizeof(buf), "From: %s", msg->from);
        elems[idx++] = ffonElementCreateString(buf);
        snprintf(buf, sizeof(buf), "To: %s", msg->to);
        elems[idx++] = ffonElementCreateString(buf);
        snprintf(buf, sizeof(buf), "Date: %s", msg->date);
        elems[idx++] = ffonElementCreateString(buf);
        snprintf(buf, sizeof(buf), "Subject: %s", msg->subject);
        elems[idx++] = ffonElementCreateString(buf);

        // Body as navigable object
        FfonElement *bodyObj = ffonElementCreateObject("Body");
        addBodyLines(bodyObj->data.object, msg->body);
        elems[idx++] = bodyObj;

        // Thread history from References header
        FfonElement *history = ecBuildHistory(folder, msg->references);
        if (history) {
            elems[idx++] = history;
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
    (void)oldName;
    if (!path || !newName) return false;

    // Check if we're inside a compose object by looking at the path
    // The commit flow pushes the label prefix as a path segment,
    // e.g. path becomes "/compose/To" when editing the To field
    const char *lastSlash = strrchr(path, '/');
    if (!lastSlash) return false;
    const char *field = lastSlash + 1;

    if (strcmp(field, "To") == 0) {
        strncpy(g_pendingCompose.to, newName,
                sizeof(g_pendingCompose.to) - 1);
        g_pendingCompose.to[sizeof(g_pendingCompose.to) - 1] = '\0';
        return true;
    }
    if (strcmp(field, "Subject") == 0) {
        strncpy(g_pendingCompose.subject, newName,
                sizeof(g_pendingCompose.subject) - 1);
        g_pendingCompose.subject[sizeof(g_pendingCompose.subject) - 1] = '\0';
        return true;
    }
    if (strcmp(field, "Body") == 0) {
        strncpy(g_pendingCompose.body, newName,
                sizeof(g_pendingCompose.body) - 1);
        g_pendingCompose.body[sizeof(g_pendingCompose.body) - 1] = '\0';
        return true;
    }

    return false;
}

static const char *ec_commands[] = {"compose", "reply", "refresh", "login", "logout"};

static const char** ecGetCommands(int *outCount) {
    *outCount = 5;
    return ec_commands;
}

static FfonElement* ecHandleCommand(const char *path, const char *command,
                                     const char *elementKey, int elementType,
                                     char *errorMsg, int errorMsgSize) {
    (void)elementType;

    if (strcmp(command, "compose") == 0) {
        // Reset compose state; the compose object is always present at root
        // level — the app layer navigates to it.
        memset(&g_pendingCompose, 0, sizeof(g_pendingCompose));
        g_composeSent = false;
        return NULL;
    }
    if (strcmp(command, "reply") == 0) {
        memset(&g_pendingCompose, 0, sizeof(g_pendingCompose));
        g_composeSent = false;

        // Store reply context
        if (pathDepth(path) >= 2) {
            char folderSeg[256], msgSeg[512];
            pathFirstSegment(path, folderSeg, sizeof(folderSeg));
            pathSecondSegment(path, msgSeg, sizeof(msgSeg));
            const char *realFolder = ecLookupFolderName(folderSeg);
            if (!realFolder) realFolder = folderSeg;
            strncpy(g_pendingCompose.replyFolder, realFolder,
                    sizeof(g_pendingCompose.replyFolder) - 1);
            g_pendingCompose.replyFolder[
                sizeof(g_pendingCompose.replyFolder) - 1] = '\0';
            g_pendingCompose.replyUid = ecLookupUid(msgSeg);
        }

        // Build reply object with children from the shared helper
        int childCount = 0;
        FfonElement **children = ecBuildComposeForm(
            g_pendingCompose.replyFolder[0] ? g_pendingCompose.replyFolder : NULL,
            g_pendingCompose.replyUid, &childCount);

        FfonElement *compose = ffonElementCreateObject("reply");
        for (int i = 0; i < childCount; i++) {
            ffonObjectAddElement(compose->data.object, children[i]);
        }
        free(children);

        return compose;
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

static void ecOnButtonPress(struct Provider *self, const char *functionName) {
    (void)self;
    if (!functionName) return;

    if (strcmp(functionName, "send") == 0) {
        if (!g_pendingCompose.to[0]) return;
        emailclientSendMessage(&g_config,
                                g_pendingCompose.to,
                                g_pendingCompose.subject,
                                g_pendingCompose.body);
        memset(&g_pendingCompose, 0, sizeof(g_pendingCompose));
        g_composeSent = true;
    }
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
        g_provider->onButtonPress = ecOnButtonPress;
    }
    return g_provider;
}

GCC_CONSTRUCTOR(emailclientRegisterFactory) {
    providerFactoryRegister("email client", emailclientGetProvider);
}

#include <win_compat.h>
#include "emailclient.h"
#include "emailclient_oauth2.h"
#include <curl/curl.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <time.h>

typedef struct {
    char *data;
    size_t size;
    size_t capacity;
} ResponseBuffer;

static size_t curlWriteCallback(char *ptr, size_t size, size_t nmemb,
                                 void *userdata) {
    ResponseBuffer *buf = (ResponseBuffer *)userdata;
    size_t total = size * nmemb;
    if (buf->size + total >= buf->capacity) {
        size_t newCap = (buf->capacity == 0) ? 4096 : buf->capacity * 2;
        while (newCap < buf->size + total + 1) newCap *= 2;
        char *newData = realloc(buf->data, newCap);
        if (!newData) return 0;
        buf->data = newData;
        buf->capacity = newCap;
    }
    memcpy(buf->data + buf->size, ptr, total);
    buf->size += total;
    buf->data[buf->size] = '\0';
    return total;
}

// Read callback for SMTP upload
typedef struct {
    const char *data;
    size_t size;
    size_t pos;
} UploadBuffer;

static size_t curlReadCallback(char *ptr, size_t size, size_t nmemb,
                                void *userdata) {
    UploadBuffer *buf = (UploadBuffer *)userdata;
    size_t room = size * nmemb;
    size_t remaining = buf->size - buf->pos;
    size_t toSend = remaining < room ? remaining : room;
    memcpy(ptr, buf->data + buf->pos, toSend);
    buf->pos += toSend;
    return toSend;
}

void emailclientGlobalInit(void) {
    curl_global_init(CURL_GLOBAL_DEFAULT);
}

// Persistent CURL handle — reused across operations to avoid TLS/auth overhead.
// curl_easy_reset() clears options but preserves the connection pool internally.
static CURL *g_imapCurl = NULL;

static CURL *getImapHandle(void) {
    if (g_imapCurl) {
        curl_easy_reset(g_imapCurl);
    } else {
        g_imapCurl = curl_easy_init();
    }
    return g_imapCurl;
}

static void releaseImapHandleOnError(void) {
    if (g_imapCurl) {
        curl_easy_cleanup(g_imapCurl);
        g_imapCurl = NULL;
    }
}

void emailclientGlobalCleanup(void) {
    releaseImapHandleOnError();
    curl_global_cleanup();
}

// Refresh OAuth2 token if expired; no-op for password auth
static bool ensureOAuth2Token(EmailClientConfig *config) {
    if (!config->oauthAccessToken[0]) return true; // password mode
    if (time(NULL) < config->tokenExpiry - 60) return true; // still valid
    if (!config->oauthRefreshToken[0]) return false;

    OAuth2TokenResult result = emailclientOAuth2RefreshToken(
        config->clientId, config->clientSecret, config->oauthRefreshToken);
    if (!result.success) return false;

    strncpy(config->oauthAccessToken, result.accessToken,
            sizeof(config->oauthAccessToken) - 1);
    config->oauthAccessToken[sizeof(config->oauthAccessToken) - 1] = '\0';
    config->tokenExpiry = time(NULL) + result.expiresIn;
    return true;
}

// Apply authentication to a curl handle
static void applyAuth(CURL *curl, const EmailClientConfig *config) {
    curl_easy_setopt(curl, CURLOPT_USERNAME, config->username);
    if (config->oauthAccessToken[0]) {
        curl_easy_setopt(curl, CURLOPT_XOAUTH2_BEARER,
                         config->oauthAccessToken);
        curl_easy_setopt(curl, CURLOPT_SASL_IR, 1L);
        curl_easy_setopt(curl, CURLOPT_LOGIN_OPTIONS, "AUTH=XOAUTH2");
    } else {
        curl_easy_setopt(curl, CURLOPT_PASSWORD, config->password);
    }
}

// URL-encode a folder name for IMAP URLs (spaces, special chars)
static void urlEncodeFolder(const char *folder, char *out, int outSize) {
    const char *src = folder;
    char *dst = out;
    char *end = out + outSize - 4;
    while (*src && dst < end) {
        if ((*src >= 'A' && *src <= 'Z') || (*src >= 'a' && *src <= 'z') ||
            (*src >= '0' && *src <= '9') || *src == '-' || *src == '_' ||
            *src == '.' || *src == '~' || *src == '/') {
            *dst++ = *src;
        } else {
            snprintf(dst, 4, "%%%02X", (unsigned char)*src);
            dst += 3;
        }
        src++;
    }
    *dst = '\0';
}

// Parse a single IMAP LIST response line to extract folder name.
// Format: * LIST (\flags) "delimiter" "FolderName"
// or:     * LIST (\flags) "delimiter" FolderName
static bool parseListLine(const char *line, char *outName, int outNameSize) {
    // Find the delimiter field (quoted single char)
    const char *p = strstr(line, ") ");
    if (!p) return false;
    p += 2; // skip ") "

    // Skip the delimiter (e.g., "/" or ".")
    if (*p == '"') {
        p = strchr(p + 1, '"');
        if (!p) return false;
        p++; // skip closing quote
    } else if (*p == 'N') {
        // NIL delimiter
        p += 3;
    }
    while (*p == ' ') p++;

    // Extract folder name (possibly quoted)
    if (*p == '"') {
        p++;
        const char *end = strchr(p, '"');
        if (!end) return false;
        int len = end - p;
        if (len >= outNameSize) len = outNameSize - 1;
        memcpy(outName, p, len);
        outName[len] = '\0';
    } else {
        // Unquoted
        int len = strlen(p);
        // Trim trailing \r\n
        while (len > 0 && (p[len - 1] == '\r' || p[len - 1] == '\n'))
            len--;
        if (len >= outNameSize) len = outNameSize - 1;
        memcpy(outName, p, len);
        outName[len] = '\0';
    }
    return outName[0] != '\0';
}

EmailFolder* emailclientListFolders(EmailClientConfig *config,
                                     int *outCount) {
    *outCount = 0;
    if (!config || !config->imapUrl[0] || !config->username[0]) return NULL;
    if (!ensureOAuth2Token(config)) return NULL;

    CURL *curl = getImapHandle();
    if (!curl) return NULL;

    char url[1024];
    snprintf(url, sizeof(url), "%s/", config->imapUrl);

    ResponseBuffer buf = {NULL, 0, 0};
    curl_easy_setopt(curl, CURLOPT_URL, url);
    applyAuth(curl, config);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curlWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

    CURLcode res = curl_easy_perform(curl);

    if (res != CURLE_OK || !buf.data) {
        if (res != CURLE_OK) releaseImapHandleOnError();
        free(buf.data);
        return NULL;
    }

    // Count lines to estimate folder count
    int lineCount = 0;
    for (size_t i = 0; i < buf.size; i++) {
        if (buf.data[i] == '\n') lineCount++;
    }
    if (lineCount == 0) lineCount = 1;

    EmailFolder *folders = calloc(lineCount, sizeof(EmailFolder));
    if (!folders) {
        free(buf.data);
        return NULL;
    }

    int count = 0;
    char *line = buf.data;
    while (line && *line) {
        char *eol = strchr(line, '\n');
        if (eol) *eol = '\0';

        char name[256];
        if (parseListLine(line, name, sizeof(name))) {
            strncpy(folders[count].name, name, sizeof(folders[count].name) - 1);
            folders[count].name[sizeof(folders[count].name) - 1] = '\0';
            count++;
        }

        line = eol ? eol + 1 : NULL;
    }

    free(buf.data);

    if (count == 0) {
        free(folders);
        return NULL;
    }

    *outCount = count;
    return folders;
}

void emailclientFreeFolders(EmailFolder *folders, int count) {
    (void)count;
    free(folders);
}

// --- IMAP ENVELOPE response parsers ---
// ENVELOPE returns all fields inline as quoted strings (no literals),
// so libcurl delivers the complete line to our write callback.

static const char *envSkipWs(const char *p) {
    while (*p == ' ' || *p == '\t') p++;
    return p;
}

// Parse a quoted string or NIL into out[outSize]. Returns pointer past field.
static const char *envParseStr(const char *p, char *out, int outSize) {
    p = envSkipWs(p);
    if (*p == '"') {
        p++;
        int len = 0;
        while (*p && *p != '"') {
            if (*p == '\\' && *(p + 1)) p++;
            if (out && len < outSize - 1) out[len++] = *p;
            p++;
        }
        if (out) out[len] = '\0';
        if (*p == '"') p++;
    } else {
        if (out && outSize > 0) out[0] = '\0';
        while (*p && *p != ' ' && *p != ')' && *p != '(') p++;
    }
    return p;
}

// Skip one field: quoted string, NIL/token, or parenthesized group.
static const char *envSkipField(const char *p) {
    p = envSkipWs(p);
    if (*p == '"') {
        p++;
        while (*p && *p != '"') { if (*p == '\\' && *(p + 1)) p++; p++; }
        if (*p == '"') p++;
    } else if (*p == '(') {
        int depth = 1; p++;
        while (*p && depth > 0) {
            if (*p == '"') {
                p++;
                while (*p && *p != '"') { if (*p == '\\' && *(p+1)) p++; p++; }
                if (*p == '"') p++;
            } else if (*p == '(') { depth++; p++; }
            else if (*p == ')') { depth--; p++; }
            else p++;
        }
    } else {
        while (*p && *p != ' ' && *p != ')') p++;
    }
    return p;
}

// Parse first address from IMAP address list ((name adl mailbox host) ...) or NIL.
static const char *envParseFrom(const char *p, char *out, int outSize) {
    p = envSkipWs(p);
    if (strncmp(p, "NIL", 3) == 0) {
        if (out && outSize > 0) out[0] = '\0';
        return p + 3;
    }
    if (*p != '(') { if (out && outSize > 0) out[0] = '\0'; return p; }
    p++; // outer (
    p = envSkipWs(p);
    if (*p != '(') {
        while (*p && *p != ')') p++;
        if (*p == ')') p++;
        if (out && outSize > 0) out[0] = '\0';
        return p;
    }
    p++; // first address (
    char name[256] = ""; char mailbox[256] = ""; char host[256] = "";
    p = envParseStr(p, name, sizeof(name));
    p = envSkipField(p);  // adl (usually NIL)
    p = envParseStr(p, mailbox, sizeof(mailbox));
    p = envParseStr(p, host, sizeof(host));
    p = envSkipWs(p);
    if (*p == ')') p++;  // first address )
    // skip remaining addresses in the list
    p = envSkipWs(p);
    while (*p && *p != ')') {
        const char *prev = p;
        p = envSkipField(p);
        if (p == prev) break;
        p = envSkipWs(p);
    }
    if (*p == ')') p++;  // outer )
    if (out) {
        if (name[0] && mailbox[0] && host[0])
            snprintf(out, outSize, "%s <%s@%s>", name, mailbox, host);
        else if (mailbox[0] && host[0])
            snprintf(out, outSize, "%s@%s", mailbox, host);
        else
            strncpy(out, name, outSize - 1);
    }
    return p;
}

// Parse IMAP ENVELOPE ("date" "subject" from ...) into EmailHeader.
// p must point to the opening '(' of the ENVELOPE.
static void parseEnvelope(const char *p, EmailHeader *out) {
    p = envSkipWs(p);
    if (*p != '(') return;
    p++;
    p = envParseStr(p, out->date, sizeof(out->date));
    p = envParseStr(p, out->subject, sizeof(out->subject));
    envParseFrom(p, out->from, sizeof(out->from));
}

EmailHeader* emailclientListMessages(EmailClientConfig *config,
                                      const char *folder, int limit,
                                      int *outCount) {
    *outCount = 0;
    if (!config || !config->imapUrl[0] || !config->username[0]) return NULL;
    if (!folder || !folder[0]) return NULL;
    if (!ensureOAuth2Token(config)) return NULL;

    CURL *curl = getImapHandle();
    if (!curl) return NULL;

    char encodedFolder[512];
    urlEncodeFolder(folder, encodedFolder, sizeof(encodedFolder));

    char url[1024];
    snprintf(url, sizeof(url), "%s/%s", config->imapUrl, encodedFolder);

    ResponseBuffer buf = {NULL, 0, 0};
    curl_easy_setopt(curl, CURLOPT_URL, url);
    applyAuth(curl, config);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curlWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

    // FETCH headers and UIDs for messages
    char fetchCmd[256];
    // Use ENVELOPE: returns all fields inline as quoted strings (no IMAP
    // literals), so libcurl delivers the complete response line to our
    // write callback. BODY[HEADER.FIELDS] uses literals which libcurl
    // silently discards when using CURLOPT_CUSTOMREQUEST.
    snprintf(fetchCmd, sizeof(fetchCmd), "FETCH 1:%d (UID ENVELOPE)", limit);
    curl_easy_setopt(curl, CURLOPT_CUSTOMREQUEST, fetchCmd);

    CURLcode res = curl_easy_perform(curl);

    if (res != CURLE_OK || !buf.data) {
        if (res != CURLE_OK) releaseImapHandleOnError();
        free(buf.data);
        return NULL;
    }

    // Parse FETCH response: each * N FETCH line contains UID and ENVELOPE.
    EmailHeader *headers = calloc(limit, sizeof(EmailHeader));
    if (!headers) {
        free(buf.data);
        return NULL;
    }

    int count = 0;
    char *pos = buf.data;
    while (pos && *pos && count < limit) {
        if (pos[0] != '*' || pos[1] != ' ') {
            char *eol = strchr(pos, '\n');
            pos = eol ? eol + 1 : NULL;
            continue;
        }

        char *lineEnd = strchr(pos, '\n');
        if (!lineEnd) break;

        char *uidStr = strstr(pos, "UID ");
        int uid = (uidStr && uidStr < lineEnd) ? atoi(uidStr + 4) : 0;

        char *envMarker = strstr(pos, "ENVELOPE ");
        if (envMarker && envMarker < lineEnd && uid > 0) {
            EmailHeader hdr;
            memset(&hdr, 0, sizeof(hdr));
            hdr.uid = uid;
            parseEnvelope(envMarker + 9, &hdr);
            headers[count++] = hdr;
        }

        char *eol = strchr(pos, '\n');
        pos = eol ? eol + 1 : NULL;
    }

    free(buf.data);

    if (count == 0) {
        free(headers);
        return NULL;
    }

    *outCount = count;
    return headers;
}

void emailclientFreeHeaders(EmailHeader *headers, int count) {
    (void)count;
    free(headers);
}

EmailMessage* emailclientFetchMessage(EmailClientConfig *config,
                                       const char *folder, int uid) {
    if (!config || !config->imapUrl[0] || !config->username[0]) return NULL;
    if (!folder || !folder[0]) return NULL;
    if (!ensureOAuth2Token(config)) return NULL;

    CURL *curl = getImapHandle();
    if (!curl) return NULL;

    char encodedFolder[512];
    urlEncodeFolder(folder, encodedFolder, sizeof(encodedFolder));

    // libcurl IMAP URL to fetch specific message by UID
    char url[2048];
    snprintf(url, sizeof(url), "%s/%s/;UID=%d",
             config->imapUrl, encodedFolder, uid);

    ResponseBuffer buf = {NULL, 0, 0};
    curl_easy_setopt(curl, CURLOPT_URL, url);
    applyAuth(curl, config);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curlWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

    CURLcode res = curl_easy_perform(curl);
    if (res != CURLE_OK) releaseImapHandleOnError();

    if (res != CURLE_OK || !buf.data) {
        free(buf.data);
        return NULL;
    }

    EmailMessage *msg = calloc(1, sizeof(EmailMessage));
    if (!msg) {
        free(buf.data);
        return NULL;
    }

    msg->uid = uid;

    // Parse raw email: headers are separated from body by blank line
    char *bodyStart = strstr(buf.data, "\r\n\r\n");
    if (!bodyStart) bodyStart = strstr(buf.data, "\n\n");

    if (bodyStart) {
        // Extract From, Subject, Date from header portion
        char *headerBlock = buf.data;
        char *p = headerBlock;
        char *hEnd = bodyStart;
        while (p < hEnd) {
            char *eol = memchr(p, '\n', hEnd - p);
            if (!eol) eol = hEnd;
            int lineLen = eol - p;
            if (lineLen > 0 && p[lineLen - 1] == '\r') lineLen--;

            if (lineLen > 6 && strncasecmp(p, "From: ", 6) == 0) {
                int len = lineLen - 6;
                if (len >= (int)sizeof(msg->from))
                    len = sizeof(msg->from) - 1;
                memcpy(msg->from, p + 6, len);
                msg->from[len] = '\0';
            } else if (lineLen > 9 &&
                       strncasecmp(p, "Subject: ", 9) == 0) {
                int len = lineLen - 9;
                if (len >= (int)sizeof(msg->subject))
                    len = sizeof(msg->subject) - 1;
                memcpy(msg->subject, p + 9, len);
                msg->subject[len] = '\0';
            } else if (lineLen > 6 && strncasecmp(p, "Date: ", 6) == 0) {
                int len = lineLen - 6;
                if (len >= (int)sizeof(msg->date))
                    len = sizeof(msg->date) - 1;
                memcpy(msg->date, p + 6, len);
                msg->date[len] = '\0';
            } else if (lineLen > 4 && strncasecmp(p, "To: ", 4) == 0) {
                int len = lineLen - 4;
                if (len >= (int)sizeof(msg->to))
                    len = sizeof(msg->to) - 1;
                memcpy(msg->to, p + 4, len);
                msg->to[len] = '\0';
            } else if (lineLen > 12 &&
                       strncasecmp(p, "Message-ID: ", 12) == 0) {
                int len = lineLen - 12;
                if (len >= (int)sizeof(msg->messageId))
                    len = sizeof(msg->messageId) - 1;
                memcpy(msg->messageId, p + 12, len);
                msg->messageId[len] = '\0';
            } else if (lineLen > 13 &&
                       strncasecmp(p, "In-Reply-To: ", 13) == 0) {
                int len = lineLen - 13;
                if (len >= (int)sizeof(msg->inReplyTo))
                    len = sizeof(msg->inReplyTo) - 1;
                memcpy(msg->inReplyTo, p + 13, len);
                msg->inReplyTo[len] = '\0';
            } else if (lineLen > 12 &&
                       strncasecmp(p, "References: ", 12) == 0) {
                int len = lineLen - 12;
                if (len >= (int)sizeof(msg->references))
                    len = sizeof(msg->references) - 1;
                memcpy(msg->references, p + 12, len);
                msg->references[len] = '\0';
            }
            p = eol + 1;
        }

        // Skip blank line separator
        bodyStart += (bodyStart[0] == '\r') ? 4 : 2;

        // Copy body
        int bodyLen = buf.size - (bodyStart - buf.data);
        if (bodyLen >= (int)sizeof(msg->body))
            bodyLen = sizeof(msg->body) - 1;
        if (bodyLen > 0) {
            memcpy(msg->body, bodyStart, bodyLen);
            msg->body[bodyLen] = '\0';
        }
    }

    free(buf.data);
    return msg;
}

void emailclientFreeMessage(EmailMessage *msg) {
    free(msg);
}

EmailMessage* emailclientFetchMessageByMessageId(
    EmailClientConfig *config, const char *folder, const char *messageId) {
    if (!config || !config->imapUrl[0] || !config->username[0]) return NULL;
    if (!folder || !folder[0] || !messageId || !messageId[0]) return NULL;
    if (!ensureOAuth2Token(config)) return NULL;

    CURL *curl = getImapHandle();
    if (!curl) return NULL;

    char encodedFolder[512];
    urlEncodeFolder(folder, encodedFolder, sizeof(encodedFolder));

    char url[1024];
    snprintf(url, sizeof(url), "%s/%s", config->imapUrl, encodedFolder);

    ResponseBuffer buf = {NULL, 0, 0};
    curl_easy_setopt(curl, CURLOPT_URL, url);
    applyAuth(curl, config);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curlWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

    // IMAP SEARCH by Message-ID header
    char searchCmd[512];
    snprintf(searchCmd, sizeof(searchCmd),
             "SEARCH HEADER Message-ID \"%s\"", messageId);
    curl_easy_setopt(curl, CURLOPT_CUSTOMREQUEST, searchCmd);

    CURLcode res = curl_easy_perform(curl);

    if (res != CURLE_OK || !buf.data) {
        if (res != CURLE_OK) releaseImapHandleOnError();
        free(buf.data);
        return NULL;
    }

    // Parse "* SEARCH <uid1> <uid2> ..." response
    int uid = 0;
    char *searchLine = strstr(buf.data, "* SEARCH");
    if (searchLine) {
        char *p = searchLine + 8; // skip "* SEARCH"
        while (*p == ' ') p++;
        if (*p >= '0' && *p <= '9') uid = atoi(p);
    }
    free(buf.data);

    if (uid <= 0) return NULL;

    return emailclientFetchMessage(config, folder, uid);
}

// Extract bare email from "Name <addr>" or "<addr>" format.
// If no angle brackets, returns the input as-is.
static const char* extractBareEmail(const char *addr, char *out, int outSize) {
    const char *lt = strchr(addr, '<');
    const char *gt = lt ? strchr(lt, '>') : NULL;
    if (lt && gt && gt > lt + 1) {
        int len = (int)(gt - lt - 1);
        if (len >= outSize) len = outSize - 1;
        memcpy(out, lt + 1, len);
        out[len] = '\0';
        return out;
    }
    return addr;
}

bool emailclientSendMessage(EmailClientConfig *config,
                             const char *to, const char *subject,
                             const char *body) {
    if (!config || !config->smtpUrl[0] || !config->username[0]) return false;
    if (!to || !to[0] || !body) return false;
    if (!ensureOAuth2Token(config)) return false;

    CURL *curl = curl_easy_init();
    if (!curl) return false;

    // Extract bare email for SMTP envelope
    char bareAddr[256];
    const char *rcpt = extractBareEmail(to, bareAddr, sizeof(bareAddr));

    // Build RFC 5322 message
    char payload[16384];
    snprintf(payload, sizeof(payload),
             "From: %s\r\n"
             "To: %s\r\n"
             "Subject: %s\r\n"
             "\r\n"
             "%s\r\n",
             config->username, to, subject ? subject : "", body);

    UploadBuffer upload = {payload, strlen(payload), 0};

    struct curl_slist *recipients = NULL;
    recipients = curl_slist_append(recipients, rcpt);

    curl_easy_setopt(curl, CURLOPT_URL, config->smtpUrl);
    applyAuth(curl, config);
    curl_easy_setopt(curl, CURLOPT_MAIL_FROM, config->username);
    curl_easy_setopt(curl, CURLOPT_MAIL_RCPT, recipients);
    curl_easy_setopt(curl, CURLOPT_READFUNCTION, curlReadCallback);
    curl_easy_setopt(curl, CURLOPT_READDATA, &upload);
    curl_easy_setopt(curl, CURLOPT_UPLOAD, 1L);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

    CURLcode res = curl_easy_perform(curl);
    curl_slist_free_all(recipients);
    curl_easy_cleanup(curl);

    return (res == CURLE_OK);
}

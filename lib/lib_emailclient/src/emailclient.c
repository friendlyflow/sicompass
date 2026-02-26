#include "emailclient.h"
#include <curl/curl.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

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

void emailclientGlobalCleanup(void) {
    curl_global_cleanup();
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

EmailFolder* emailclientListFolders(const EmailClientConfig *config,
                                     int *outCount) {
    *outCount = 0;
    if (!config || !config->imapUrl[0] || !config->username[0]) return NULL;

    CURL *curl = curl_easy_init();
    if (!curl) return NULL;

    char url[1024];
    snprintf(url, sizeof(url), "%s/", config->imapUrl);

    ResponseBuffer buf = {NULL, 0, 0};
    curl_easy_setopt(curl, CURLOPT_URL, url);
    curl_easy_setopt(curl, CURLOPT_USERNAME, config->username);
    curl_easy_setopt(curl, CURLOPT_PASSWORD, config->password);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curlWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

    CURLcode res = curl_easy_perform(curl);
    curl_easy_cleanup(curl);

    if (res != CURLE_OK || !buf.data) {
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

// Parse a FETCH response to extract headers.
// libcurl IMAP FETCH returns raw response data with header fields.
static bool parseHeaderBlock(const char *block, int blockLen,
                              EmailHeader *out) {
    out->uid = 0;
    out->from[0] = '\0';
    out->subject[0] = '\0';
    out->date[0] = '\0';

    const char *end = block + blockLen;
    const char *p = block;

    while (p < end) {
        const char *eol = memchr(p, '\n', end - p);
        if (!eol) eol = end;

        int lineLen = eol - p;
        // Trim \r
        if (lineLen > 0 && p[lineLen - 1] == '\r') lineLen--;

        if (lineLen > 6 && strncasecmp(p, "From: ", 6) == 0) {
            int len = lineLen - 6;
            if (len >= (int)sizeof(out->from)) len = sizeof(out->from) - 1;
            memcpy(out->from, p + 6, len);
            out->from[len] = '\0';
        } else if (lineLen > 9 && strncasecmp(p, "Subject: ", 9) == 0) {
            int len = lineLen - 9;
            if (len >= (int)sizeof(out->subject))
                len = sizeof(out->subject) - 1;
            memcpy(out->subject, p + 9, len);
            out->subject[len] = '\0';
        } else if (lineLen > 6 && strncasecmp(p, "Date: ", 6) == 0) {
            int len = lineLen - 6;
            if (len >= (int)sizeof(out->date)) len = sizeof(out->date) - 1;
            memcpy(out->date, p + 6, len);
            out->date[len] = '\0';
        }

        p = eol + 1;
    }

    return out->from[0] || out->subject[0];
}

EmailHeader* emailclientListMessages(const EmailClientConfig *config,
                                      const char *folder, int limit,
                                      int *outCount) {
    *outCount = 0;
    if (!config || !config->imapUrl[0] || !config->username[0]) return NULL;
    if (!folder || !folder[0]) return NULL;

    CURL *curl = curl_easy_init();
    if (!curl) return NULL;

    char encodedFolder[512];
    urlEncodeFolder(folder, encodedFolder, sizeof(encodedFolder));

    char url[1024];
    snprintf(url, sizeof(url), "%s/%s", config->imapUrl, encodedFolder);

    ResponseBuffer buf = {NULL, 0, 0};
    curl_easy_setopt(curl, CURLOPT_URL, url);
    curl_easy_setopt(curl, CURLOPT_USERNAME, config->username);
    curl_easy_setopt(curl, CURLOPT_PASSWORD, config->password);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curlWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

    // FETCH headers and UIDs for messages
    char fetchCmd[256];
    snprintf(fetchCmd, sizeof(fetchCmd),
             "FETCH 1:%d (UID BODY[HEADER.FIELDS (FROM SUBJECT DATE)])", limit);
    curl_easy_setopt(curl, CURLOPT_CUSTOMREQUEST, fetchCmd);

    CURLcode res = curl_easy_perform(curl);
    curl_easy_cleanup(curl);

    if (res != CURLE_OK || !buf.data) {
        free(buf.data);
        return NULL;
    }

    // Parse FETCH response: each message block starts with "* N FETCH"
    // and contains UID and header fields
    EmailHeader *headers = calloc(limit, sizeof(EmailHeader));
    if (!headers) {
        free(buf.data);
        return NULL;
    }

    int count = 0;
    char *line = buf.data;
    while (line && *line && count < limit) {
        // Look for "* N FETCH" lines
        if (line[0] == '*' && line[1] == ' ') {
            // Try to extract UID from the response
            char *uidStr = strstr(line, "UID ");
            int uid = 0;
            if (uidStr) uid = atoi(uidStr + 4);

            // Find the header block (between { and the next *)
            char *headerStart = strchr(line, '\n');
            if (headerStart) {
                headerStart++;
                // Find end of this fetch response (next "* " or end)
                char *headerEnd = strstr(headerStart, "\r\n* ");
                if (!headerEnd) headerEnd = strstr(headerStart, "\n* ");
                if (!headerEnd) headerEnd = buf.data + buf.size;

                EmailHeader hdr;
                if (parseHeaderBlock(headerStart, headerEnd - headerStart,
                                     &hdr)) {
                    hdr.uid = uid;
                    headers[count++] = hdr;
                }

                line = headerEnd;
                continue;
            }
        }

        // Advance to next line
        char *eol = strchr(line, '\n');
        line = eol ? eol + 1 : NULL;
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

EmailMessage* emailclientFetchMessage(const EmailClientConfig *config,
                                       const char *folder, int uid) {
    if (!config || !config->imapUrl[0] || !config->username[0]) return NULL;
    if (!folder || !folder[0]) return NULL;

    CURL *curl = curl_easy_init();
    if (!curl) return NULL;

    char encodedFolder[512];
    urlEncodeFolder(folder, encodedFolder, sizeof(encodedFolder));

    // libcurl IMAP URL to fetch specific message by UID
    char url[2048];
    snprintf(url, sizeof(url), "%s/%s/;UID=%d",
             config->imapUrl, encodedFolder, uid);

    ResponseBuffer buf = {NULL, 0, 0};
    curl_easy_setopt(curl, CURLOPT_URL, url);
    curl_easy_setopt(curl, CURLOPT_USERNAME, config->username);
    curl_easy_setopt(curl, CURLOPT_PASSWORD, config->password);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curlWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

    CURLcode res = curl_easy_perform(curl);
    curl_easy_cleanup(curl);

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
        // Parse headers before the blank line
        int headerLen = bodyStart - buf.data;
        parseHeaderBlock(buf.data, headerLen, &(EmailHeader){
            .uid = uid, .from = "", .subject = "", .date = ""
        });

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

bool emailclientSendMessage(const EmailClientConfig *config,
                             const char *to, const char *subject,
                             const char *body) {
    if (!config || !config->smtpUrl[0] || !config->username[0]) return false;
    if (!to || !to[0] || !body) return false;

    CURL *curl = curl_easy_init();
    if (!curl) return false;

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
    recipients = curl_slist_append(recipients, to);

    curl_easy_setopt(curl, CURLOPT_URL, config->smtpUrl);
    curl_easy_setopt(curl, CURLOPT_USERNAME, config->username);
    curl_easy_setopt(curl, CURLOPT_PASSWORD, config->password);
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

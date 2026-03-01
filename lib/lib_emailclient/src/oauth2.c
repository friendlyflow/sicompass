#include "emailclient_oauth2.h"
#include <platform.h>
#include <curl/curl.h>
#include <json-c/json.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <unistd.h>

static const char *GOOGLE_AUTH_URL =
    "https://accounts.google.com/o/oauth2/v2/auth";
static const char *GOOGLE_TOKEN_URL =
    "https://oauth2.googleapis.com/token";
static const char *OAUTH2_SCOPE = "https://mail.google.com/";

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

static int startLocalServer(int *outPort) {
    int sockfd = socket(AF_INET, SOCK_STREAM, 0);
    if (sockfd < 0) return -1;

    int optval = 1;
    setsockopt(sockfd, SOL_SOCKET, SO_REUSEADDR, &optval, sizeof(optval));

    struct sockaddr_in addr = {0};
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    addr.sin_port = 0;

    if (bind(sockfd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        close(sockfd);
        return -1;
    }
    if (listen(sockfd, 1) < 0) {
        close(sockfd);
        return -1;
    }

    struct sockaddr_in bound;
    socklen_t len = sizeof(bound);
    getsockname(sockfd, (struct sockaddr *)&bound, &len);
    *outPort = ntohs(bound.sin_port);
    return sockfd;
}

static bool waitForAuthCode(int sockfd, int timeoutSeconds,
                             char *outCode, int outCodeSize) {
    struct timeval tv = {.tv_sec = timeoutSeconds, .tv_usec = 0};
    setsockopt(sockfd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));

    int clientfd = accept(sockfd, NULL, NULL);
    if (clientfd < 0) return false;

    char buf[4096];
    int n = recv(clientfd, buf, sizeof(buf) - 1, 0);
    if (n <= 0) {
        close(clientfd);
        return false;
    }
    buf[n] = '\0';

    const char *response =
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n"
        "<html><body><h2>Authentication successful</h2>"
        "<p>You can close this tab and return to sicompass.</p>"
        "</body></html>";
    send(clientfd, response, strlen(response), 0);
    close(clientfd);

    // Check for error in redirect
    char *errParam = strstr(buf, "error=");
    if (errParam && errParam < strstr(buf, "\r\n")) return false;

    char *codeParam = strstr(buf, "code=");
    if (!codeParam) return false;
    codeParam += 5;
    char *end = codeParam;
    while (*end && *end != '&' && *end != ' ' && *end != '\r' &&
           *end != '\n')
        end++;
    int len = end - codeParam;
    if (len >= outCodeSize) len = outCodeSize - 1;
    memcpy(outCode, codeParam, len);
    outCode[len] = '\0';
    return outCode[0] != '\0';
}

static OAuth2TokenResult parseTokenResponse(const char *jsonData) {
    OAuth2TokenResult result = {.success = false};

    json_object *resp = json_tokener_parse(jsonData);
    if (!resp) {
        snprintf(result.error, sizeof(result.error), "invalid JSON response");
        return result;
    }

    json_object *errObj;
    if (json_object_object_get_ex(resp, "error", &errObj)) {
        json_object *descObj;
        const char *desc = "";
        if (json_object_object_get_ex(resp, "error_description", &descObj))
            desc = json_object_get_string(descObj);
        snprintf(result.error, sizeof(result.error), "%s: %s",
                 json_object_get_string(errObj), desc ? desc : "");
        json_object_put(resp);
        return result;
    }

    json_object *val;
    if (json_object_object_get_ex(resp, "access_token", &val)) {
        const char *s = json_object_get_string(val);
        if (s)
            strncpy(result.accessToken, s, sizeof(result.accessToken) - 1);
    }
    if (json_object_object_get_ex(resp, "refresh_token", &val)) {
        const char *s = json_object_get_string(val);
        if (s)
            strncpy(result.refreshToken, s, sizeof(result.refreshToken) - 1);
    }
    if (json_object_object_get_ex(resp, "expires_in", &val))
        result.expiresIn = json_object_get_int(val);

    result.success = (result.accessToken[0] != '\0');
    if (!result.success)
        snprintf(result.error, sizeof(result.error),
                 "no access_token in response");

    json_object_put(resp);
    return result;
}

static OAuth2TokenResult exchangeCodeForTokens(const char *code,
                                                 const char *clientId,
                                                 const char *clientSecret,
                                                 const char *redirectUri) {
    OAuth2TokenResult result = {.success = false};

    CURL *curl = curl_easy_init();
    if (!curl) {
        snprintf(result.error, sizeof(result.error), "failed to init curl");
        return result;
    }

    char *encCode = curl_easy_escape(curl, code, 0);
    char *encRedirect = curl_easy_escape(curl, redirectUri, 0);
    char *encId = curl_easy_escape(curl, clientId, 0);
    char *encSecret = curl_easy_escape(curl, clientSecret, 0);

    char postFields[4096];
    snprintf(postFields, sizeof(postFields),
             "code=%s&client_id=%s&client_secret=%s"
             "&redirect_uri=%s&grant_type=authorization_code",
             encCode, encId, encSecret, encRedirect);

    curl_free(encCode);
    curl_free(encRedirect);
    curl_free(encId);
    curl_free(encSecret);

    ResponseBuffer buf = {NULL, 0, 0};
    curl_easy_setopt(curl, CURLOPT_URL, GOOGLE_TOKEN_URL);
    curl_easy_setopt(curl, CURLOPT_POST, 1L);
    curl_easy_setopt(curl, CURLOPT_POSTFIELDS, postFields);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curlWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

    CURLcode res = curl_easy_perform(curl);
    curl_easy_cleanup(curl);

    if (res != CURLE_OK || !buf.data) {
        snprintf(result.error, sizeof(result.error),
                 "token exchange failed: %s", curl_easy_strerror(res));
        free(buf.data);
        return result;
    }

    result = parseTokenResponse(buf.data);
    free(buf.data);
    return result;
}

OAuth2TokenResult emailclientOAuth2Authorize(const char *clientId,
                                              const char *clientSecret,
                                              int timeoutSeconds) {
    OAuth2TokenResult result = {.success = false};
    if (!clientId || !clientId[0] || !clientSecret || !clientSecret[0]) {
        snprintf(result.error, sizeof(result.error),
                 "client ID and client secret are required");
        return result;
    }

    int port = 0;
    int sockfd = startLocalServer(&port);
    if (sockfd < 0) {
        snprintf(result.error, sizeof(result.error),
                 "failed to start local server");
        return result;
    }

    char redirectUri[64];
    snprintf(redirectUri, sizeof(redirectUri), "http://localhost:%d", port);

    char authUrl[2048];
    snprintf(authUrl, sizeof(authUrl),
             "%s?client_id=%s&redirect_uri=%s&response_type=code"
             "&scope=%s&access_type=offline&prompt=consent",
             GOOGLE_AUTH_URL, clientId, redirectUri, OAUTH2_SCOPE);
    platformOpenWithDefault(authUrl);

    char code[512] = "";
    bool gotCode = waitForAuthCode(sockfd, timeoutSeconds, code, sizeof(code));
    close(sockfd);

    if (!gotCode) {
        snprintf(result.error, sizeof(result.error),
                 "timed out waiting for authorization");
        return result;
    }

    return exchangeCodeForTokens(code, clientId, clientSecret, redirectUri);
}

OAuth2TokenResult emailclientOAuth2RefreshToken(const char *clientId,
                                                 const char *clientSecret,
                                                 const char *refreshToken) {
    OAuth2TokenResult result = {.success = false};
    if (!clientId || !clientId[0] || !clientSecret || !clientSecret[0] ||
        !refreshToken || !refreshToken[0]) {
        snprintf(result.error, sizeof(result.error),
                 "client ID, client secret, and refresh token are required");
        return result;
    }

    CURL *curl = curl_easy_init();
    if (!curl) {
        snprintf(result.error, sizeof(result.error), "failed to init curl");
        return result;
    }

    char *encId = curl_easy_escape(curl, clientId, 0);
    char *encSecret = curl_easy_escape(curl, clientSecret, 0);
    char *encRefresh = curl_easy_escape(curl, refreshToken, 0);

    char postFields[4096];
    snprintf(postFields, sizeof(postFields),
             "client_id=%s&client_secret=%s&refresh_token=%s"
             "&grant_type=refresh_token",
             encId, encSecret, encRefresh);

    curl_free(encId);
    curl_free(encSecret);
    curl_free(encRefresh);

    ResponseBuffer buf = {NULL, 0, 0};
    curl_easy_setopt(curl, CURLOPT_URL, GOOGLE_TOKEN_URL);
    curl_easy_setopt(curl, CURLOPT_POST, 1L);
    curl_easy_setopt(curl, CURLOPT_POSTFIELDS, postFields);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curlWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

    CURLcode res = curl_easy_perform(curl);
    curl_easy_cleanup(curl);

    if (res != CURLE_OK || !buf.data) {
        snprintf(result.error, sizeof(result.error),
                 "token refresh failed: %s", curl_easy_strerror(res));
        free(buf.data);
        return result;
    }

    result = parseTokenResponse(buf.data);
    free(buf.data);
    return result;
}

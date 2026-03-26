#include <win_compat.h>
#include "emailclient_idle.h"
#include "emailclient_oauth2.h"
#include <curl/curl.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <time.h>

#ifdef _MSC_VER
#include <windows.h>
#else
#include <pthread.h>
#include <unistd.h>
#endif

/* IDLE re-enters every 25 minutes (servers timeout at ~29 min) */
#define IDLE_TIMEOUT_SEC  (25 * 60)
#define RECV_POLL_MS      200
#define RECONNECT_DELAY_SEC 10

typedef struct {
    EmailClientConfig config;
    char folder[256];
    EmailIdleNotifyFn notifyFn;
    void *userdata;
    volatile int running;
#ifdef _MSC_VER
    HANDLE thread;
#else
    pthread_t thread;
#endif
} IdleContext;

static IdleContext g_idle;
static volatile int g_idleActive = 0;

/* ---- base64 encoder for XOAUTH2 ---- */

static const char b64_table[] =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

static int base64Encode(const unsigned char *in, int inLen, char *out, int outSize) {
    int i = 0, j = 0;
    while (i < inLen && j < outSize - 4) {
        unsigned int a = (unsigned int)in[i++];
        unsigned int b = (i < inLen) ? (unsigned int)in[i++] : 0;
        unsigned int c = (i < inLen) ? (unsigned int)in[i++] : 0;
        unsigned int triple = (a << 16) | (b << 8) | c;
        out[j++] = b64_table[(triple >> 18) & 0x3F];
        out[j++] = b64_table[(triple >> 12) & 0x3F];
        out[j++] = (i > inLen + 1) ? '=' : b64_table[(triple >> 6) & 0x3F];
        out[j++] = (i > inLen)     ? '=' : b64_table[triple & 0x3F];
    }
    out[j] = '\0';
    return j;
}

/* ---- raw IMAP I/O over curl CONNECT_ONLY ---- */

static bool imapSend(CURL *curl, const char *data, size_t len) {
    size_t sent = 0;
    while (sent < len) {
        size_t n = 0;
        CURLcode res = curl_easy_send(curl, data + sent, len - sent, &n);
        if (res == CURLE_AGAIN) {
#ifdef _MSC_VER
            Sleep(10);
#else
            usleep(10000);
#endif
            continue;
        }
        if (res != CURLE_OK) return false;
        sent += n;
    }
    return true;
}

static bool imapSendStr(CURL *curl, const char *cmd) {
    return imapSend(curl, cmd, strlen(cmd));
}

/* Read a line (up to \n) with a timeout in milliseconds.
 * Returns bytes read, 0 on timeout, -1 on error. */
static int imapRecvLine(CURL *curl, char *buf, int bufSize, int timeoutMs,
                        volatile int *running) {
    int pos = 0;
    int elapsed = 0;
    while (pos < bufSize - 1 && elapsed < timeoutMs) {
        if (running && !*running) return -1;
        size_t nread = 0;
        CURLcode res = curl_easy_recv(curl, buf + pos, 1, &nread);
        if (res == CURLE_AGAIN) {
#ifdef _MSC_VER
            Sleep(RECV_POLL_MS);
#else
            usleep(RECV_POLL_MS * 1000);
#endif
            elapsed += RECV_POLL_MS;
            continue;
        }
        if (res != CURLE_OK || nread == 0) return -1;
        pos += (int)nread;
        elapsed = 0;
        if (buf[pos - 1] == '\n') break;
    }
    buf[pos] = '\0';
    if (pos == 0 && elapsed >= timeoutMs) return 0; /* timeout */
    return pos;
}

/* Wait for a tagged response line "tagNNN OK ..." or "tagNNN NO/BAD ...".
 * Returns true if OK. */
static bool imapWaitTagged(CURL *curl, const char *tag, int timeoutMs,
                           volatile int *running) {
    char line[4096];
    int tagLen = (int)strlen(tag);
    while (1) {
        int n = imapRecvLine(curl, line, sizeof(line), timeoutMs, running);
        if (n <= 0) return false;
        if (strncmp(line, tag, tagLen) == 0 && line[tagLen] == ' ') {
            return strstr(line + tagLen, "OK") != NULL;
        }
    }
}

/* Build XOAUTH2 SASL token: base64("user=<user>\x01auth=Bearer <token>\x01\x01") */
static bool buildXOAuth2Token(const EmailClientConfig *config,
                              char *out, int outSize) {
    char raw[4096];
    int rawLen = snprintf(raw, sizeof(raw), "user=%s\x01auth=Bearer %s\x01\x01",
                          config->username, config->oauthAccessToken);
    if (rawLen <= 0 || rawLen >= (int)sizeof(raw)) return false;
    return base64Encode((unsigned char *)raw, rawLen, out, outSize) > 0;
}

/* Refresh token if needed. Returns false if refresh fails. */
static bool idleRefreshToken(EmailClientConfig *config) {
    if (!config->oauthAccessToken[0]) return true;
    if (time(NULL) < config->tokenExpiry - 60) return true;
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

/* ---- IDLE thread ---- */

static void *idleThreadFunc(void *arg) {
    IdleContext *ctx = (IdleContext *)arg;

    while (ctx->running) {
        /* Refresh OAuth2 token before connecting */
        if (!idleRefreshToken(&ctx->config)) {
            fprintf(stderr, "emailclient_idle: token refresh failed, retrying in %ds\n",
                    RECONNECT_DELAY_SEC);
            goto delay_and_retry;
        }

        /* Establish TLS connection via curl CONNECT_ONLY */
        CURL *curl = curl_easy_init();
        if (!curl) goto delay_and_retry;

        char url[1024];
        snprintf(url, sizeof(url), "%s/", ctx->config.imapUrl);
        curl_easy_setopt(curl, CURLOPT_URL, url);
        curl_easy_setopt(curl, CURLOPT_CONNECT_ONLY, 1L);
        curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

        CURLcode res = curl_easy_perform(curl);
        if (res != CURLE_OK) {
            fprintf(stderr, "emailclient_idle: connect failed: %s\n",
                    curl_easy_strerror(res));
            curl_easy_cleanup(curl);
            goto delay_and_retry;
        }

        /* Read server greeting */
        char line[4096];
        if (imapRecvLine(curl, line, sizeof(line), 10000, &ctx->running) <= 0) {
            curl_easy_cleanup(curl);
            goto delay_and_retry;
        }

        /* Authenticate */
        if (ctx->config.oauthAccessToken[0]) {
            char token[4096];
            if (!buildXOAuth2Token(&ctx->config, token, sizeof(token))) {
                curl_easy_cleanup(curl);
                goto delay_and_retry;
            }
            char cmd[8192];
            snprintf(cmd, sizeof(cmd), "A001 AUTHENTICATE XOAUTH2 %s\r\n", token);
            if (!imapSendStr(curl, cmd)) { curl_easy_cleanup(curl); goto delay_and_retry; }
        } else {
            char cmd[1024];
            snprintf(cmd, sizeof(cmd), "A001 LOGIN \"%s\" \"%s\"\r\n",
                     ctx->config.username, ctx->config.password);
            if (!imapSendStr(curl, cmd)) { curl_easy_cleanup(curl); goto delay_and_retry; }
        }
        if (!imapWaitTagged(curl, "A001", 10000, &ctx->running)) {
            curl_easy_cleanup(curl);
            goto delay_and_retry;
        }

        /* SELECT folder */
        char selectCmd[512];
        snprintf(selectCmd, sizeof(selectCmd), "A002 SELECT \"%s\"\r\n", ctx->folder);
        if (!imapSendStr(curl, selectCmd)) { curl_easy_cleanup(curl); goto delay_and_retry; }
        if (!imapWaitTagged(curl, "A002", 10000, &ctx->running)) {
            curl_easy_cleanup(curl);
            goto delay_and_retry;
        }

        /* IDLE loop — re-enter IDLE every IDLE_TIMEOUT_SEC */
        while (ctx->running) {
            if (!imapSendStr(curl, "A003 IDLE\r\n")) break;

            /* Wait for + continuation */
            int n = imapRecvLine(curl, line, sizeof(line), 10000, &ctx->running);
            if (n <= 0 || line[0] != '+') break;

            /* Wait for server notifications or timeout */
            bool gotNotification = false;
            time_t idleStart = time(NULL);
            while (ctx->running) {
                int elapsed = (int)(time(NULL) - idleStart);
                if (elapsed >= IDLE_TIMEOUT_SEC) break; /* re-IDLE */

                int remaining = (IDLE_TIMEOUT_SEC - elapsed) * 1000;
                if (remaining < 1000) remaining = 1000;
                n = imapRecvLine(curl, line, sizeof(line), remaining, &ctx->running);
                if (n < 0) goto end_connection; /* error */
                if (n == 0) break; /* timeout — re-IDLE */

                if (strstr(line, "EXISTS") || strstr(line, "EXPUNGE")) {
                    gotNotification = true;
                    break;
                }
            }

            /* Send DONE to end IDLE */
            imapSendStr(curl, "DONE\r\n");
            imapWaitTagged(curl, "A003", 5000, &ctx->running);

            if (gotNotification && ctx->running) {
                ctx->notifyFn(ctx->userdata);
            }

            /* Refresh token before next IDLE iteration if needed */
            if (ctx->running && !idleRefreshToken(&ctx->config)) break;
        }

    end_connection:
        imapSendStr(curl, "A999 LOGOUT\r\n");
        curl_easy_cleanup(curl);
        continue;

    delay_and_retry:
        for (int i = 0; i < RECONNECT_DELAY_SEC && ctx->running; i++) {
#ifdef _MSC_VER
            Sleep(1000);
#else
            sleep(1);
#endif
        }
    }
    return NULL;
}

#ifdef _MSC_VER
static DWORD WINAPI idleThreadFuncWin(LPVOID arg) {
    idleThreadFunc(arg);
    return 0;
}
#endif

bool emailclientIdleStart(const EmailClientConfig *config, const char *folder,
                          EmailIdleNotifyFn notifyFn, void *userdata) {
    if (g_idleActive) emailclientIdleStop();

    memcpy(&g_idle.config, config, sizeof(EmailClientConfig));
    strncpy(g_idle.folder, folder, sizeof(g_idle.folder) - 1);
    g_idle.folder[sizeof(g_idle.folder) - 1] = '\0';
    g_idle.notifyFn = notifyFn;
    g_idle.userdata = userdata;
    g_idle.running = 1;

#ifdef _MSC_VER
    g_idle.thread = CreateThread(NULL, 0, idleThreadFuncWin, &g_idle, 0, NULL);
    if (!g_idle.thread) { g_idle.running = 0; return false; }
#else
    if (pthread_create(&g_idle.thread, NULL, idleThreadFunc, &g_idle) != 0) {
        g_idle.running = 0;
        return false;
    }
#endif

    g_idleActive = 1;
    return true;
}

void emailclientIdleStop(void) {
    if (!g_idleActive) return;
    g_idle.running = 0;

#ifdef _MSC_VER
    WaitForSingleObject(g_idle.thread, 15000);
    CloseHandle(g_idle.thread);
#else
    pthread_join(g_idle.thread, NULL);
#endif

    g_idleActive = 0;
}

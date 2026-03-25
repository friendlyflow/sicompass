#pragma once

#include <stdbool.h>

typedef struct {
    char imapUrl[512];      // e.g. "imaps://imap.gmail.com"
    char smtpUrl[512];      // e.g. "smtps://smtp.gmail.com"
    char username[256];
    char password[256];
    char clientId[256];
    char clientSecret[256];
    char oauthAccessToken[2048];
    char oauthRefreshToken[2048];
    long tokenExpiry;       // Unix timestamp
} EmailClientConfig;

typedef struct {
    char name[256];         // e.g. "INBOX", "Sent"
} EmailFolder;

typedef struct {
    int uid;
    char from[256];
    char subject[512];
    char date[64];
} EmailHeader;

typedef struct {
    int uid;
    char from[256];
    char to[256];
    char subject[512];
    char date[64];
    char messageId[256];
    char inReplyTo[256];
    char references[2048];
    char body[8192];
} EmailMessage;

/**
 * Global libcurl initialization. Call once at startup.
 */
void emailclientGlobalInit(void);

/**
 * Global libcurl cleanup. Call once at shutdown.
 */
void emailclientGlobalCleanup(void);

/**
 * List IMAP folders.
 * Returns heap-allocated array; caller frees with emailclientFreeFolders().
 */
EmailFolder* emailclientListFolders(EmailClientConfig *config, int *outCount);
void emailclientFreeFolders(EmailFolder *folders, int count);

/**
 * List message headers in a folder.
 * Returns heap-allocated array; caller frees with emailclientFreeHeaders().
 */
EmailHeader* emailclientListMessages(EmailClientConfig *config,
                                      const char *folder, int limit,
                                      int *outCount);
void emailclientFreeHeaders(EmailHeader *headers, int count);

/**
 * Fetch a single message by UID from a folder.
 * Returns heap-allocated message; caller frees with emailclientFreeMessage().
 */
EmailMessage* emailclientFetchMessage(EmailClientConfig *config,
                                       const char *folder, int uid);
void emailclientFreeMessage(EmailMessage *msg);

/**
 * Fetch a message by its Message-ID header from a folder.
 * Uses IMAP SEARCH to find the UID, then fetches the full message.
 * Returns NULL if not found.
 */
EmailMessage* emailclientFetchMessageByMessageId(
    EmailClientConfig *config, const char *folder, const char *messageId);

/**
 * Send a plain-text email via SMTP.
 */
bool emailclientSendMessage(EmailClientConfig *config,
                             const char *to, const char *subject,
                             const char *body);

#pragma once

#include <stdbool.h>

typedef struct {
    char imapUrl[512];      // e.g. "imaps://imap.example.com"
    char smtpUrl[512];      // e.g. "smtps://smtp.example.com"
    char username[256];
    char password[256];
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
    char subject[512];
    char date[64];
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
EmailFolder* emailclientListFolders(const EmailClientConfig *config, int *outCount);
void emailclientFreeFolders(EmailFolder *folders, int count);

/**
 * List message headers in a folder.
 * Returns heap-allocated array; caller frees with emailclientFreeHeaders().
 */
EmailHeader* emailclientListMessages(const EmailClientConfig *config,
                                      const char *folder, int limit,
                                      int *outCount);
void emailclientFreeHeaders(EmailHeader *headers, int count);

/**
 * Fetch a single message by UID from a folder.
 * Returns heap-allocated message; caller frees with emailclientFreeMessage().
 */
EmailMessage* emailclientFetchMessage(const EmailClientConfig *config,
                                       const char *folder, int uid);
void emailclientFreeMessage(EmailMessage *msg);

/**
 * Send a plain-text email via SMTP.
 */
bool emailclientSendMessage(const EmailClientConfig *config,
                             const char *to, const char *subject,
                             const char *body);

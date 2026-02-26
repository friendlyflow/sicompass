#pragma once

#include <stdbool.h>

typedef struct {
    char homeserverUrl[512];
    char accessToken[512];
    char username[256];
    char password[256];
} ChatClientConfig;

typedef struct {
    char roomId[256];
    char displayName[256];
} ChatRoom;

typedef struct {
    char sender[256];
    char body[4096];
    char eventId[256];
} ChatMessage;

/**
 * Global libcurl initialization. Call once at startup.
 */
void chatclientGlobalInit(void);

/**
 * Global libcurl cleanup. Call once at shutdown.
 */
void chatclientGlobalCleanup(void);

/**
 * Fetch the list of joined rooms from the homeserver.
 * Returns heap-allocated array; caller frees with chatclientFreeRooms().
 */
ChatRoom* chatclientGetJoinedRooms(const ChatClientConfig *config, int *outCount);
void chatclientFreeRooms(ChatRoom *rooms, int count);

/**
 * Fetch recent messages from a room (newest last).
 * Returns heap-allocated array; caller frees with chatclientFreeMessages().
 */
ChatMessage* chatclientGetRoomMessages(const ChatClientConfig *config,
                                        const char *roomId, int limit,
                                        int *outCount);
void chatclientFreeMessages(ChatMessage *messages, int count);

/**
 * Send a text message to a room.
 */
bool chatclientSendMessage(const ChatClientConfig *config,
                            const char *roomId, const char *body);

/**
 * Resolve the display name for a room.
 * Tries m.room.name state event first, falls back to room ID.
 */
void chatclientResolveRoomName(const ChatClientConfig *config,
                                const char *roomId,
                                char *outName, int outNameSize);

/**
 * Result of a login or register operation.
 */
typedef struct {
    bool success;
    bool requiresAuth;
    char accessToken[512];
    char userId[256];
    char deviceId[256];
    char session[256];
    char nextStage[256];
    char error[512];
} ChatAuthResult;

/**
 * Log in to a Matrix homeserver with username/password.
 * Uses POST /_matrix/client/v3/login with m.login.password.
 */
ChatAuthResult chatclientLogin(const char *homeserverUrl,
                               const char *username, const char *password);

/**
 * Register a new account on a Matrix homeserver.
 * Uses POST /_matrix/client/v3/register with m.login.dummy auth.
 * Only works on servers with open registration (not matrix.org).
 */
ChatAuthResult chatclientRegister(const char *homeserverUrl,
                                  const char *username, const char *password);

/**
 * Complete a registration that requires interactive auth (CAPTCHA, etc).
 * Retries with the given UIA session after the user completed the fallback page.
 */
ChatAuthResult chatclientRegisterComplete(const char *homeserverUrl,
                                          const char *session,
                                          const char *username,
                                          const char *password);

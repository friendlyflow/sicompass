#pragma once

#include <stdbool.h>

typedef struct {
    char homeserverUrl[512];
    char accessToken[512];
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

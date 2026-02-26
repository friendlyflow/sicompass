#include <chatclient.h>
#include <chatclient_provider.h>
#include <provider_tags.h>
#include <ffon.h>
#include <json-c/json.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

// Shared config loaded from settings
static ChatClientConfig g_config = { .homeserverUrl = "", .accessToken = "" };

// Room display name to room ID mapping
#define MAX_ROOMS 256

typedef struct {
    char displayName[256];
    char roomId[256];
} RoomMapping;

static RoomMapping g_roomMappings[MAX_ROOMS];
static int g_roomMappingCount = 0;

static void ccStoreRoomMappings(const ChatRoom *rooms, int count) {
    g_roomMappingCount = 0;
    for (int i = 0; i < count && i < MAX_ROOMS; i++) {
        strncpy(g_roomMappings[i].displayName, rooms[i].displayName,
                sizeof(g_roomMappings[i].displayName) - 1);
        g_roomMappings[i].displayName[sizeof(g_roomMappings[i].displayName) - 1] = '\0';
        strncpy(g_roomMappings[i].roomId, rooms[i].roomId,
                sizeof(g_roomMappings[i].roomId) - 1);
        g_roomMappings[i].roomId[sizeof(g_roomMappings[i].roomId) - 1] = '\0';
        g_roomMappingCount++;
    }
}

static const char* ccLookupRoomId(const char *displayName) {
    for (int i = 0; i < g_roomMappingCount; i++) {
        if (strcmp(g_roomMappings[i].displayName, displayName) == 0)
            return g_roomMappings[i].roomId;
    }
    return NULL;
}

static FfonElement** ccFetch(const char *path, int *outCount) {
    if (!g_config.homeserverUrl[0] || !g_config.accessToken[0]) {
        *outCount = 1;
        FfonElement **elems = malloc(sizeof(FfonElement*));
        elems[0] = ffonElementCreateString(
            "configure homeserver URL and access token in settings");
        return elems;
    }

    if (strcmp(path, "/") == 0) {
        int roomCount = 0;
        ChatRoom *rooms = chatclientGetJoinedRooms(&g_config, &roomCount);
        if (!rooms || roomCount == 0) {
            chatclientFreeRooms(rooms, roomCount);
            *outCount = 1;
            FfonElement **elems = malloc(sizeof(FfonElement*));
            elems[0] = ffonElementCreateString("no rooms found");
            return elems;
        }

        FfonElement **elems = malloc(roomCount * sizeof(FfonElement*));
        for (int i = 0; i < roomCount; i++) {
            elems[i] = ffonElementCreateObject(rooms[i].displayName);
        }
        ccStoreRoomMappings(rooms, roomCount);
        chatclientFreeRooms(rooms, roomCount);
        *outCount = roomCount;
        return elems;
    }

    // Inside a room: path = "/{displayName}"
    const char *segment = path + 1;
    const char *roomId = ccLookupRoomId(segment);
    if (!roomId) {
        *outCount = 1;
        FfonElement **elems = malloc(sizeof(FfonElement*));
        elems[0] = ffonElementCreateString("room not found");
        return elems;
    }

    int msgCount = 0;
    ChatMessage *msgs = chatclientGetRoomMessages(&g_config, roomId, 50, &msgCount);
    if (!msgs || msgCount == 0) {
        chatclientFreeMessages(msgs, msgCount);
        *outCount = 1;
        FfonElement **elems = malloc(sizeof(FfonElement*));
        elems[0] = ffonElementCreateString("<input></input>");
        return elems;
    }

    FfonElement **elems = malloc((msgCount + 1) * sizeof(FfonElement*));
    for (int i = 0; i < msgCount; i++) {
        char buf[4096 + 256 + 4];
        snprintf(buf, sizeof(buf), "%s: %s", msgs[i].sender, msgs[i].body);
        elems[i] = ffonElementCreateString(buf);
    }
    elems[msgCount] = ffonElementCreateString("<input></input>");
    chatclientFreeMessages(msgs, msgCount);
    *outCount = msgCount + 1;
    return elems;
}

static bool ccCommit(const char *path, const char *oldName, const char *newName) {
    (void)oldName;
    if (!newName || !newName[0]) return false;
    if (strcmp(path, "/") == 0) return false;

    const char *segment = path + 1;
    const char *roomId = ccLookupRoomId(segment);
    if (!roomId) return false;

    return chatclientSendMessage(&g_config, roomId, newName);
}

static const char *cc_commands[] = {
    "send message",
    "refresh"
};

static const char** ccGetCommands(int *outCount) {
    *outCount = 2;
    return cc_commands;
}

static FfonElement* ccHandleCommand(const char *path, const char *command,
                                     const char *elementKey, int elementType,
                                     char *errorMsg, int errorMsgSize) {
    (void)path;
    (void)elementType;

    if (strcmp(command, "send message") == 0) {
        return ffonElementCreateString("<input></input>");
    }
    if (strcmp(command, "refresh") == 0) {
        return NULL;
    }
    // Internal commands for settings dispatch
    if (strcmp(command, "set homeserver") == 0) {
        if (elementKey) {
            strncpy(g_config.homeserverUrl, elementKey, sizeof(g_config.homeserverUrl) - 1);
            g_config.homeserverUrl[sizeof(g_config.homeserverUrl) - 1] = '\0';
        }
        return NULL;
    }
    if (strcmp(command, "set access token") == 0) {
        if (elementKey) {
            strncpy(g_config.accessToken, elementKey, sizeof(g_config.accessToken) - 1);
            g_config.accessToken[sizeof(g_config.accessToken) - 1] = '\0';
        }
        return NULL;
    }

    if (errorMsg && errorMsgSize > 0)
        snprintf(errorMsg, errorMsgSize, "unknown command: %s", command);
    return NULL;
}

static bool ccExecuteCommand(const char *path, const char *command,
                              const char *selection) {
    (void)path;
    (void)command;
    (void)selection;
    return true;
}

// Provider singleton
static Provider *g_provider = NULL;
static void (*g_originalInit)(struct Provider *self) = NULL;

static void ccInit(struct Provider *self) {
    if (g_originalInit) g_originalInit(self);
    chatclientGlobalInit();

    // Load config from settings.json
    char *configPath = providerGetMainConfigPath();
    if (configPath) {
        json_object *root = json_object_from_file(configPath);
        if (root) {
            json_object *section;
            if (json_object_object_get_ex(root, "chat client", &section)) {
                json_object *val;
                if (json_object_object_get_ex(section, "chatHomeserver", &val)) {
                    const char *s = json_object_get_string(val);
                    if (s) {
                        strncpy(g_config.homeserverUrl, s, sizeof(g_config.homeserverUrl) - 1);
                        g_config.homeserverUrl[sizeof(g_config.homeserverUrl) - 1] = '\0';
                    }
                }
                if (json_object_object_get_ex(section, "chatAccessToken", &val)) {
                    const char *s = json_object_get_string(val);
                    if (s) {
                        strncpy(g_config.accessToken, s, sizeof(g_config.accessToken) - 1);
                        g_config.accessToken[sizeof(g_config.accessToken) - 1] = '\0';
                    }
                }
            }
            json_object_put(root);
        }
        free(configPath);
    }
}

static void ccCleanup(struct Provider *self) {
    (void)self;
    chatclientGlobalCleanup();
}

Provider* chatclientGetProvider(void) {
    if (!g_provider) {
        static ProviderOps ops = {
            .name = "chatclient",
            .displayName = "chat client",
            .fetch = ccFetch,
            .commit = ccCommit,
            .createDirectory = NULL,
            .createFile = NULL,
            .deleteItem = NULL,
            .copyItem = NULL,
            .getCommands = ccGetCommands,
            .handleCommand = ccHandleCommand,
            .getCommandListItems = NULL,
            .executeCommand = ccExecuteCommand,
            .collectDeepSearchItems = NULL,
        };
        g_provider = providerCreate(&ops);
        g_originalInit = g_provider->init;
        g_provider->init = ccInit;
        g_provider->cleanup = ccCleanup;
    }
    return g_provider;
}

__attribute__((constructor))
static void chatclientRegisterFactory(void) {
    providerFactoryRegister("chat client", chatclientGetProvider);
}

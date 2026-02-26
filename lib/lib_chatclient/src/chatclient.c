#include "chatclient.h"
#include <curl/curl.h>
#include <json-c/json.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <stdint.h>

typedef struct {
    char *data;
    size_t size;
    size_t capacity;
} ResponseBuffer;

static size_t curlWriteCallback(char *ptr, size_t size, size_t nmemb, void *userdata) {
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

static json_object* matrixApiGet(const ChatClientConfig *config, const char *endpoint) {
    if (!config || !config->homeserverUrl[0] || !config->accessToken[0]) return NULL;

    char url[2048];
    snprintf(url, sizeof(url), "%s%s", config->homeserverUrl, endpoint);

    char authHeader[600];
    snprintf(authHeader, sizeof(authHeader), "Authorization: Bearer %s", config->accessToken);

    CURL *curl = curl_easy_init();
    if (!curl) return NULL;

    ResponseBuffer buf = {NULL, 0, 0};
    struct curl_slist *headers = NULL;
    headers = curl_slist_append(headers, authHeader);

    curl_easy_setopt(curl, CURLOPT_URL, url);
    curl_easy_setopt(curl, CURLOPT_HTTPHEADER, headers);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curlWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

    CURLcode res = curl_easy_perform(curl);
    curl_slist_free_all(headers);
    curl_easy_cleanup(curl);

    if (res != CURLE_OK || !buf.data) {
        free(buf.data);
        return NULL;
    }

    json_object *obj = json_tokener_parse(buf.data);
    free(buf.data);
    return obj;
}

static json_object* matrixApiPut(const ChatClientConfig *config, const char *endpoint,
                                  json_object *body) {
    if (!config || !config->homeserverUrl[0] || !config->accessToken[0]) return NULL;

    char url[2048];
    snprintf(url, sizeof(url), "%s%s", config->homeserverUrl, endpoint);

    char authHeader[600];
    snprintf(authHeader, sizeof(authHeader), "Authorization: Bearer %s", config->accessToken);

    const char *bodyStr = json_object_to_json_string(body);

    CURL *curl = curl_easy_init();
    if (!curl) return NULL;

    ResponseBuffer buf = {NULL, 0, 0};
    struct curl_slist *headers = NULL;
    headers = curl_slist_append(headers, authHeader);
    headers = curl_slist_append(headers, "Content-Type: application/json");

    curl_easy_setopt(curl, CURLOPT_URL, url);
    curl_easy_setopt(curl, CURLOPT_CUSTOMREQUEST, "PUT");
    curl_easy_setopt(curl, CURLOPT_HTTPHEADER, headers);
    curl_easy_setopt(curl, CURLOPT_POSTFIELDS, bodyStr);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curlWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

    CURLcode res = curl_easy_perform(curl);
    curl_slist_free_all(headers);
    curl_easy_cleanup(curl);

    if (res != CURLE_OK || !buf.data) {
        free(buf.data);
        return NULL;
    }

    json_object *obj = json_tokener_parse(buf.data);
    free(buf.data);
    return obj;
}

static json_object* matrixApiPost(const char *homeserverUrl, const char *endpoint,
                                   json_object *body) {
    if (!homeserverUrl || !homeserverUrl[0]) return NULL;

    char url[2048];
    snprintf(url, sizeof(url), "%s%s", homeserverUrl, endpoint);

    const char *bodyStr = json_object_to_json_string(body);

    CURL *curl = curl_easy_init();
    if (!curl) return NULL;

    ResponseBuffer buf = {NULL, 0, 0};
    struct curl_slist *headers = NULL;
    headers = curl_slist_append(headers, "Content-Type: application/json");

    curl_easy_setopt(curl, CURLOPT_URL, url);
    curl_easy_setopt(curl, CURLOPT_POST, 1L);
    curl_easy_setopt(curl, CURLOPT_HTTPHEADER, headers);
    curl_easy_setopt(curl, CURLOPT_POSTFIELDS, bodyStr);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, curlWriteCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 30L);

    CURLcode res = curl_easy_perform(curl);
    curl_slist_free_all(headers);
    curl_easy_cleanup(curl);

    if (res != CURLE_OK || !buf.data) {
        free(buf.data);
        return NULL;
    }

    json_object *obj = json_tokener_parse(buf.data);
    free(buf.data);
    return obj;
}

static ChatAuthResult parseAuthResponse(json_object *resp) {
    ChatAuthResult result = { .success = false, .requiresAuth = false };

    if (!resp) {
        snprintf(result.error, sizeof(result.error), "failed to connect to homeserver");
        return result;
    }

    // Check for UIA (User-Interactive Authentication) response
    json_object *sessionObj, *flowsObj;
    if (json_object_object_get_ex(resp, "session", &sessionObj) &&
        json_object_object_get_ex(resp, "flows", &flowsObj)) {
        const char *session = json_object_get_string(sessionObj);
        if (session) {
            result.requiresAuth = true;
            strncpy(result.session, session, sizeof(result.session) - 1);
            // Extract first stage from first flow
            if (json_object_is_type(flowsObj, json_type_array) &&
                json_object_array_length(flowsObj) > 0) {
                json_object *flow = json_object_array_get_idx(flowsObj, 0);
                json_object *stagesObj;
                if (json_object_object_get_ex(flow, "stages", &stagesObj) &&
                    json_object_is_type(stagesObj, json_type_array)) {
                    // Find first incomplete stage (skip completed ones)
                    json_object *completedObj;
                    int completedCount = 0;
                    if (json_object_object_get_ex(resp, "completed", &completedObj) &&
                        json_object_is_type(completedObj, json_type_array))
                        completedCount = json_object_array_length(completedObj);
                    int stageCount = json_object_array_length(stagesObj);
                    if (completedCount < stageCount) {
                        json_object *stage = json_object_array_get_idx(
                            stagesObj, completedCount);
                        const char *s = json_object_get_string(stage);
                        if (s)
                            strncpy(result.nextStage, s,
                                    sizeof(result.nextStage) - 1);
                    }
                }
            }
            snprintf(result.error, sizeof(result.error),
                     "interactive auth required: %s",
                     result.nextStage[0] ? result.nextStage : "unknown stage");
            json_object_put(resp);
            return result;
        }
    }

    json_object *errObj;
    if (json_object_object_get_ex(resp, "errcode", &errObj)) {
        json_object *errMsgObj;
        const char *errCode = json_object_get_string(errObj);
        const char *errMsg = "";
        if (json_object_object_get_ex(resp, "error", &errMsgObj))
            errMsg = json_object_get_string(errMsgObj);
        snprintf(result.error, sizeof(result.error), "%s: %s",
                 errCode ? errCode : "", errMsg ? errMsg : "");
        json_object_put(resp);
        return result;
    }

    json_object *tokenObj, *userIdObj, *deviceIdObj;
    if (json_object_object_get_ex(resp, "access_token", &tokenObj)) {
        const char *token = json_object_get_string(tokenObj);
        if (token) {
            strncpy(result.accessToken, token, sizeof(result.accessToken) - 1);
            result.success = true;
        }
    }
    if (json_object_object_get_ex(resp, "user_id", &userIdObj)) {
        const char *uid = json_object_get_string(userIdObj);
        if (uid) strncpy(result.userId, uid, sizeof(result.userId) - 1);
    }
    if (json_object_object_get_ex(resp, "device_id", &deviceIdObj)) {
        const char *did = json_object_get_string(deviceIdObj);
        if (did) strncpy(result.deviceId, did, sizeof(result.deviceId) - 1);
    }

    if (!result.success)
        snprintf(result.error, sizeof(result.error), "no access_token in response");

    json_object_put(resp);
    return result;
}

void chatclientGlobalInit(void) {
    curl_global_init(CURL_GLOBAL_DEFAULT);
}

void chatclientGlobalCleanup(void) {
    curl_global_cleanup();
}

void chatclientResolveRoomName(const ChatClientConfig *config,
                                const char *roomId,
                                char *outName, int outNameSize) {
    if (!outName || outNameSize <= 0) return;
    outName[0] = '\0';

    char endpoint[1024];
    char encodedRoomId[512];
    // URL-encode the room ID (! and : need encoding)
    const char *src = roomId;
    char *dst = encodedRoomId;
    char *end = encodedRoomId + sizeof(encodedRoomId) - 4;
    while (*src && dst < end) {
        if ((*src >= 'A' && *src <= 'Z') || (*src >= 'a' && *src <= 'z') ||
            (*src >= '0' && *src <= '9') || *src == '-' || *src == '_' || *src == '.' || *src == '~') {
            *dst++ = *src;
        } else {
            snprintf(dst, 4, "%%%02X", (unsigned char)*src);
            dst += 3;
        }
        src++;
    }
    *dst = '\0';

    snprintf(endpoint, sizeof(endpoint),
             "/_matrix/client/v3/rooms/%s/state/m.room.name", encodedRoomId);

    json_object *resp = matrixApiGet(config, endpoint);
    if (resp) {
        json_object *nameObj;
        if (json_object_object_get_ex(resp, "name", &nameObj)) {
            const char *name = json_object_get_string(nameObj);
            if (name && name[0]) {
                strncpy(outName, name, outNameSize - 1);
                outName[outNameSize - 1] = '\0';
                json_object_put(resp);
                return;
            }
        }
        json_object_put(resp);
    }

    // Fallback to room ID
    strncpy(outName, roomId, outNameSize - 1);
    outName[outNameSize - 1] = '\0';
}

// URL-encode a room ID into a buffer
static void urlEncodeRoomId(const char *roomId, char *out, int outSize) {
    const char *src = roomId;
    char *dst = out;
    char *end = out + outSize - 4;
    while (*src && dst < end) {
        if ((*src >= 'A' && *src <= 'Z') || (*src >= 'a' && *src <= 'z') ||
            (*src >= '0' && *src <= '9') || *src == '-' || *src == '_' || *src == '.' || *src == '~') {
            *dst++ = *src;
        } else {
            snprintf(dst, 4, "%%%02X", (unsigned char)*src);
            dst += 3;
        }
        src++;
    }
    *dst = '\0';
}

ChatRoom* chatclientGetJoinedRooms(const ChatClientConfig *config, int *outCount) {
    *outCount = 0;

    json_object *resp = matrixApiGet(config, "/_matrix/client/v3/joined_rooms");
    if (!resp) return NULL;

    json_object *roomsArr;
    if (!json_object_object_get_ex(resp, "joined_rooms", &roomsArr) ||
        !json_object_is_type(roomsArr, json_type_array)) {
        json_object_put(resp);
        return NULL;
    }

    int count = json_object_array_length(roomsArr);
    if (count <= 0) {
        json_object_put(resp);
        return NULL;
    }

    ChatRoom *rooms = calloc(count, sizeof(ChatRoom));
    if (!rooms) {
        json_object_put(resp);
        return NULL;
    }

    for (int i = 0; i < count; i++) {
        json_object *item = json_object_array_get_idx(roomsArr, i);
        const char *id = json_object_get_string(item);
        if (id) {
            strncpy(rooms[i].roomId, id, sizeof(rooms[i].roomId) - 1);
            chatclientResolveRoomName(config, id,
                                      rooms[i].displayName, sizeof(rooms[i].displayName));
        }
    }

    json_object_put(resp);
    *outCount = count;
    return rooms;
}

void chatclientFreeRooms(ChatRoom *rooms, int count) {
    (void)count;
    free(rooms);
}

ChatMessage* chatclientGetRoomMessages(const ChatClientConfig *config,
                                        const char *roomId, int limit,
                                        int *outCount) {
    *outCount = 0;

    char encodedRoomId[512];
    urlEncodeRoomId(roomId, encodedRoomId, sizeof(encodedRoomId));

    char endpoint[1024];
    snprintf(endpoint, sizeof(endpoint),
             "/_matrix/client/v3/rooms/%s/messages?dir=b&limit=%d",
             encodedRoomId, limit);

    json_object *resp = matrixApiGet(config, endpoint);
    if (!resp) return NULL;

    json_object *chunk;
    if (!json_object_object_get_ex(resp, "chunk", &chunk) ||
        !json_object_is_type(chunk, json_type_array)) {
        json_object_put(resp);
        return NULL;
    }

    int rawCount = json_object_array_length(chunk);
    if (rawCount <= 0) {
        json_object_put(resp);
        return NULL;
    }

    ChatMessage *msgs = calloc(rawCount, sizeof(ChatMessage));
    if (!msgs) {
        json_object_put(resp);
        return NULL;
    }

    // Parse messages (API returns newest-first with dir=b)
    int msgIdx = 0;
    for (int i = 0; i < rawCount; i++) {
        json_object *event = json_object_array_get_idx(chunk, i);
        json_object *typeObj, *contentObj, *senderObj, *eventIdObj;

        if (!json_object_object_get_ex(event, "type", &typeObj)) continue;
        const char *type = json_object_get_string(typeObj);
        if (!type || strcmp(type, "m.room.message") != 0) continue;

        if (!json_object_object_get_ex(event, "content", &contentObj)) continue;

        json_object *bodyObj;
        if (!json_object_object_get_ex(contentObj, "body", &bodyObj)) continue;
        const char *body = json_object_get_string(bodyObj);
        if (!body) continue;

        const char *sender = "";
        if (json_object_object_get_ex(event, "sender", &senderObj))
            sender = json_object_get_string(senderObj);

        const char *eventId = "";
        if (json_object_object_get_ex(event, "event_id", &eventIdObj))
            eventId = json_object_get_string(eventIdObj);

        strncpy(msgs[msgIdx].sender, sender ? sender : "", sizeof(msgs[msgIdx].sender) - 1);
        strncpy(msgs[msgIdx].body, body, sizeof(msgs[msgIdx].body) - 1);
        strncpy(msgs[msgIdx].eventId, eventId ? eventId : "", sizeof(msgs[msgIdx].eventId) - 1);
        msgIdx++;
    }

    json_object_put(resp);

    if (msgIdx == 0) {
        free(msgs);
        return NULL;
    }

    // Reverse to chronological order (API returned newest-first)
    for (int i = 0; i < msgIdx / 2; i++) {
        ChatMessage tmp = msgs[i];
        msgs[i] = msgs[msgIdx - 1 - i];
        msgs[msgIdx - 1 - i] = tmp;
    }

    *outCount = msgIdx;
    return msgs;
}

void chatclientFreeMessages(ChatMessage *messages, int count) {
    (void)count;
    free(messages);
}

bool chatclientSendMessage(const ChatClientConfig *config,
                            const char *roomId, const char *body) {
    if (!body || !body[0]) return false;

    static uint64_t g_txnId = 0;
    g_txnId++;

    char encodedRoomId[512];
    urlEncodeRoomId(roomId, encodedRoomId, sizeof(encodedRoomId));

    char endpoint[1024];
    snprintf(endpoint, sizeof(endpoint),
             "/_matrix/client/v3/rooms/%s/send/m.room.message/m%llu",
             encodedRoomId, (unsigned long long)g_txnId);

    json_object *msgBody = json_object_new_object();
    json_object_object_add(msgBody, "msgtype", json_object_new_string("m.text"));
    json_object_object_add(msgBody, "body", json_object_new_string(body));

    json_object *resp = matrixApiPut(config, endpoint, msgBody);
    json_object_put(msgBody);

    bool success = (resp != NULL);
    if (resp) {
        // Check for error
        json_object *errObj;
        if (json_object_object_get_ex(resp, "errcode", &errObj))
            success = false;
        json_object_put(resp);
    }

    return success;
}

ChatAuthResult chatclientLogin(const char *homeserverUrl,
                               const char *username, const char *password) {
    ChatAuthResult result = { .success = false };
    if (!homeserverUrl || !homeserverUrl[0] ||
        !username || !username[0] || !password || !password[0]) {
        snprintf(result.error, sizeof(result.error),
                 "homeserver, username, and password are required");
        return result;
    }

    json_object *body = json_object_new_object();
    json_object_object_add(body, "type",
                           json_object_new_string("m.login.password"));
    json_object *identifier = json_object_new_object();
    json_object_object_add(identifier, "type",
                           json_object_new_string("m.id.user"));
    json_object_object_add(identifier, "user",
                           json_object_new_string(username));
    json_object_object_add(body, "identifier", identifier);
    json_object_object_add(body, "password",
                           json_object_new_string(password));

    json_object *resp = matrixApiPost(homeserverUrl,
                                      "/_matrix/client/v3/login", body);
    json_object_put(body);
    return parseAuthResponse(resp);
}

ChatAuthResult chatclientRegister(const char *homeserverUrl,
                                  const char *username, const char *password) {
    ChatAuthResult result = { .success = false };
    if (!homeserverUrl || !homeserverUrl[0] ||
        !username || !username[0] || !password || !password[0]) {
        snprintf(result.error, sizeof(result.error),
                 "homeserver, username, and password are required");
        return result;
    }

    json_object *body = json_object_new_object();
    json_object *auth = json_object_new_object();
    json_object_object_add(auth, "type",
                           json_object_new_string("m.login.dummy"));
    json_object_object_add(body, "auth", auth);
    json_object_object_add(body, "username",
                           json_object_new_string(username));
    json_object_object_add(body, "password",
                           json_object_new_string(password));

    json_object *resp = matrixApiPost(homeserverUrl,
                                      "/_matrix/client/v3/register", body);
    json_object_put(body);
    return parseAuthResponse(resp);
}

ChatAuthResult chatclientRegisterComplete(const char *homeserverUrl,
                                          const char *session,
                                          const char *username,
                                          const char *password) {
    ChatAuthResult result = { .success = false };
    if (!homeserverUrl || !homeserverUrl[0] ||
        !session || !session[0] ||
        !username || !username[0] || !password || !password[0]) {
        snprintf(result.error, sizeof(result.error),
                 "homeserver, session, username, and password are required");
        return result;
    }

    json_object *body = json_object_new_object();
    json_object *auth = json_object_new_object();
    json_object_object_add(auth, "session",
                           json_object_new_string(session));
    json_object_object_add(body, "auth", auth);
    json_object_object_add(body, "username",
                           json_object_new_string(username));
    json_object_object_add(body, "password",
                           json_object_new_string(password));

    json_object *resp = matrixApiPost(homeserverUrl,
                                      "/_matrix/client/v3/register", body);
    json_object_put(body);
    return parseAuthResponse(resp);
}

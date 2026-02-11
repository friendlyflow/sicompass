#include <provider_tags.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

bool providerTagHasInput(const char *text) {
    if (!text) return false;
    return strstr(text, INPUT_TAG_OPEN) != NULL && strstr(text, INPUT_TAG_CLOSE) != NULL;
}

char* providerTagExtractContent(const char *taggedText) {
    if (!taggedText) return NULL;

    const char *start = strstr(taggedText, INPUT_TAG_OPEN);
    if (!start) return NULL;

    start += INPUT_TAG_OPEN_LEN;

    const char *end = strstr(start, INPUT_TAG_CLOSE);
    if (!end) return NULL;

    size_t len = end - start;
    char *result = malloc(len + 1);
    if (!result) return NULL;

    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

char* providerTagStripDisplay(const char *text) {
    if (!text) return NULL;

    const char *openTag = strstr(text, INPUT_TAG_OPEN);
    const char *closeTag = strstr(text, INPUT_TAG_CLOSE);

    if (!openTag || !closeTag) {
        return strdup(text);
    }

    size_t textLen = strlen(text);
    size_t resultLen = textLen - INPUT_TAG_OPEN_LEN - INPUT_TAG_CLOSE_LEN;

    char *result = malloc(resultLen + 1);
    if (!result) return NULL;

    size_t pos = 0;

    // Copy before <input>
    size_t beforeLen = openTag - text;
    memcpy(result + pos, text, beforeLen);
    pos += beforeLen;

    // Copy between tags
    const char *contentStart = openTag + INPUT_TAG_OPEN_LEN;
    size_t contentLen = closeTag - contentStart;
    memcpy(result + pos, contentStart, contentLen);
    pos += contentLen;

    // Copy after </input>
    const char *afterClose = closeTag + INPUT_TAG_CLOSE_LEN;
    strcpy(result + pos, afterClose);

    return result;
}

char* providerTagFormatKey(const char *content) {
    if (!content) return NULL;

    size_t len = INPUT_TAG_OPEN_LEN + strlen(content) + INPUT_TAG_CLOSE_LEN + 1;
    char *result = malloc(len);
    if (result) {
        snprintf(result, len, INPUT_TAG_OPEN "%s" INPUT_TAG_CLOSE, content);
    }
    return result;
}

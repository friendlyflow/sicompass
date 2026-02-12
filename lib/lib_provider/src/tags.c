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
    size_t openLen = INPUT_TAG_OPEN_LEN;
    size_t closeLen = INPUT_TAG_CLOSE_LEN;

    if (!openTag || !closeTag) {
        openTag = strstr(text, RADIO_TAG_OPEN);
        closeTag = strstr(text, RADIO_TAG_CLOSE);
        openLen = RADIO_TAG_OPEN_LEN;
        closeLen = RADIO_TAG_CLOSE_LEN;
    }

    if (!openTag || !closeTag) {
        openTag = strstr(text, CHECKED_TAG_OPEN);
        closeTag = strstr(text, CHECKED_TAG_CLOSE);
        openLen = CHECKED_TAG_OPEN_LEN;
        closeLen = CHECKED_TAG_CLOSE_LEN;
    }

    if (!openTag || !closeTag) {
        return strdup(text);
    }

    size_t textLen = strlen(text);
    size_t resultLen = textLen - openLen - closeLen;

    char *result = malloc(resultLen + 1);
    if (!result) return NULL;

    size_t pos = 0;

    // Copy before <input>
    size_t beforeLen = openTag - text;
    memcpy(result + pos, text, beforeLen);
    pos += beforeLen;

    // Copy between tags
    const char *contentStart = openTag + openLen;
    size_t contentLen = closeTag - contentStart;
    memcpy(result + pos, contentStart, contentLen);
    pos += contentLen;

    // Copy after close tag
    const char *afterClose = closeTag + closeLen;
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

bool providerTagHasRadio(const char *text) {
    if (!text) return false;
    return strstr(text, RADIO_TAG_OPEN) != NULL && strstr(text, RADIO_TAG_CLOSE) != NULL;
}

bool providerTagHasChecked(const char *text) {
    if (!text) return false;
    return strstr(text, CHECKED_TAG_OPEN) != NULL && strstr(text, CHECKED_TAG_CLOSE) != NULL;
}

char* providerTagExtractRadioContent(const char *taggedText) {
    if (!taggedText) return NULL;

    const char *start = strstr(taggedText, RADIO_TAG_OPEN);
    if (!start) return NULL;

    start += RADIO_TAG_OPEN_LEN;

    const char *end = strstr(start, RADIO_TAG_CLOSE);
    if (!end) return NULL;

    size_t len = end - start;
    char *result = malloc(len + 1);
    if (!result) return NULL;

    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

char* providerTagExtractCheckedContent(const char *taggedText) {
    if (!taggedText) return NULL;

    const char *start = strstr(taggedText, CHECKED_TAG_OPEN);
    if (!start) return NULL;

    start += CHECKED_TAG_OPEN_LEN;

    const char *end = strstr(start, CHECKED_TAG_CLOSE);
    if (!end) return NULL;

    size_t len = end - start;
    char *result = malloc(len + 1);
    if (!result) return NULL;

    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

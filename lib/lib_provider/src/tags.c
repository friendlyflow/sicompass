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
    const char *closeTag = openTag ? strstr(text, INPUT_TAG_CLOSE) : NULL;
    size_t openLen = INPUT_TAG_OPEN_LEN;
    size_t closeLen = INPUT_TAG_CLOSE_LEN;

    if (!openTag) {
        openTag = strstr(text, RADIO_TAG_OPEN);
        closeTag = openTag ? strstr(text, RADIO_TAG_CLOSE) : NULL;
        openLen = RADIO_TAG_OPEN_LEN;
        closeLen = RADIO_TAG_CLOSE_LEN;
    }

    if (!openTag) {
        openTag = strstr(text, CHECKED_TAG_OPEN);
        closeTag = openTag ? strstr(text, CHECKED_TAG_CLOSE) : NULL;
        openLen = CHECKED_TAG_OPEN_LEN;
        closeLen = CHECKED_TAG_CLOSE_LEN;
    }

    // Check <checkbox checked> before <checkbox> (longer match first)
    if (!openTag) {
        openTag = strstr(text, CHECKBOX_CHECKED_TAG_OPEN);
        closeTag = openTag ? strstr(openTag + CHECKBOX_CHECKED_TAG_OPEN_LEN, CHECKBOX_TAG_CLOSE) : NULL;
        openLen = CHECKBOX_CHECKED_TAG_OPEN_LEN;
        closeLen = CHECKBOX_TAG_CLOSE_LEN;
    }

    if (!openTag) {
        openTag = strstr(text, CHECKBOX_TAG_OPEN);
        closeTag = openTag ? strstr(openTag + CHECKBOX_TAG_OPEN_LEN, CHECKBOX_TAG_CLOSE) : NULL;
        openLen = CHECKBOX_TAG_OPEN_LEN;
        closeLen = CHECKBOX_TAG_CLOSE_LEN;
    }

    if (!openTag) {
        return strdup(text);
    }

    size_t textLen = strlen(text);
    size_t resultLen = textLen - openLen - (closeTag ? closeLen : 0);

    char *result = malloc(resultLen + 1);
    if (!result) return NULL;

    size_t pos = 0;

    // Copy before open tag
    size_t beforeLen = openTag - text;
    memcpy(result + pos, text, beforeLen);
    pos += beforeLen;

    // Copy between tags (or to end if no close tag)
    const char *contentStart = openTag + openLen;
    if (closeTag) {
        size_t contentLen = closeTag - contentStart;
        memcpy(result + pos, contentStart, contentLen);
        pos += contentLen;

        // Copy after close tag
        const char *afterClose = closeTag + closeLen;
        strcpy(result + pos, afterClose);
    } else {
        strcpy(result + pos, contentStart);
    }

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
    return strstr(text, RADIO_TAG_OPEN) != NULL;
}

bool providerTagHasChecked(const char *text) {
    if (!text) return false;
    return strstr(text, CHECKED_TAG_OPEN) != NULL;
}

char* providerTagExtractRadioContent(const char *taggedText) {
    if (!taggedText) return NULL;

    const char *start = strstr(taggedText, RADIO_TAG_OPEN);
    if (!start) return NULL;

    start += RADIO_TAG_OPEN_LEN;

    const char *end = strstr(start, RADIO_TAG_CLOSE);
    size_t len = end ? (size_t)(end - start) : strlen(start);
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
    size_t len = end ? (size_t)(end - start) : strlen(start);
    char *result = malloc(len + 1);
    if (!result) return NULL;

    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

char* providerTagFormatCheckedKey(const char *content) {
    if (!content) return NULL;

    size_t len = CHECKED_TAG_OPEN_LEN + strlen(content) + 1;
    char *result = malloc(len);
    if (result) {
        snprintf(result, len, CHECKED_TAG_OPEN "%s", content);
    }
    return result;
}

bool providerTagHasCheckbox(const char *text) {
    if (!text) return false;
    // Must have <checkbox> but NOT <checkbox checked>
    return strstr(text, CHECKBOX_TAG_OPEN) != NULL &&
           strstr(text, CHECKBOX_CHECKED_TAG_OPEN) == NULL;
}

bool providerTagHasCheckboxChecked(const char *text) {
    if (!text) return false;
    return strstr(text, CHECKBOX_CHECKED_TAG_OPEN) != NULL;
}

char* providerTagExtractCheckboxContent(const char *taggedText) {
    if (!taggedText) return NULL;

    const char *start = strstr(taggedText, CHECKBOX_TAG_OPEN);
    if (!start) return NULL;

    start += CHECKBOX_TAG_OPEN_LEN;

    const char *end = strstr(start, CHECKBOX_TAG_CLOSE);
    size_t len = end ? (size_t)(end - start) : strlen(start);
    char *result = malloc(len + 1);
    if (!result) return NULL;

    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

char* providerTagExtractCheckboxCheckedContent(const char *taggedText) {
    if (!taggedText) return NULL;

    const char *start = strstr(taggedText, CHECKBOX_CHECKED_TAG_OPEN);
    if (!start) return NULL;

    start += CHECKBOX_CHECKED_TAG_OPEN_LEN;

    const char *end = strstr(start, CHECKBOX_TAG_CLOSE);
    size_t len = end ? (size_t)(end - start) : strlen(start);
    char *result = malloc(len + 1);
    if (!result) return NULL;

    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

char* providerTagFormatCheckboxKey(const char *content) {
    if (!content) return NULL;

    size_t len = CHECKBOX_TAG_OPEN_LEN + strlen(content) + 1;
    char *result = malloc(len);
    if (result) {
        snprintf(result, len, CHECKBOX_TAG_OPEN "%s", content);
    }
    return result;
}

char* providerTagFormatCheckboxCheckedKey(const char *content) {
    if (!content) return NULL;

    size_t len = CHECKBOX_CHECKED_TAG_OPEN_LEN + strlen(content) + 1;
    char *result = malloc(len);
    if (result) {
        snprintf(result, len, CHECKBOX_CHECKED_TAG_OPEN "%s", content);
    }
    return result;
}

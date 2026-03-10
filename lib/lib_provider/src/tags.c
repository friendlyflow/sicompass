#include <provider_tags.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// Like strstr() but skips matches where the '<' is preceded by '\'.
static const char* tagFindUnescaped(const char *haystack, const char *needle) {
    const char *p = haystack;
    while ((p = strstr(p, needle)) != NULL) {
        if (p > haystack && *(p - 1) == '\\') {
            p++;
            continue;
        }
        return p;
    }
    return NULL;
}

// In-place conversion of \< to < and \> to >.
static void tagUnescape(char *text) {
    if (!text) return;
    char *read = text;
    char *write = text;
    while (*read) {
        if (*read == '\\' && (*(read + 1) == '<' || *(read + 1) == '>')) {
            read++;
        }
        *write++ = *read++;
    }
    *write = '\0';
}

bool providerTagHasInput(const char *text) {
    if (!text) return false;
    return tagFindUnescaped(text, INPUT_TAG_OPEN) != NULL && tagFindUnescaped(text, INPUT_TAG_CLOSE) != NULL;
}

char* providerTagExtractContent(const char *taggedText) {
    if (!taggedText) return NULL;

    const char *start = tagFindUnescaped(taggedText, INPUT_TAG_OPEN);
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

    // Strip opt-tag prefixes before any other processing
    if (providerTagHasOneOpt(text)) {
        char *stripped = providerTagStripOneOpt(text);
        char *result = providerTagStripDisplay(stripped);
        free(stripped);
        return result;
    }
    if (providerTagHasManyOpt(text)) {
        char *stripped = providerTagStripManyOpt(text);
        char *result = providerTagStripDisplay(stripped);
        free(stripped);
        return result;
    }

    // Button tags: <button>function_name</button>display_text
    // Strip entirely: show only display_text (after </button>)
    if (tagFindUnescaped(text, BUTTON_TAG_OPEN)) {
        char *result = providerTagExtractButtonDisplayText(text);
        tagUnescape(result);
        return result;
    }

    const char *openTag = tagFindUnescaped(text, INPUT_TAG_OPEN);
    const char *closeTag = openTag ? strstr(openTag, INPUT_TAG_CLOSE) : NULL;
    size_t openLen = INPUT_TAG_OPEN_LEN;
    size_t closeLen = INPUT_TAG_CLOSE_LEN;

    if (!openTag) {
        openTag = tagFindUnescaped(text, RADIO_TAG_OPEN);
        closeTag = openTag ? strstr(openTag, RADIO_TAG_CLOSE) : NULL;
        openLen = RADIO_TAG_OPEN_LEN;
        closeLen = RADIO_TAG_CLOSE_LEN;
    }

    if (!openTag) {
        openTag = tagFindUnescaped(text, CHECKED_TAG_OPEN);
        closeTag = openTag ? strstr(openTag, CHECKED_TAG_CLOSE) : NULL;
        openLen = CHECKED_TAG_OPEN_LEN;
        closeLen = CHECKED_TAG_CLOSE_LEN;
    }

    // Check <checkbox checked> before <checkbox> (longer match first)
    if (!openTag) {
        openTag = tagFindUnescaped(text, CHECKBOX_CHECKED_TAG_OPEN);
        closeTag = openTag ? strstr(openTag + CHECKBOX_CHECKED_TAG_OPEN_LEN, CHECKBOX_TAG_CLOSE) : NULL;
        openLen = CHECKBOX_CHECKED_TAG_OPEN_LEN;
        closeLen = CHECKBOX_TAG_CLOSE_LEN;
    }

    if (!openTag) {
        openTag = tagFindUnescaped(text, CHECKBOX_TAG_OPEN);
        closeTag = openTag ? strstr(openTag + CHECKBOX_TAG_OPEN_LEN, CHECKBOX_TAG_CLOSE) : NULL;
        openLen = CHECKBOX_TAG_OPEN_LEN;
        closeLen = CHECKBOX_TAG_CLOSE_LEN;
    }

    if (!openTag) {
        openTag = tagFindUnescaped(text, LINK_TAG_OPEN);
        closeTag = openTag ? strstr(openTag, LINK_TAG_CLOSE) : NULL;
        openLen = LINK_TAG_OPEN_LEN;
        closeLen = LINK_TAG_CLOSE_LEN;
    }

    if (!openTag) {
        openTag = tagFindUnescaped(text, IMAGE_TAG_OPEN);
        closeTag = openTag ? strstr(openTag, IMAGE_TAG_CLOSE) : NULL;
        openLen = IMAGE_TAG_OPEN_LEN;
        closeLen = IMAGE_TAG_CLOSE_LEN;
    }

    if (!openTag) {
        char *result = strdup(text);
        tagUnescape(result);
        return result;
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

    tagUnescape(result);
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
    return tagFindUnescaped(text, RADIO_TAG_OPEN) != NULL;
}

bool providerTagHasChecked(const char *text) {
    if (!text) return false;
    return tagFindUnescaped(text, CHECKED_TAG_OPEN) != NULL;
}

char* providerTagExtractRadioContent(const char *taggedText) {
    if (!taggedText) return NULL;

    const char *start = tagFindUnescaped(taggedText, RADIO_TAG_OPEN);
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

    const char *start = tagFindUnescaped(taggedText, CHECKED_TAG_OPEN);
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
    return tagFindUnescaped(text, CHECKBOX_TAG_OPEN) != NULL &&
           tagFindUnescaped(text, CHECKBOX_CHECKED_TAG_OPEN) == NULL;
}

bool providerTagHasCheckboxChecked(const char *text) {
    if (!text) return false;
    return tagFindUnescaped(text, CHECKBOX_CHECKED_TAG_OPEN) != NULL;
}

char* providerTagExtractCheckboxContent(const char *taggedText) {
    if (!taggedText) return NULL;

    const char *start = tagFindUnescaped(taggedText, CHECKBOX_TAG_OPEN);
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

    const char *start = tagFindUnescaped(taggedText, CHECKBOX_CHECKED_TAG_OPEN);
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

bool providerTagHasLink(const char *text) {
    if (!text) return false;
    return tagFindUnescaped(text, LINK_TAG_OPEN) != NULL && tagFindUnescaped(text, LINK_TAG_CLOSE) != NULL;
}

char* providerTagExtractLinkContent(const char *taggedText) {
    if (!taggedText) return NULL;

    const char *start = tagFindUnescaped(taggedText, LINK_TAG_OPEN);
    if (!start) return NULL;

    start += LINK_TAG_OPEN_LEN;

    const char *end = strstr(start, LINK_TAG_CLOSE);
    if (!end) return NULL;

    size_t len = end - start;
    char *result = malloc(len + 1);
    if (!result) return NULL;

    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

bool providerTagHasImage(const char *text) {
    if (!text) return false;
    return tagFindUnescaped(text, IMAGE_TAG_OPEN) != NULL && tagFindUnescaped(text, IMAGE_TAG_CLOSE) != NULL;
}

char* providerTagExtractImageContent(const char *taggedText) {
    if (!taggedText) return NULL;

    const char *start = tagFindUnescaped(taggedText, IMAGE_TAG_OPEN);
    if (!start) return NULL;

    start += IMAGE_TAG_OPEN_LEN;

    const char *end = strstr(start, IMAGE_TAG_CLOSE);
    if (!end) return NULL;

    size_t len = end - start;
    char *result = malloc(len + 1);
    if (!result) return NULL;

    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

bool providerTagHasManyOpt(const char *text) {
    if (!text) return false;
    return strncmp(text, MANY_OPT_TAG, MANY_OPT_TAG_LEN) == 0;
}

bool providerTagHasOneOpt(const char *text) {
    if (!text) return false;
    return strncmp(text, ONE_OPT_TAG, ONE_OPT_TAG_LEN) == 0;
}

char* providerTagStripOneOpt(const char *text) {
    if (!text) return NULL;
    if (strncmp(text, ONE_OPT_TAG, ONE_OPT_TAG_LEN) == 0) {
        return strdup(text + ONE_OPT_TAG_LEN);
    }
    return strdup(text);
}

char* providerTagStripManyOpt(const char *text) {
    if (!text) return NULL;
    if (strncmp(text, MANY_OPT_TAG, MANY_OPT_TAG_LEN) == 0) {
        return strdup(text + MANY_OPT_TAG_LEN);
    }
    return strdup(text);
}

bool providerTagHasButton(const char *text) {
    if (!text) return false;
    return tagFindUnescaped(text, BUTTON_TAG_OPEN) != NULL && tagFindUnescaped(text, BUTTON_TAG_CLOSE) != NULL;
}

char* providerTagExtractButtonFunctionName(const char *taggedText) {
    if (!taggedText) return NULL;

    const char *start = tagFindUnescaped(taggedText, BUTTON_TAG_OPEN);
    if (!start) return NULL;

    start += BUTTON_TAG_OPEN_LEN;

    const char *end = strstr(start, BUTTON_TAG_CLOSE);
    if (!end) return NULL;

    size_t len = end - start;
    char *result = malloc(len + 1);
    if (!result) return NULL;

    memcpy(result, start, len);
    result[len] = '\0';
    return result;
}

char* providerTagExtractButtonDisplayText(const char *taggedText) {
    if (!taggedText) return NULL;

    const char *closeTag = strstr(taggedText, BUTTON_TAG_CLOSE);
    if (!closeTag) return NULL;

    const char *start = closeTag + BUTTON_TAG_CLOSE_LEN;
    return strdup(start);
}

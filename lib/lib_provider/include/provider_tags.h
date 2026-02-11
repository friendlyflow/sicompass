#pragma once

#include <stdbool.h>

#define INPUT_TAG_OPEN "<input>"
#define INPUT_TAG_CLOSE "</input>"
#define INPUT_TAG_OPEN_LEN 7
#define INPUT_TAG_CLOSE_LEN 8

/**
 * Check if text contains <input>...</input> tags.
 */
bool providerTagHasInput(const char *text);

/**
 * Extract content between <input> and </input> tags.
 * Caller must free the returned string.
 */
char* providerTagExtractContent(const char *taggedText);

/**
 * Strip <input> and </input> tags from text for display.
 * Caller must free the returned string.
 */
char* providerTagStripDisplay(const char *text);

/**
 * Wrap content in <input>...</input> tags.
 * Caller must free the returned string.
 */
char* providerTagFormatKey(const char *content);

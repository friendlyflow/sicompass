#pragma once

#include <stdbool.h>

#define INPUT_TAG_OPEN "<input>"
#define INPUT_TAG_CLOSE "</input>"
#define INPUT_TAG_OPEN_LEN 7
#define INPUT_TAG_CLOSE_LEN 8

#define RADIO_TAG_OPEN "<radio>"
#define RADIO_TAG_CLOSE "</radio>"
#define RADIO_TAG_OPEN_LEN 7
#define RADIO_TAG_CLOSE_LEN 8

#define CHECKED_TAG_OPEN "<checked>"
#define CHECKED_TAG_CLOSE "</checked>"
#define CHECKED_TAG_OPEN_LEN 9
#define CHECKED_TAG_CLOSE_LEN 10

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

/**
 * Check if text contains <radio>...</radio> tags.
 */
bool providerTagHasRadio(const char *text);

/**
 * Check if text contains <checked>...</checked> tags.
 */
bool providerTagHasChecked(const char *text);

/**
 * Extract content between <radio> and </radio> tags.
 * Caller must free the returned string.
 */
char* providerTagExtractRadioContent(const char *taggedText);

/**
 * Extract content between <checked> and </checked> tags.
 * Caller must free the returned string.
 */
char* providerTagExtractCheckedContent(const char *taggedText);

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

#define CHECKBOX_TAG_OPEN "<checkbox>"
#define CHECKBOX_TAG_CLOSE "</checkbox>"
#define CHECKBOX_TAG_OPEN_LEN 10
#define CHECKBOX_TAG_CLOSE_LEN 11

#define CHECKBOX_CHECKED_TAG_OPEN "<checkbox checked>"
#define CHECKBOX_CHECKED_TAG_OPEN_LEN 18

#define LINK_TAG_OPEN "<link>"
#define LINK_TAG_CLOSE "</link>"
#define LINK_TAG_OPEN_LEN 6
#define LINK_TAG_CLOSE_LEN 7

#define IMAGE_TAG_OPEN "<image>"
#define IMAGE_TAG_CLOSE "</image>"
#define IMAGE_TAG_OPEN_LEN 7
#define IMAGE_TAG_CLOSE_LEN 8

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

/**
 * Wrap content in <checked> tag (no closing tag).
 * Caller must free the returned string.
 */
char* providerTagFormatCheckedKey(const char *content);

/**
 * Check if text contains <checkbox> tag (but not <checkbox checked>).
 */
bool providerTagHasCheckbox(const char *text);

/**
 * Check if text contains <checkbox checked> tag.
 */
bool providerTagHasCheckboxChecked(const char *text);

/**
 * Extract content after <checkbox> tag.
 * Caller must free the returned string.
 */
char* providerTagExtractCheckboxContent(const char *taggedText);

/**
 * Extract content after <checkbox checked> tag.
 * Caller must free the returned string.
 */
char* providerTagExtractCheckboxCheckedContent(const char *taggedText);

/**
 * Wrap content in <checkbox> tag (no closing tag).
 * Caller must free the returned string.
 */
char* providerTagFormatCheckboxKey(const char *content);

/**
 * Wrap content in <checkbox checked> tag (no closing tag).
 * Caller must free the returned string.
 */
char* providerTagFormatCheckboxCheckedKey(const char *content);

/**
 * Check if text contains <link>...</link> tags.
 */
bool providerTagHasLink(const char *text);

/**
 * Extract content between <link> and </link> tags.
 * Caller must free the returned string.
 */
char* providerTagExtractLinkContent(const char *taggedText);

/**
 * Check if text contains <image>...</image> tags.
 */
bool providerTagHasImage(const char *text);

/**
 * Extract content between <image> and </image> tags.
 * Caller must free the returned string.
 */
char* providerTagExtractImageContent(const char *taggedText);

#pragma once

#include <ffon.h>

// Fetch raw HTML from a URL via libcurl.
// Returns heap-allocated string; caller frees. NULL on failure.
char* webbrowserFetchUrl(const char *url);

// Convert an HTML string into FFON elements.
// Heading hierarchy (h1-h6) maps to FFON nesting depth.
// Returns heap-allocated array of FfonElement*; caller frees.
FfonElement** webbrowserHtmlToFfon(const char *html, const char *baseUrl, int *outCount);

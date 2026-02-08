#pragma once

// Unicode-aware case-insensitive substring search
// Returns pointer to first match in haystack, or NULL if not found
const char* utf8_stristr(const char* haystack, const char* needle);

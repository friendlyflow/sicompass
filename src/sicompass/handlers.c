#include "view.h"
#include "provider.h"
#include "text.h"
#include <provider_tags.h>
#include <platform.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <SDL3/SDL.h>

// UTF-8 helper functions

// Get the length in bytes of a UTF-8 character starting at the given position
static int utf8_char_length(const char *str, int pos) {
    unsigned char c = (unsigned char)str[pos];

    if ((c & 0x80) == 0) {
        // Single-byte character (0xxxxxxx)
        return 1;
    } else if ((c & 0xE0) == 0xC0) {
        // Two-byte character (110xxxxx)
        return 2;
    } else if ((c & 0xF0) == 0xE0) {
        // Three-byte character (1110xxxx)
        return 3;
    } else if ((c & 0xF8) == 0xF0) {
        // Four-byte character (11110xxx)
        return 4;
    }

    // Invalid UTF-8, treat as single byte
    return 1;
}

// Move cursor position backward by one UTF-8 character
// Returns the new cursor position
static int utf8_move_backward(const char *str, int cursorPos) {
    if (cursorPos <= 0) {
        return 0;
    }

    // Move back one byte
    int newPos = cursorPos - 1;

    // Keep moving back while we're in the middle of a multi-byte character
    // A continuation byte has the form 10xxxxxx
    while (newPos > 0 && ((unsigned char)str[newPos] & 0xC0) == 0x80) {
        newPos--;
    }

    return newPos;
}

// Move cursor position forward by one UTF-8 character
// Returns the new cursor position
static int utf8_move_forward(const char *str, int cursorPos, int bufferSize) {
    if (cursorPos >= bufferSize) {
        return bufferSize;
    }

    int charLen = utf8_char_length(str, cursorPos);
    int newPos = cursorPos + charLen;

    // Make sure we don't go past the buffer size
    if (newPos > bufferSize) {
        newPos = bufferSize;
    }

    return newPos;
}

// Selection helpers

bool hasSelection(AppRenderer *appRenderer) {
    return appRenderer->selectionAnchor != -1 &&
           appRenderer->selectionAnchor != appRenderer->cursorPosition;
}

void clearSelection(AppRenderer *appRenderer) {
    appRenderer->selectionAnchor = -1;
}

void getSelectionRange(AppRenderer *appRenderer, int *start, int *end) {
    int a = appRenderer->selectionAnchor;
    int b = appRenderer->cursorPosition;
    *start = (a < b) ? a : b;
    *end = (a > b) ? a : b;
}

void deleteSelection(AppRenderer *appRenderer) {
    if (!hasSelection(appRenderer)) return;
    int start, end;
    getSelectionRange(appRenderer, &start, &end);
    memmove(&appRenderer->inputBuffer[start],
            &appRenderer->inputBuffer[end],
            appRenderer->inputBufferSize - end + 1);
    appRenderer->inputBufferSize -= (end - start);
    appRenderer->cursorPosition = start;
    clearSelection(appRenderer);
}

// Selection-extending handlers

void handleShiftLeft(AppRenderer *appRenderer) {
    if (appRenderer->cursorPosition <= 0) return;

    if (appRenderer->selectionAnchor == -1) {
        appRenderer->selectionAnchor = appRenderer->cursorPosition;
    }

    appRenderer->cursorPosition = utf8_move_backward(
        appRenderer->inputBuffer, appRenderer->cursorPosition);

    caretReset(appRenderer->caretState, SDL_GetTicks());
    appRenderer->needsRedraw = true;
}

void handleShiftRight(AppRenderer *appRenderer) {
    if (appRenderer->cursorPosition >= appRenderer->inputBufferSize) return;

    if (appRenderer->selectionAnchor == -1) {
        appRenderer->selectionAnchor = appRenderer->cursorPosition;
    }

    appRenderer->cursorPosition = utf8_move_forward(
        appRenderer->inputBuffer, appRenderer->cursorPosition,
        appRenderer->inputBufferSize);

    caretReset(appRenderer->caretState, SDL_GetTicks());
    appRenderer->needsRedraw = true;
}

void handleHome(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        // Text scroll mode: go to top of text
        appRenderer->textScrollOffset = 0;
        appRenderer->needsRedraw = true;
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
        appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL) {
        // List navigation: go to first item, or root on double-tap
        uint64_t now = SDL_GetTicks();
        if (now - appRenderer->lastKeypressTime <= DELTA_MS && appRenderer->currentId.depth > 1) {
            // Double-tap: navigate to root ffon[x]
            while (appRenderer->currentId.depth > 1) {
                providerNavigateLeft(appRenderer);
            }
        } else {
            // Single: go to first item at current level
            appRenderer->currentId.ids[appRenderer->currentId.depth - 1] = 0;
        }
        appRenderer->lastKeypressTime = now;
        createListCurrentLayer(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->scrollOffset = appRenderer->listIndex;
        accesskitSpeakCurrentElement(appRenderer);
        appRenderer->needsRedraw = true;
        return;
    }
    // Text cursor: move to start (existing behavior)
    clearSelection(appRenderer);
    appRenderer->cursorPosition = 0;
    caretReset(appRenderer->caretState, SDL_GetTicks());
    appRenderer->needsRedraw = true;
}

void handleShiftHome(AppRenderer *appRenderer) {
    if (appRenderer->selectionAnchor == -1) {
        appRenderer->selectionAnchor = appRenderer->cursorPosition;
    }
    appRenderer->cursorPosition = 0;
    caretReset(appRenderer->caretState, SDL_GetTicks());
    appRenderer->needsRedraw = true;
}

void handleEnd(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        // Text scroll mode: go to bottom of text
        float scale = getTextScale(appRenderer->app, FONT_SIZE_PT);
        int lineHeight = (int)getLineHeight(appRenderer->app, scale, TEXT_PADDING);
        int headerLines = 2;  // header line + gap
        int availableHeight = (int)appRenderer->app->swapChainExtent.height - (lineHeight * headerLines);
        int visibleLines = availableHeight / lineHeight;
        if (visibleLines < 1) visibleLines = 1;

        int maxOffset = appRenderer->textScrollLineCount - visibleLines;
        if (maxOffset < 0) maxOffset = 0;
        appRenderer->textScrollOffset = maxOffset;
        appRenderer->needsRedraw = true;
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
        appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL) {
        // List navigation: go to last item at current level
        if (appRenderer->currentId.depth > 0) {
            int maxId = getFfonMaxIdAtPath(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId);
            if (maxId >= 0) {
                appRenderer->currentId.ids[appRenderer->currentId.depth - 1] = maxId;
                createListCurrentLayer(appRenderer);
                appRenderer->listIndex = maxId;
                appRenderer->scrollOffset = maxId;
                accesskitSpeakCurrentElement(appRenderer);
            }
        }
        appRenderer->needsRedraw = true;
        return;
    }
    // Text cursor: move to end (existing behavior)
    clearSelection(appRenderer);
    appRenderer->cursorPosition = appRenderer->inputBufferSize;
    caretReset(appRenderer->caretState, SDL_GetTicks());
    appRenderer->needsRedraw = true;
}

void handleShiftEnd(AppRenderer *appRenderer) {
    if (appRenderer->selectionAnchor == -1) {
        appRenderer->selectionAnchor = appRenderer->cursorPosition;
    }
    appRenderer->cursorPosition = appRenderer->inputBufferSize;
    caretReset(appRenderer->caretState, SDL_GetTicks());
    appRenderer->needsRedraw = true;
}

void handleCtrlHome(AppRenderer *appRenderer) {
    int count = (appRenderer->filteredListCount > 0) ?
                 appRenderer->filteredListCount : appRenderer->totalListCount;
    if (count > 0) {
        appRenderer->listIndex = 0;
        appRenderer->scrollOffset = 0;
        accesskitSpeakCurrentElement(appRenderer);
    }
    appRenderer->needsRedraw = true;
}

void handleCtrlEnd(AppRenderer *appRenderer) {
    int count = (appRenderer->filteredListCount > 0) ?
                 appRenderer->filteredListCount : appRenderer->totalListCount;
    if (count > 0) {
        appRenderer->listIndex = count - 1;
        appRenderer->scrollOffset = count - 1;
        accesskitSpeakCurrentElement(appRenderer);
    }
    appRenderer->needsRedraw = true;
}

void handleSelectAll(AppRenderer *appRenderer) {
    if (appRenderer->inputBufferSize == 0) return;

    appRenderer->selectionAnchor = 0;
    appRenderer->cursorPosition = appRenderer->inputBufferSize;
    caretReset(appRenderer->caretState, SDL_GetTicks());
    appRenderer->needsRedraw = true;
}

void handleTab(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL ||
        appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        return;
    }

    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH) {
        ListItem *list = appRenderer->filteredListCount > 0 ?
                         appRenderer->filteredListCurrentLayer : appRenderer->totalListCurrentLayer;
        int count = appRenderer->filteredListCount > 0 ?
                    appRenderer->filteredListCount : appRenderer->totalListCount;
        if (appRenderer->listIndex >= 0 && appRenderer->listIndex < count) {
            idArrayCopy(&appRenderer->currentId, &list[appRenderer->listIndex].id);
        }
        appRenderer->currentCoordinate = COORDINATE_SCROLL;
        appRenderer->textScrollOffset = 0;
        appRenderer->textScrollLineCount = 0;
        accesskitSpeakModeChange(appRenderer, NULL);
        appRenderer->needsRedraw = true;
        return;
    }

    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    appRenderer->currentCoordinate = COORDINATE_SIMPLE_SEARCH;
    accesskitSpeakModeChange(appRenderer, NULL);

    // Clear input buffer for searching
    appRenderer->inputBuffer[0] = '\0';
    appRenderer->inputBufferSize = 0;
    appRenderer->cursorPosition = 0;
    appRenderer->selectionAnchor = -1;

    createListCurrentLayer(appRenderer);
    appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    appRenderer->scrollOffset = 0;
    appRenderer->needsRedraw = true;
}

void handleCtrlA(AppRenderer *appRenderer, History history) {
    uint64_t now = SDL_GetTicks();

    if (now - appRenderer->lastKeypressTime <= DELTA_MS) {
        appRenderer->lastKeypressTime = 0;
        handleHistoryAction(appRenderer, HISTORY_UNDO);
        updateState(appRenderer, TASK_APPEND_APPEND, HISTORY_NONE);
    } else {
        updateState(appRenderer, TASK_APPEND, history);
    }

    appRenderer->lastKeypressTime = now;
    appRenderer->needsRedraw = true;
}

// Returns true if the element at elementId is a radio child and was toggled checked + moved to top.
static bool handleRadioSelect(AppRenderer *appRenderer, IdArray *elementId) {
    if (elementId->depth < 2) return false;

    // Resolve the element
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, elementId, &count);
    if (!arr) return false;
    int idx = elementId->ids[elementId->depth - 1];
    if (idx < 0 || idx >= count) return false;
    FfonElement *elem = arr[idx];
    if (elem->type != FFON_STRING) return false;

    // Get parent object and check for <radio> tag
    IdArray parentId;
    idArrayCopy(&parentId, elementId);
    idArrayPop(&parentId);
    int parentCount;
    FfonElement **parentArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &parentId, &parentCount);
    if (!parentArr) return false;
    int parentIdx = parentId.ids[parentId.depth - 1];
    if (parentIdx < 0 || parentIdx >= parentCount) return false;
    FfonElement *parentElem = parentArr[parentIdx];
    if (parentElem->type != FFON_OBJECT) return false;
    if (!providerTagHasRadio(parentElem->data.object->key)) return false;

    FfonObject *parent = parentElem->data.object;

    // Uncheck any currently checked sibling
    for (int i = 0; i < parent->count; i++) {
        FfonElement *child = parent->elements[i];
        if (child->type == FFON_STRING && providerTagHasChecked(child->data.string)) {
            char *content = providerTagExtractCheckedContent(child->data.string);
            if (content) {
                free(child->data.string);
                child->data.string = content;
            }
        }
    }

    // Add <checked> to the selected element (strip display text first if it had tags)
    char *displayText = providerTagStripDisplay(elem->data.string);
    char *checkedKey = providerTagFormatCheckedKey(displayText ? displayText : elem->data.string);
    free(displayText);
    if (checkedKey) {
        free(elem->data.string);
        elem->data.string = checkedKey;
    }

    return true;
}

// Returns true if the element at elementId is a checkbox and was toggled.
static bool handleCheckboxToggle(AppRenderer *appRenderer, IdArray *elementId) {
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, elementId, &count);
    if (!arr) return false;
    int idx = elementId->ids[elementId->depth - 1];
    if (idx < 0 || idx >= count) return false;
    FfonElement *elem = arr[idx];
    if (elem->type != FFON_STRING) return false;

    if (providerTagHasCheckboxChecked(elem->data.string)) {
        // Uncheck: <checkbox checked>content -> <checkbox>content
        char *content = providerTagExtractCheckboxCheckedContent(elem->data.string);
        if (!content) return false;
        char *newKey = providerTagFormatCheckboxKey(content);
        free(content);
        if (newKey) {
            free(elem->data.string);
            elem->data.string = newKey;
        }
        return true;
    } else if (providerTagHasCheckbox(elem->data.string)) {
        // Check: <checkbox>content -> <checkbox checked>content
        char *content = providerTagExtractCheckboxContent(elem->data.string);
        if (!content) return false;
        char *newKey = providerTagFormatCheckboxCheckedKey(content);
        free(content);
        if (newKey) {
            free(elem->data.string);
            elem->data.string = newKey;
        }
        return true;
    }

    return false;
}

void handleEnter(AppRenderer *appRenderer, History history) {
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) return;

    uint64_t now = SDL_GetTicks();

    if (appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT) {
        // Get current element
        int count;
        FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
        if (arr && count > 0) {
            int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                FfonElement *elem = arr[idx];
                const char *elementKey = (elem->type == FFON_STRING) ?
                    elem->data.string : elem->data.object->key;

                // Check if provider handles this element
                char *oldContent = providerTagExtractContent(elementKey);
                if (oldContent) {
                    const char *newContent = appRenderer->inputBuffer;
                    // Only commit if changed
                    if (strcmp(oldContent, newContent) != 0) {
                        bool success;
                        if (oldContent[0] == '\0' && elem->type == FFON_OBJECT) {
                            success = providerCreateDirectory(appRenderer, newContent);
                        } else if (oldContent[0] == '\0' && elem->type == FFON_STRING) {
                            success = providerCreateFile(appRenderer, newContent);
                        } else {
                            success = providerCommitEdit(appRenderer, oldContent, newContent);
                        }
                        if (success) {
                            // Update element with new key
                            char *newKey = providerTagFormatKey(newContent);
                            if (newKey) {
                                if (elem->type == FFON_STRING) {
                                    free(elem->data.string);
                                    elem->data.string = newKey;
                                } else {
                                    free(elem->data.object->key);
                                    elem->data.object->key = newKey;
                                }
                            }
                        }
                    }
                    free(oldContent);
                    // Return to operator general
                    appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
                    appRenderer->previousCoordinate = COORDINATE_OPERATOR_GENERAL;
                    accesskitSpeakModeChange(appRenderer, NULL);
                    createListCurrentLayer(appRenderer);
                    appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                    appRenderer->scrollOffset = 0;
                    appRenderer->needsRedraw = true;
                    appRenderer->lastKeypressTime = now;
                    return;
                }
            }
        }
        // Default behavior: save contents and return to operator general
        updateState(appRenderer, TASK_INPUT, HISTORY_NONE);
        appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
        appRenderer->previousCoordinate = COORDINATE_OPERATOR_GENERAL;
        accesskitSpeakModeChange(appRenderer, NULL);
        appRenderer->needsRedraw = true;
    } else if (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {
        // Check for checkbox toggle first
        if (handleCheckboxToggle(appRenderer, &appRenderer->currentId)) {
            int savedIndex = appRenderer->listIndex;
            createListCurrentLayer(appRenderer);
            appRenderer->listIndex = savedIndex;
            appRenderer->needsRedraw = true;
            appRenderer->lastKeypressTime = now;
            return;
        }
        // Check for radio selection
        if (handleRadioSelect(appRenderer, &appRenderer->currentId)) {
            int savedIndex = appRenderer->listIndex;
            providerNotifyRadioChanged(appRenderer, &appRenderer->currentId);
            createListCurrentLayer(appRenderer);
            appRenderer->listIndex = savedIndex;
            appRenderer->needsRedraw = true;
            appRenderer->lastKeypressTime = now;
            return;
        }
        // Get current element to check if it's a string (file) or object (directory)
        int count;
        FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
        if (arr && count > 0) {
            int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                FfonElement *elem = arr[idx];
                if (elem->type == FFON_STRING) {
                    // Open file with default program
                    char *filename = providerTagExtractContent(elem->data.string);
                    const char *path = providerGetCurrentPath(appRenderer);
                    if (filename && path) {
                        const char *sep = platformGetPathSeparator();
                        char fullPath[MAX_URI_LENGTH * 2 + 2];
                        snprintf(fullPath, sizeof(fullPath), "%s%s%s", path, sep, filename);
                        platformOpenWithDefault(fullPath);
                    }
                    free(filename);
                } else if (elem->type == FFON_OBJECT) {
                    // Navigate into the object
                    handleRight(appRenderer);
                }
            }
        }
        appRenderer->needsRedraw = true;
    } else if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH) {
        // Get selected item from list
        ListItem *list = appRenderer->filteredListCount > 0 ?
                         appRenderer->filteredListCurrentLayer : appRenderer->totalListCurrentLayer;
        int count = appRenderer->filteredListCount > 0 ?
                    appRenderer->filteredListCount : appRenderer->totalListCount;

        if (appRenderer->listIndex >= 0 && appRenderer->listIndex < count) {
            IdArray selectedId;
            idArrayCopy(&selectedId, &list[appRenderer->listIndex].id);

            if (handleCheckboxToggle(appRenderer, &selectedId)) {
                int savedIndex = appRenderer->listIndex;
                createListCurrentLayer(appRenderer);
                appRenderer->listIndex = savedIndex;
                appRenderer->needsRedraw = true;
                appRenderer->lastKeypressTime = now;
                return;
            }

            if (handleRadioSelect(appRenderer, &selectedId)) {
                idArrayCopy(&appRenderer->currentId, &selectedId);
                appRenderer->currentCoordinate = appRenderer->previousCoordinate;
                accesskitSpeakModeChange(appRenderer, NULL);
                providerNotifyRadioChanged(appRenderer, &appRenderer->currentId);
                createListCurrentLayer(appRenderer);
                appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                appRenderer->needsRedraw = true;
                appRenderer->lastKeypressTime = now;
                return;
            }

            idArrayCopy(&appRenderer->currentId, &selectedId);

            // If selected item is an object, try navigating into it
            int ecount;
            FfonElement **earr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                              &appRenderer->currentId, &ecount);
            if (earr) {
                int eidx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                if (eidx >= 0 && eidx < ecount && earr[eidx]->type == FFON_OBJECT) {
                    if (!providerNavigateRight(appRenderer)) {
                        // Navigation rejected (e.g. invalid radio group) - stay in search mode
                        appRenderer->needsRedraw = true;
                        appRenderer->lastKeypressTime = now;
                        return;
                    }
                }
            }
        }
        appRenderer->currentCoordinate = appRenderer->previousCoordinate;
        accesskitSpeakModeChange(appRenderer, NULL);
        createListCurrentLayer(appRenderer);
        // Sync listIndex with current position (after createListCurrentLayer which resets it)
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->scrollOffset = 0;
        appRenderer->needsRedraw = true;
    } else if (appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        // Get selected item from list
        ListItem *list = appRenderer->filteredListCount > 0 ?
                         appRenderer->filteredListCurrentLayer : appRenderer->totalListCurrentLayer;
        int count = appRenderer->filteredListCount > 0 ?
                    appRenderer->filteredListCount : appRenderer->totalListCount;

        if (appRenderer->listIndex >= 0 && appRenderer->listIndex < count) {
            IdArray selectedId;
            idArrayCopy(&selectedId, &list[appRenderer->listIndex].id);

            // Validate ancestor objects between root and selected item
            const char *blockedError = NULL;
            for (int d = 1; d < selectedId.depth - 1; d++) {
                IdArray ancestorId;
                idArrayCopy(&ancestorId, &selectedId);
                ancestorId.depth = d + 1;

                int acount;
                FfonElement **aarr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                                  &ancestorId, &acount);
                if (!aarr) continue;

                int aidx = ancestorId.ids[d];
                if (aidx < 0 || aidx >= acount) continue;

                FfonElement *aelem = aarr[aidx];
                if (aelem->type != FFON_OBJECT) continue;

                if (providerTagHasRadio(aelem->data.object->key) && aelem->data.object->count > 0) {
                    const char *radioError = NULL;
                    int checkedCount = 0;
                    for (int ci = 0; ci < aelem->data.object->count; ci++) {
                        if (aelem->data.object->elements[ci]->type == FFON_OBJECT) {
                            radioError = "Radio group children must be strings, not objects";
                            break;
                        }
                        if (aelem->data.object->elements[ci]->type == FFON_STRING &&
                            providerTagHasChecked(aelem->data.object->elements[ci]->data.string)) {
                            checkedCount++;
                        }
                    }
                    if (!radioError && checkedCount > 1) {
                        radioError = "Radio group must have at most one checked item";
                    }
                    if (radioError) {
                        idArrayCopy(&appRenderer->currentId, &ancestorId);
                        blockedError = radioError;
                        break;
                    }
                }
            }

            if (blockedError) {
                appRenderer->currentCoordinate = appRenderer->previousCoordinate;
                accesskitSpeakModeChange(appRenderer, NULL);
                createListCurrentLayer(appRenderer);
                setErrorMessage(appRenderer, blockedError);
                appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                appRenderer->scrollOffset = 0;
                appRenderer->needsRedraw = true;
                appRenderer->lastKeypressTime = now;
                return;
            }

            if (handleCheckboxToggle(appRenderer, &selectedId)) {
                int savedIndex = appRenderer->listIndex;
                createListExtendedSearch(appRenderer);
                appRenderer->listIndex = savedIndex;
                appRenderer->needsRedraw = true;
                appRenderer->lastKeypressTime = now;
                return;
            }

            if (handleRadioSelect(appRenderer, &selectedId)) {
                idArrayCopy(&appRenderer->currentId, &selectedId);
                appRenderer->currentCoordinate = appRenderer->previousCoordinate;
                accesskitSpeakModeChange(appRenderer, NULL);
                providerNotifyRadioChanged(appRenderer, &appRenderer->currentId);
                createListCurrentLayer(appRenderer);
                appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                appRenderer->needsRedraw = true;
                appRenderer->lastKeypressTime = now;
                return;
            }

            // Deep search item: navigate by path instead of FFON id
            const char *navPath = list[appRenderer->listIndex].navPath;
            if (navPath) {
                const char *slash = strrchr(navPath, '/');
                const char *filename = slash ? slash + 1 : navPath;
                char parentDir[4096];
                if (slash && slash != navPath) {
                    size_t len = (size_t)(slash - navPath);
                    strncpy(parentDir, navPath, len);
                    parentDir[len] = '\0';
                } else {
                    strcpy(parentDir, "/");
                }
                int rootIdx = list[appRenderer->listIndex].id.ids[0];
                providerNavigateToPath(appRenderer, rootIdx, parentDir, filename);

                // currentId is now set by providerNavigateToPath.
                // If the found item is a directory, navigate into it.
                {
                    int ecount;
                    FfonElement **earr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                                      &appRenderer->currentId, &ecount);
                    if (earr) {
                        int eidx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                        if (eidx >= 0 && eidx < ecount && earr[eidx]->type == FFON_OBJECT) {
                            providerNavigateRight(appRenderer);
                        }
                    }
                }

                appRenderer->currentCoordinate = appRenderer->previousCoordinate;
                accesskitSpeakModeChange(appRenderer, NULL);
                createListCurrentLayer(appRenderer);
                appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                appRenderer->scrollOffset = 0;
                appRenderer->needsRedraw = true;
                appRenderer->lastKeypressTime = now;
                return;
            }

            idArrayCopy(&appRenderer->currentId, &selectedId);

            // If selected item is an object, try navigating into it
            int ecount;
            FfonElement **earr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                              &appRenderer->currentId, &ecount);
            if (earr) {
                int eidx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                if (eidx >= 0 && eidx < ecount && earr[eidx]->type == FFON_OBJECT) {
                    if (!providerNavigateRight(appRenderer)) {
                        // Navigation rejected (e.g. invalid radio group) - stay in search mode
                        appRenderer->needsRedraw = true;
                        appRenderer->lastKeypressTime = now;
                        return;
                    }
                }
            }
        }
        appRenderer->currentCoordinate = appRenderer->previousCoordinate;
        accesskitSpeakModeChange(appRenderer, NULL);
        createListCurrentLayer(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->scrollOffset = 0;
        appRenderer->needsRedraw = true;
    } else if (appRenderer->currentCoordinate == COORDINATE_COMMAND) {
        ListItem *list = appRenderer->filteredListCount > 0 ?
                         appRenderer->filteredListCurrentLayer : appRenderer->totalListCurrentLayer;
        int count = appRenderer->filteredListCount > 0 ?
                    appRenderer->filteredListCount : appRenderer->totalListCount;

        if (appRenderer->listIndex >= 0 && appRenderer->listIndex < count) {
            if (appRenderer->currentCommand == COMMAND_PROVIDER) {
                // Execute provider command with selected item
                const char *selection = list[appRenderer->listIndex].data ?
                                       list[appRenderer->listIndex].data :
                                       list[appRenderer->listIndex].label;
                providerExecuteCommand(appRenderer, appRenderer->providerCommandName, selection);
                appRenderer->currentCommand = COMMAND_NONE;
                appRenderer->currentCoordinate = appRenderer->previousCoordinate;
                appRenderer->previousCoordinate = appRenderer->currentCoordinate;
                accesskitSpeakModeChange(appRenderer, NULL);
                createListCurrentLayer(appRenderer);
                appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                appRenderer->scrollOffset = 0;
                appRenderer->needsRedraw = true;
            } else {
                // Execute selected command
                const char *cmd = list[appRenderer->listIndex].label;
                if (strcmp(cmd, "editor mode") == 0) {
                    appRenderer->currentCommand = COMMAND_EDITOR_MODE;
                } else if (strcmp(cmd, "operator mode") == 0) {
                    appRenderer->currentCommand = COMMAND_OPERATOR_MODE;
                } else {
                    appRenderer->currentCommand = COMMAND_PROVIDER;
                    strncpy(appRenderer->providerCommandName, cmd,
                            sizeof(appRenderer->providerCommandName) - 1);
                    appRenderer->providerCommandName[sizeof(appRenderer->providerCommandName) - 1] = '\0';
                }
                handleCommand(appRenderer);
            }
        }
    }

    appRenderer->lastKeypressTime = now;
}

void handleCtrlEnter(AppRenderer *appRenderer, History history) {
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT) {
        updateState(appRenderer, TASK_INPUT, HISTORY_NONE);
        appRenderer->currentCoordinate = COORDINATE_EDITOR_GENERAL;
        handleEscape(appRenderer);
        handleCtrlA(appRenderer, HISTORY_NONE);
    }
}

void handleCtrlI(AppRenderer *appRenderer, History history) {
    uint64_t now = SDL_GetTicks();

    if (now - appRenderer->lastKeypressTime <= DELTA_MS) {
        appRenderer->lastKeypressTime = 0;
        handleHistoryAction(appRenderer, HISTORY_UNDO);
        updateState(appRenderer, TASK_INSERT_INSERT, HISTORY_NONE);
    } else {
        updateState(appRenderer, TASK_INSERT, history);
    }

    appRenderer->lastKeypressTime = now;
    appRenderer->needsRedraw = true;
}

void handleDelete(AppRenderer *appRenderer, History history) {
    updateState(appRenderer, TASK_DELETE, history);
    appRenderer->needsRedraw = true;
}

void handleColon(AppRenderer *appRenderer) {
    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    appRenderer->currentCoordinate = COORDINATE_COMMAND;
    appRenderer->currentCommand = COMMAND_NONE;
    accesskitSpeakModeChange(appRenderer, NULL);

    // Clear input buffer for searching
    appRenderer->inputBuffer[0] = '\0';
    appRenderer->inputBufferSize = 0;
    appRenderer->cursorPosition = 0;
    appRenderer->selectionAnchor = -1;

    createListCurrentLayer(appRenderer);
    appRenderer->scrollOffset = 0;
    appRenderer->needsRedraw = true;
}

void handleUp(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        // Previous match
        if (appRenderer->scrollSearchMatchCount > 0) {
            if (appRenderer->scrollSearchCurrentMatch > 0)
                appRenderer->scrollSearchCurrentMatch--;
            else
                appRenderer->scrollSearchCurrentMatch = appRenderer->scrollSearchMatchCount - 1;
        }
        appRenderer->needsRedraw = true;
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        // Text scroll mode: scroll up one line
        if (appRenderer->textScrollOffset > 0) {
            appRenderer->textScrollOffset--;
        }
        appRenderer->needsRedraw = true;
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        appRenderer->errorMessage[0] = '\0';
        if (appRenderer->listIndex > 0) {
            appRenderer->listIndex--;
            ListItem *list = appRenderer->filteredListCount > 0 ?
                             appRenderer->filteredListCurrentLayer : appRenderer->totalListCurrentLayer;
            int count = appRenderer->filteredListCount > 0 ?
                        appRenderer->filteredListCount : appRenderer->totalListCount;
            if (appRenderer->listIndex >= 0 && appRenderer->listIndex < count &&
                appRenderer->currentCoordinate != COORDINATE_COMMAND) {
                idArrayCopy(&appRenderer->currentId, &list[appRenderer->listIndex].id);
            }
            accesskitSpeakCurrentElement(appRenderer);
        }
    } else if (appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT &&
               appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT) {
        updateState(appRenderer, TASK_K_ARROW_UP, HISTORY_NONE);
        // Sync listIndex with current position in hierarchy
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        accesskitSpeakCurrentElement(appRenderer);
    }
    appRenderer->needsRedraw = true;
}

void handleDown(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        // Next match
        if (appRenderer->scrollSearchMatchCount > 0) {
            if (appRenderer->scrollSearchCurrentMatch < appRenderer->scrollSearchMatchCount - 1)
                appRenderer->scrollSearchCurrentMatch++;
            else
                appRenderer->scrollSearchCurrentMatch = 0;
        }
        appRenderer->needsRedraw = true;
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        // Text scroll mode: scroll down one line
        // Calculate visible lines from window height
        float scale = getTextScale(appRenderer->app, FONT_SIZE_PT);
        int lineHeight = (int)getLineHeight(appRenderer->app, scale, TEXT_PADDING);
        int headerLines = 2;  // header line + gap
        int availableHeight = (int)appRenderer->app->swapChainExtent.height - (lineHeight * headerLines);
        int visibleLines = availableHeight / lineHeight;
        if (visibleLines < 1) visibleLines = 1;

        int maxOffset = appRenderer->textScrollLineCount - visibleLines;
        if (maxOffset < 0) maxOffset = 0;
        if (appRenderer->textScrollOffset < maxOffset) {
            appRenderer->textScrollOffset++;
        }
        appRenderer->needsRedraw = true;
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        appRenderer->errorMessage[0] = '\0';
        int maxIndex = (appRenderer->filteredListCount > 0) ?
                        appRenderer->filteredListCount - 1 :
                        appRenderer->totalListCount - 1;
        if (appRenderer->listIndex < maxIndex) {
            appRenderer->listIndex++;
            ListItem *list = appRenderer->filteredListCount > 0 ?
                             appRenderer->filteredListCurrentLayer : appRenderer->totalListCurrentLayer;
            int count = appRenderer->filteredListCount > 0 ?
                        appRenderer->filteredListCount : appRenderer->totalListCount;
            if (appRenderer->listIndex >= 0 && appRenderer->listIndex < count &&
                appRenderer->currentCoordinate != COORDINATE_COMMAND) {
                idArrayCopy(&appRenderer->currentId, &list[appRenderer->listIndex].id);
            }
            accesskitSpeakCurrentElement(appRenderer);
        }
    } else if (appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT &&
               appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT) {
        updateState(appRenderer, TASK_J_ARROW_DOWN, HISTORY_NONE);
        // Sync listIndex with current position in hierarchy
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        accesskitSpeakCurrentElement(appRenderer);
    }
    appRenderer->needsRedraw = true;
}

void handlePageUp(AppRenderer *appRenderer) {
    // Skip insert modes
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT) {
        return;
    }

    // Calculate page size from window height and line height
    float scale = getTextScale(appRenderer->app, FONT_SIZE_PT);
    int lineHeight = (int)getLineHeight(appRenderer->app, scale, TEXT_PADDING);
    int pageSize = lineHeight > 0 ? (int)appRenderer->app->swapChainExtent.height / lineHeight - 3 : 10;
    if (pageSize < 1) pageSize = 1;

    if (appRenderer->currentCoordinate == COORDINATE_SCROLL ||
        appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        // Text scroll mode: scroll up by page
        appRenderer->textScrollOffset -= pageSize;
        if (appRenderer->textScrollOffset < 0) {
            appRenderer->textScrollOffset = 0;
        }
        appRenderer->needsRedraw = true;
        return;
    }

    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        // Search/command/extended search modes: adjust listIndex
        appRenderer->errorMessage[0] = '\0';
        int count = (appRenderer->filteredListCount > 0) ?
                     appRenderer->filteredListCount :
                     appRenderer->totalListCount;
        if (count > 0) {
            appRenderer->listIndex -= pageSize;
            if (appRenderer->listIndex < 0) {
                appRenderer->listIndex = 0;
            }
            appRenderer->scrollOffset = appRenderer->listIndex;
            accesskitSpeakCurrentElement(appRenderer);
        }
    } else if (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
               appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL) {
        // General modes: adjust currentId directly
        if (appRenderer->currentId.depth > 0) {
            int maxId = getFfonMaxIdAtPath(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId);
            if (maxId >= 0) {
                int newId = appRenderer->currentId.ids[appRenderer->currentId.depth - 1] - pageSize;
                if (newId < 0) newId = 0;
                appRenderer->currentId.ids[appRenderer->currentId.depth - 1] = newId;

                // Rebuild list and sync listIndex
                createListCurrentLayer(appRenderer);
                appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                appRenderer->scrollOffset = appRenderer->listIndex;
                accesskitSpeakCurrentElement(appRenderer);
            }
        }
    }

    appRenderer->needsRedraw = true;
}

void handlePageDown(AppRenderer *appRenderer) {
    // Skip insert modes
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT) {
        return;
    }

    // Calculate page size from window height and line height
    float scale = getTextScale(appRenderer->app, FONT_SIZE_PT);
    int lineHeight = (int)getLineHeight(appRenderer->app, scale, TEXT_PADDING);
    int pageSize = lineHeight > 0 ? (int)appRenderer->app->swapChainExtent.height / lineHeight - 3 : 10;
    if (pageSize < 1) pageSize = 1;

    if (appRenderer->currentCoordinate == COORDINATE_SCROLL ||
        appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        // Text scroll mode: scroll down by page
        int headerLines = 2;  // header line + gap
        int availableHeight = (int)appRenderer->app->swapChainExtent.height - (lineHeight * headerLines);
        int visibleLines = availableHeight / lineHeight;
        if (visibleLines < 1) visibleLines = 1;

        int maxOffset = appRenderer->textScrollLineCount - visibleLines;
        if (maxOffset < 0) maxOffset = 0;

        appRenderer->textScrollOffset += pageSize;
        if (appRenderer->textScrollOffset > maxOffset) {
            appRenderer->textScrollOffset = maxOffset;
        }
        appRenderer->needsRedraw = true;
        return;
    }

    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        // Search/command/extended search modes: adjust listIndex
        appRenderer->errorMessage[0] = '\0';
        int count = (appRenderer->filteredListCount > 0) ?
                     appRenderer->filteredListCount :
                     appRenderer->totalListCount;
        if (count > 0) {
            appRenderer->listIndex += pageSize;
            if (appRenderer->listIndex >= count) {
                appRenderer->listIndex = count - 1;
            }
            appRenderer->scrollOffset = appRenderer->listIndex;
            accesskitSpeakCurrentElement(appRenderer);
        }
    } else if (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
               appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL) {
        // General modes: adjust currentId directly
        if (appRenderer->currentId.depth > 0) {
            int maxId = getFfonMaxIdAtPath(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId);
            if (maxId >= 0) {
                int newId = appRenderer->currentId.ids[appRenderer->currentId.depth - 1] + pageSize;
                if (newId > maxId) newId = maxId;
                appRenderer->currentId.ids[appRenderer->currentId.depth - 1] = newId;

                // Rebuild list and sync listIndex
                createListCurrentLayer(appRenderer);
                appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                appRenderer->scrollOffset = appRenderer->listIndex;
                accesskitSpeakCurrentElement(appRenderer);
            }
        }
    }

    appRenderer->needsRedraw = true;
}

void handleLeft(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        // Text scroll mode: left/right navigation disabled
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        // If selection active, jump to selection start and clear
        if (hasSelection(appRenderer)) {
            int start, end;
            getSelectionRange(appRenderer, &start, &end);
            appRenderer->cursorPosition = start;
            clearSelection(appRenderer);
            caretReset(appRenderer->caretState, SDL_GetTicks());
            appRenderer->needsRedraw = true;
            return;
        }
        if (appRenderer->cursorPosition > 0) {
            // Move backward by one UTF-8 character
            appRenderer->cursorPosition = utf8_move_backward(
                appRenderer->inputBuffer,
                appRenderer->cursorPosition
            );

            // Reset caret to visible when user presses left arrow
            uint64_t currentTime = SDL_GetTicks();
            caretReset(appRenderer->caretState, currentTime);

            accesskitSpeakCurrentElement(appRenderer);

            appRenderer->needsRedraw = true;
        } else if (appRenderer->currentCoordinate != COORDINATE_SCROLL_SEARCH &&
                   providerNavigateLeft(appRenderer)) {
            if (appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
                createListExtendedSearch(appRenderer);
            } else {
                createListCurrentLayer(appRenderer);
            }
            appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            accesskitSpeakCurrentElement(appRenderer);
            appRenderer->needsRedraw = true;
        }
    } else {
        // Use provider for navigation
        if (providerNavigateLeft(appRenderer)) {
            // Rebuild list for new location
            createListCurrentLayer(appRenderer);
            // Sync listIndex with current position in hierarchy
            appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            accesskitSpeakCurrentElement(appRenderer);
            appRenderer->needsRedraw = true;
        }
    }
}

void handleRight(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        // Text scroll mode: left/right navigation disabled
        return;
    }
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        // If selection active, jump to selection end and clear
        if (hasSelection(appRenderer)) {
            int start, end;
            getSelectionRange(appRenderer, &start, &end);
            appRenderer->cursorPosition = end;
            clearSelection(appRenderer);
            caretReset(appRenderer->caretState, SDL_GetTicks());
            appRenderer->needsRedraw = true;
            return;
        }
        if (appRenderer->cursorPosition < appRenderer->inputBufferSize) {
            // Move forward by one UTF-8 character
            appRenderer->cursorPosition = utf8_move_forward(
                appRenderer->inputBuffer,
                appRenderer->cursorPosition,
                appRenderer->inputBufferSize
            );

            // Reset caret to visible when user presses right arrow
            uint64_t currentTime = SDL_GetTicks();
            caretReset(appRenderer->caretState, currentTime);

            accesskitSpeakCurrentElement(appRenderer);

            appRenderer->needsRedraw = true;
        } else if (appRenderer->currentCoordinate != COORDINATE_SCROLL_SEARCH &&
                   providerNavigateRight(appRenderer)) {
            createListCurrentLayer(appRenderer);
            appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            appRenderer->scrollOffset = appRenderer->listIndex;
            accesskitSpeakCurrentElement(appRenderer);
            appRenderer->needsRedraw = true;
        }
    } else {
        // Use provider for navigation (fetches children dynamically)
        if (providerNavigateRight(appRenderer)) {
            // Rebuild list for new location
            createListCurrentLayer(appRenderer);
            // Sync listIndex with currentId (normally 0 for new child,
            // but may differ if validation bounced back to parent)
            appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            appRenderer->scrollOffset = appRenderer->listIndex;
            accesskitSpeakCurrentElement(appRenderer);
        }
        appRenderer->needsRedraw = true;
    }
}

void handleI(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {

        // In operator mode, only allow insert on provider-editable elements
        if (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {
            int count;
            FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
            if (arr && count > 0) {
                int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                if (idx >= 0 && idx < count) {
                    FfonElement *elem = arr[idx];
                    const char *elementKey = (elem->type == FFON_STRING) ?
                        elem->data.string : elem->data.object->key;
                    char *content = providerTagExtractContent(elementKey);
                    if (!content) return;
                    free(content);
                }
            }
        }

        idArrayCopy(&appRenderer->currentInsertId, &appRenderer->currentId);
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        appRenderer->currentCoordinate = (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) ?
            COORDINATE_OPERATOR_INSERT : COORDINATE_EDITOR_INSERT;

        // Clear the input buffer first
        appRenderer->inputBuffer[0] = '\0';
        appRenderer->inputBufferSize = 0;

        // Get current element
        int count;
        FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
        const char *context = NULL;
        if (arr && count > 0) {
            int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                FfonElement *elem = arr[idx];
                const char *elementKey = (elem->type == FFON_STRING) ?
                    elem->data.string : elem->data.object->key;
                context = elementKey;

                // Try provider first
                char *content = providerTagExtractContent(elementKey);
                if (content) {
                    strncpy(appRenderer->inputBuffer, content,
                           appRenderer->inputBufferCapacity - 1);
                    appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
                    free(content);
                } else if (elem->type == FFON_STRING) {
                    // Default: use raw string
                    strncpy(appRenderer->inputBuffer, elem->data.string,
                           appRenderer->inputBufferCapacity - 1);
                    appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
                } else {
                    // For objects, include the colon
                    snprintf(appRenderer->inputBuffer, appRenderer->inputBufferCapacity,
                            "%s:", elem->data.object->key);
                    appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
                }
            }
        }

        // Speak mode change with current item as context
        accesskitSpeakModeChange(appRenderer, context);

        appRenderer->cursorPosition = 0;
        appRenderer->selectionAnchor = -1;
        idArrayInit(&appRenderer->currentInsertId);
        appRenderer->needsRedraw = true;
    }
}

void handleA(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {

        // In operator mode, only allow insert on provider-editable elements
        if (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {
            int count;
            FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
            if (arr && count > 0) {
                int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                if (idx >= 0 && idx < count) {
                    FfonElement *elem = arr[idx];
                    const char *elementKey = (elem->type == FFON_STRING) ?
                        elem->data.string : elem->data.object->key;
                    char *content = providerTagExtractContent(elementKey);
                    if (!content) return;
                    free(content);
                }
            }
        }

        idArrayCopy(&appRenderer->currentInsertId, &appRenderer->currentId);
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        appRenderer->currentCoordinate = (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) ?
            COORDINATE_OPERATOR_INSERT : COORDINATE_EDITOR_INSERT;

        // Clear the input buffer first
        appRenderer->inputBuffer[0] = '\0';
        appRenderer->inputBufferSize = 0;

        // Get current element
        int count;
        FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
        const char *context = NULL;
        if (arr && count > 0) {
            int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                FfonElement *elem = arr[idx];
                const char *elementKey = (elem->type == FFON_STRING) ?
                    elem->data.string : elem->data.object->key;
                context = elementKey;

                // Try provider first
                char *content = providerTagExtractContent(elementKey);
                if (content) {
                    strncpy(appRenderer->inputBuffer, content,
                           appRenderer->inputBufferCapacity - 1);
                    appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
                    free(content);
                } else if (elem->type == FFON_STRING) {
                    // Default: use raw string
                    strncpy(appRenderer->inputBuffer, elem->data.string,
                           appRenderer->inputBufferCapacity - 1);
                    appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
                } else {
                    // For objects, include the colon
                    snprintf(appRenderer->inputBuffer, appRenderer->inputBufferCapacity,
                            "%s:", elem->data.object->key);
                    appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
                }
            }
        }

        // Speak mode change with current item as context
        accesskitSpeakModeChange(appRenderer, context);

        appRenderer->cursorPosition = appRenderer->inputBufferSize;
        appRenderer->selectionAnchor = -1;
        idArrayInit(&appRenderer->currentInsertId);
        appRenderer->needsRedraw = true;
    }
}

void handleCtrlF(AppRenderer *appRenderer) {
    uint64_t now = SDL_GetTicks();

    // SCROLL mode: Ctrl+F enters SCROLL_SEARCH
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        appRenderer->previousCoordinate = COORDINATE_SCROLL;
        appRenderer->currentCoordinate = COORDINATE_SCROLL_SEARCH;
        appRenderer->inputBuffer[0] = '\0';
        appRenderer->inputBufferSize = 0;
        appRenderer->cursorPosition = 0;
        appRenderer->selectionAnchor = -1;
        appRenderer->scrollSearchMatchCount = 0;
        appRenderer->scrollSearchCurrentMatch = 0;
        accesskitSpeakModeChange(appRenderer, NULL);
        appRenderer->needsRedraw = true;
        return;
    }

    // SCROLL_SEARCH: Ctrl+F does nothing (no double-tap)
    if (appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        return;
    }

    // Double-tap: if already in extended search, switch to root search
    if (appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH &&
        now - appRenderer->lastKeypressTime <= DELTA_MS) {
        // Reset to root of currentId provider
        appRenderer->currentId.depth = 1;
        appRenderer->inputBuffer[0] = '\0';
        appRenderer->inputBufferSize = 0;
        appRenderer->cursorPosition = 0;
        appRenderer->selectionAnchor = -1;
        appRenderer->scrollOffset = 0;
        createListExtendedSearch(appRenderer);
        appRenderer->listIndex = 0;
        appRenderer->lastKeypressTime = now;
        appRenderer->needsRedraw = true;
        return;
    }

    if (appRenderer->currentCoordinate != COORDINATE_COMMAND &&
        appRenderer->currentCoordinate != COORDINATE_EXTENDED_SEARCH) {
        if (appRenderer->currentCoordinate != COORDINATE_SIMPLE_SEARCH) {
            appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        }
        appRenderer->currentCoordinate = COORDINATE_EXTENDED_SEARCH;
        accesskitSpeakModeChange(appRenderer, NULL);

        // Clear input buffer for searching
        appRenderer->inputBuffer[0] = '\0';
        appRenderer->inputBufferSize = 0;
        appRenderer->cursorPosition = 0;
        appRenderer->selectionAnchor = -1;
        appRenderer->scrollOffset = 0;

        createListExtendedSearch(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->lastKeypressTime = now;
        appRenderer->needsRedraw = true;
    }
}

void handleEscape(AppRenderer *appRenderer) {
    clearSelection(appRenderer);
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT) {
        // Editor mode: Escape saves changes
        int count;
        FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
        if (arr && count > 0) {
            int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                FfonElement *elem = arr[idx];
                const char *elementKey = (elem->type == FFON_STRING) ?
                    elem->data.string : elem->data.object->key;

                // Check if provider handles this element
                char *oldContent = providerTagExtractContent(elementKey);
                if (oldContent) {
                    const char *newContent = appRenderer->inputBuffer;
                    if (strcmp(oldContent, newContent) != 0) {
                        if (providerCommitEdit(appRenderer, oldContent, newContent)) {
                            char *newKey = providerTagFormatKey(newContent);
                            if (newKey) {
                                if (elem->type == FFON_STRING) {
                                    free(elem->data.string);
                                    elem->data.string = newKey;
                                } else {
                                    free(elem->data.object->key);
                                    elem->data.object->key = newKey;
                                }
                            }
                        }
                    }
                    free(oldContent);
                    appRenderer->currentCoordinate = COORDINATE_EDITOR_GENERAL;
                    appRenderer->previousCoordinate = COORDINATE_EDITOR_GENERAL;
                    accesskitSpeakModeChange(appRenderer, NULL);
                    createListCurrentLayer(appRenderer);
                    appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                    appRenderer->scrollOffset = 0;
                    appRenderer->needsRedraw = true;
                    return;
                }
            }
        }
        // Default: save via updateState
        updateState(appRenderer, TASK_INPUT, HISTORY_NONE);
        appRenderer->currentCoordinate = COORDINATE_EDITOR_GENERAL;
    } else if (appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT) {
        // Operator mode: Escape discards changes
        appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    } else if (appRenderer->currentCoordinate == COORDINATE_COMMAND) {
        // Cancel command mode (or open with)
        appRenderer->currentCommand = COMMAND_NONE;
        appRenderer->currentCoordinate = appRenderer->previousCoordinate;
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        accesskitSpeakModeChange(appRenderer, NULL);
        createListCurrentLayer(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->scrollOffset = 0;
        appRenderer->needsRedraw = true;
        return;
    } else if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH) {
        // Search mode: Escape cancels search without selecting
        appRenderer->currentCoordinate = appRenderer->previousCoordinate;
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        accesskitSpeakModeChange(appRenderer, NULL);
        createListCurrentLayer(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->scrollOffset = 0;
        appRenderer->needsRedraw = true;
        return;
    } else if (appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        // Extended search mode: Escape cancels search without selecting
        appRenderer->currentCoordinate = appRenderer->previousCoordinate;
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        accesskitSpeakModeChange(appRenderer, NULL);
        createListCurrentLayer(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->scrollOffset = 0;
        appRenderer->needsRedraw = true;
        return;
    } else if (appRenderer->currentCoordinate == COORDINATE_SCROLL_SEARCH) {
        appRenderer->currentCoordinate = COORDINATE_SCROLL;
        appRenderer->scrollSearchMatchCount = 0;
        appRenderer->scrollSearchCurrentMatch = 0;
        accesskitSpeakModeChange(appRenderer, NULL);
        appRenderer->needsRedraw = true;
        return;
    } else if (appRenderer->currentCoordinate == COORDINATE_SCROLL) {
        appRenderer->currentCoordinate = COORDINATE_SIMPLE_SEARCH;
        appRenderer->inputBuffer[0] = '\0';
        appRenderer->inputBufferSize = 0;
        appRenderer->cursorPosition = 0;
        appRenderer->selectionAnchor = -1;
        createListCurrentLayer(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->scrollOffset = 0;
        accesskitSpeakModeChange(appRenderer, NULL);
        appRenderer->needsRedraw = true;
        return;
    } else if (appRenderer->previousCoordinate == COORDINATE_OPERATOR_GENERAL ||
               appRenderer->previousCoordinate == COORDINATE_OPERATOR_INSERT) {
        appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    } else {
        appRenderer->currentCoordinate = COORDINATE_EDITOR_GENERAL;
    }

    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    accesskitSpeakModeChange(appRenderer, NULL);
    appRenderer->needsRedraw = true;
}

void handleCommand(AppRenderer *appRenderer) {
    switch (appRenderer->currentCommand) {
        case COMMAND_NONE:
            break;

        case COMMAND_EDITOR_MODE:
            appRenderer->previousCoordinate = appRenderer->currentCoordinate;
            appRenderer->currentCoordinate = COORDINATE_EDITOR_GENERAL;
            accesskitSpeakModeChange(appRenderer, NULL);
            break;

        case COMMAND_OPERATOR_MODE:
            appRenderer->previousCoordinate = appRenderer->currentCoordinate;
            appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
            accesskitSpeakModeChange(appRenderer, NULL);
            break;

        case COMMAND_PROVIDER: {
            int count;
            FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                            &appRenderer->currentId, &count);
            if (!arr || count == 0) break;
            int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            if (idx < 0 || idx >= count) break;
            FfonElement *elem = arr[idx];
            const char *elementKey = (elem->type == FFON_STRING) ?
                elem->data.string : elem->data.object->key;

            char errorMsg[256] = {0};
            FfonElement *newElem = providerHandleCommand(appRenderer,
                appRenderer->providerCommandName, elementKey, elem->type, errorMsg, sizeof(errorMsg));

            if (newElem) {
                // Insert after current position
                int insertIdx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1] + 1;

                if (appRenderer->currentId.depth == 1) {
                    // Root level: insert into appRenderer->ffon[]
                    if (appRenderer->ffonCount >= appRenderer->ffonCapacity) {
                        appRenderer->ffonCapacity *= 2;
                        appRenderer->ffon = realloc(appRenderer->ffon,
                            appRenderer->ffonCapacity * sizeof(FfonElement*));
                    }
                    memmove(&appRenderer->ffon[insertIdx + 1],
                            &appRenderer->ffon[insertIdx],
                            (appRenderer->ffonCount - insertIdx) * sizeof(FfonElement*));
                    appRenderer->ffon[insertIdx] = newElem;
                    appRenderer->ffonCount++;
                } else {
                    // Nested: get parent object and insert
                    IdArray parentId;
                    idArrayCopy(&parentId, &appRenderer->currentId);
                    idArrayPop(&parentId);
                    int parentCount;
                    FfonElement **parentArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                                           &parentId, &parentCount);
                    int parentIdx = parentId.ids[parentId.depth - 1];
                    if (parentArr && parentIdx >= 0 && parentIdx < parentCount &&
                        parentArr[parentIdx]->type == FFON_OBJECT) {
                        ffonObjectInsertElement(parentArr[parentIdx]->data.object, newElem, insertIdx);
                    }
                }

                // Move cursor to new element
                appRenderer->currentId.ids[appRenderer->currentId.depth - 1] = insertIdx;

                // Switch to operator general, refresh list, then enter insert mode
                appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
                createListCurrentLayer(appRenderer);
                appRenderer->listIndex = insertIdx;
                appRenderer->scrollOffset = 0;
                handleI(appRenderer);
            } else if (errorMsg[0]) {
                setErrorMessage(appRenderer, errorMsg);
                appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
                appRenderer->currentCommand = COMMAND_NONE;
            } else {
                // Command needs secondary selection (e.g., open-with app list)
                appRenderer->inputBuffer[0] = '\0';
                appRenderer->inputBufferSize = 0;
                appRenderer->cursorPosition = 0;
                createListCurrentLayer(appRenderer);
                appRenderer->scrollOffset = 0;
                // If no secondary items were produced, the command was a state-toggle
                // (e.g., "show/hide properties"). Return to normal mode and refresh.
                if (appRenderer->totalListCount == 0) {
                    appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
                    appRenderer->previousCoordinate = COORDINATE_OPERATOR_GENERAL;
                    appRenderer->currentCommand = COMMAND_NONE;
                    providerRefreshCurrentDirectory(appRenderer);
                    createListCurrentLayer(appRenderer);
                    // Sync visual selection with logical cursor so Enter acts on the
                    // highlighted item (createListCurrentLayer resets listIndex to 0).
                    appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                    appRenderer->scrollOffset = 0;
                }
            }
            break;
        }
    }

    appRenderer->needsRedraw = true;
}

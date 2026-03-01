#include "view.h"
#include "provider.h"
#include "text.h"
#include <provider_tags.h>
#include <platform.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <SDL3/SDL.h>

// Forward declarations
static char* resolveSaveFolder(AppRenderer *appRenderer);
static void handleFileBrowserSaveAs(AppRenderer *appRenderer);
static bool loadProviderConfigFromFile(AppRenderer *appRenderer, const char *filepath, int rootIdx);

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

// Returns true if the element at elementId is a button and the press was dispatched.
static bool handleButtonPress(AppRenderer *appRenderer, IdArray *elementId) {
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, elementId, &count);
    if (!arr) return false;
    int idx = elementId->ids[elementId->depth - 1];
    if (idx < 0 || idx >= count) return false;
    FfonElement *elem = arr[idx];
    if (elem->type != FFON_STRING) return false;
    if (!providerTagHasButton(elem->data.string)) return false;
    providerNotifyButtonPressed(appRenderer, elementId);
    return true;
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

                    // Prefix-based creation (from Ctrl+I/Ctrl+A in operator general)
                    if (oldContent[0] == '\0' && appRenderer->prefixedInsertMode) {
                        bool isFile = false, isDir = false;
                        const char *name = NULL;
                        if (newContent[0] == '-') {
                            isFile = true;
                            name = newContent + 1;
                            while (*name == ' ') name++;
                        } else if (newContent[0] == '+') {
                            isDir = true;
                            name = newContent + 1;
                            while (*name == ' ') name++;
                        }

                        if ((!isFile && !isDir) || !name || name[0] == '\0') {
                            setErrorMessage(appRenderer, "Start with '- name' for file or '+ name' for directory");
                            free(oldContent);
                            appRenderer->needsRedraw = true;
                            return;  // stay in COORDINATE_OPERATOR_INSERT
                        }

                        bool success;
                        if (isFile) {
                            success = providerCreateFile(appRenderer, name);
                        } else {
                            success = providerCreateDirectory(appRenderer, name);
                        }

                        if (!success) {
                            if (appRenderer->errorMessage[0] == '\0')
                                setErrorMessage(appRenderer, "Failed to create item");
                            free(oldContent);
                            appRenderer->needsRedraw = true;
                            return;  // stay in COORDINATE_OPERATOR_INSERT
                        }

                        if (isFile) {
                            char *newKey = providerTagFormatKey(name);
                            if (newKey) {
                                free(elem->data.string);
                                elem->data.string = newKey;
                            }
                        } else {
                            char *newKey = providerTagFormatKey(name);
                            FfonElement *dirElem = ffonElementCreateObject(newKey ? newKey : name);
                            free(newKey);
                            ffonElementDestroy(arr[idx]);
                            arr[idx] = dirElem;
                        }

                        free(oldContent);
                        appRenderer->prefixedInsertMode = false;
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

                    // File-browser save-as: save source provider data to new file
                    if (appRenderer->pendingFileBrowserSaveAs && oldContent[0] == '\0') {
                        if (newContent[0] == '\0') {
                            // Empty filename — do nothing, stay in insert mode
                            free(oldContent);
                            appRenderer->needsRedraw = true;
                            return;
                        }

                        char *saveDir = resolveSaveFolder(appRenderer);
                        if (!saveDir) {
                            setErrorMessage(appRenderer, "Cannot determine save folder");
                            free(oldContent);
                            appRenderer->needsRedraw = true;
                            return;
                        }

                        // Build filename with .json extension, handle duplicates
                        char baseName[256];
                        snprintf(baseName, sizeof(baseName), "%s", newContent);
                        char destName[MAX_URI_LENGTH];
                        snprintf(destName, sizeof(destName), "%s.json", baseName);
                        char destFull[MAX_URI_LENGTH];
                        snprintf(destFull, sizeof(destFull), "%s/%s", saveDir, destName);

                        struct stat stCheck;
                        int copyNum = 0;
                        while (stat(destFull, &stCheck) == 0) {
                            copyNum++;
                            snprintf(destName, sizeof(destName), "%s (copy %d).json", baseName, copyNum);
                            snprintf(destFull, sizeof(destFull), "%s/%s", saveDir, destName);
                        }
                        free(saveDir);

                        // Save source provider data to the file
                        int srcIdx = appRenderer->saveAsSourceRootIdx;
                        FfonElement *srcRoot = appRenderer->ffon[srcIdx];
                        if (srcRoot && srcRoot->type == FFON_OBJECT) {
                            FfonObject *srcObj = srcRoot->data.object;
                            json_object *array = ffonElementsToJsonArray(srcObj->elements, srcObj->count);
                            if (json_object_to_file_ext(destFull, array, JSON_C_TO_STRING_PRETTY) == 0) {
                                snprintf(appRenderer->currentSavePath, sizeof(appRenderer->currentSavePath), "%s", destFull);
                                char msg[256];
                                snprintf(msg, sizeof(msg), "Saved to %s", destFull);
                                setErrorMessage(appRenderer, msg);
                            } else {
                                setErrorMessage(appRenderer, "Failed to write file");
                            }
                            json_object_put(array);
                        }

                        // Remove the placeholder element from file browser
                        int depth = appRenderer->currentId.depth;
                        int removeIdx = appRenderer->currentId.ids[depth - 1];
                        IdArray parentId;
                        idArrayCopy(&parentId, &appRenderer->currentId);
                        idArrayPop(&parentId);
                        int parentCount;
                        FfonElement **parentArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                                               &parentId, &parentCount);
                        int parentIdx = parentId.ids[parentId.depth - 1];
                        if (parentArr && parentIdx >= 0 && parentIdx < parentCount &&
                            parentArr[parentIdx]->type == FFON_OBJECT) {
                            FfonObject *parentObj = parentArr[parentIdx]->data.object;
                            ffonElementDestroy(parentObj->elements[removeIdx]);
                            memmove(&parentObj->elements[removeIdx], &parentObj->elements[removeIdx + 1],
                                    (parentObj->count - removeIdx - 1) * sizeof(FfonElement*));
                            parentObj->count--;
                        }

                        // Navigate back to source provider
                        idArrayCopy(&appRenderer->currentId, &appRenderer->saveAsReturnId);
                        appRenderer->pendingFileBrowserSaveAs = false;
                        appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
                        appRenderer->previousCoordinate = COORDINATE_OPERATOR_GENERAL;
                        accesskitSpeakModeChange(appRenderer, NULL);
                        char savedError[256];
                        memcpy(savedError, appRenderer->errorMessage, sizeof(savedError));
                        createListCurrentLayer(appRenderer);
                        if (savedError[0] != '\0')
                            memcpy(appRenderer->errorMessage, savedError, sizeof(appRenderer->errorMessage));
                        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                        appRenderer->scrollOffset = 0;
                        appRenderer->needsRedraw = true;
                        appRenderer->lastKeypressTime = now;
                        free(oldContent);
                        return;
                    }

                    // Only commit if changed
                    if (strcmp(oldContent, newContent) != 0) {
                        bool success;
                        if (oldContent[0] == '\0' && elem->type == FFON_OBJECT) {
                            success = providerCreateDirectory(appRenderer, newContent);
                        } else if (oldContent[0] == '\0' && elem->type == FFON_STRING) {
                            Provider *p = providerGetActive(appRenderer);
                            if (p && p->createFile) {
                                success = providerCreateFile(appRenderer, newContent);
                            } else {
                                // Provider has no file creation: update FFON element directly
                                char *newKey = providerTagFormatKey(newContent);
                                if (newKey) {
                                    free(elem->data.string);
                                    elem->data.string = newKey;
                                    success = true;
                                }
                            }
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
                    // Preserve error set by commitEdit callback (createListCurrentLayer clears it)
                    char savedError[256];
                    memcpy(savedError, appRenderer->errorMessage, sizeof(savedError));
                    createListCurrentLayer(appRenderer);
                    if (savedError[0] != '\0')
                        memcpy(appRenderer->errorMessage, savedError, sizeof(appRenderer->errorMessage));
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
        // Check for button press
        if (handleButtonPress(appRenderer, &appRenderer->currentId)) {
            createListCurrentLayer(appRenderer);
            appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
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
                    char *filename = providerTagExtractContent(elem->data.string);
                    const char *path = providerGetCurrentPath(appRenderer);

                    // File-browser open: load selected .json into source provider
                    if (appRenderer->pendingFileBrowserOpen && filename && path) {
                        size_t len = strlen(filename);
                        if (len < 5 || strcmp(filename + len - 5, ".json") != 0) {
                            setErrorMessage(appRenderer, "Please select a .json file");
                            free(filename);
                            appRenderer->needsRedraw = true;
                            appRenderer->lastKeypressTime = now;
                            return;
                        }
                        const char *sep = platformGetPathSeparator();
                        char fullPath[MAX_URI_LENGTH * 2 + 2];
                        snprintf(fullPath, sizeof(fullPath), "%s%s%s", path, sep, filename);
                        free(filename);

                        int srcIdx = appRenderer->saveAsSourceRootIdx;
                        if (loadProviderConfigFromFile(appRenderer, fullPath, srcIdx)) {
                            // Restore navigation to source provider
                            appRenderer->currentId.depth = 2;
                            appRenderer->currentId.ids[0] = srcIdx;
                            appRenderer->currentId.ids[1] = 0;
                        } else {
                            // Load failed — return to source provider anyway
                            idArrayCopy(&appRenderer->currentId, &appRenderer->saveAsReturnId);
                        }
                        appRenderer->pendingFileBrowserOpen = false;
                        char savedError[256];
                        memcpy(savedError, appRenderer->errorMessage, sizeof(savedError));
                        createListCurrentLayer(appRenderer);
                        if (savedError[0] != '\0')
                            memcpy(appRenderer->errorMessage, savedError, sizeof(appRenderer->errorMessage));
                        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                        appRenderer->scrollOffset = 0;
                        appRenderer->needsRedraw = true;
                        appRenderer->lastKeypressTime = now;
                        return;
                    }

                    // Open file with default program
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
    } else if (appRenderer->currentCoordinate == COORDINATE_COMMAND && appRenderer->pendingSaveAs) {
        // "Save as" mode: input buffer contains the filename
        appRenderer->pendingSaveAs = false;
        const char *filename = appRenderer->inputBuffer;
        if (filename[0] == '\0') {
            setErrorMessage(appRenderer, "No filename provided");
        } else {
            // Build save path
            char *saveDir = resolveSaveFolder(appRenderer);
            if (!saveDir) {
                setErrorMessage(appRenderer, "Cannot determine save folder");
            } else {
                struct stat st;
                if (stat(saveDir, &st) != 0 || !S_ISDIR(st.st_mode)) {
                    char msg[256];
                    snprintf(msg, sizeof(msg), "Save folder does not exist: %s", saveDir);
                    setErrorMessage(appRenderer, msg);
                    free(saveDir);
                } else {
                    char filepath[MAX_URI_LENGTH];
                    snprintf(filepath, sizeof(filepath), "%s/%s.json", saveDir, filename);
                    free(saveDir);

                    int rootIdx = appRenderer->currentId.ids[0];
                    FfonElement *rootElem = appRenderer->ffon[rootIdx];
                    if (rootElem && rootElem->type == FFON_OBJECT) {
                        FfonObject *rootObj = rootElem->data.object;
                        json_object *array = ffonElementsToJsonArray(rootObj->elements, rootObj->count);
                        if (json_object_to_file_ext(filepath, array, JSON_C_TO_STRING_PRETTY) == 0) {
                            snprintf(appRenderer->currentSavePath, sizeof(appRenderer->currentSavePath), "%s", filepath);
                            char msg[512];
                            snprintf(msg, sizeof(msg), "Saved to %s", filepath);
                            setErrorMessage(appRenderer, msg);
                        } else {
                            setErrorMessage(appRenderer, "Failed to write file");
                        }
                        json_object_put(array);
                    }
                }
            }
        }
        appRenderer->currentCommand = COMMAND_NONE;
        appRenderer->currentCoordinate = appRenderer->previousCoordinate;
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
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
                appRenderer->currentCommand = COMMAND_PROVIDER;
                strncpy(appRenderer->providerCommandName, cmd,
                        sizeof(appRenderer->providerCommandName) - 1);
                appRenderer->providerCommandName[sizeof(appRenderer->providerCommandName) - 1] = '\0';
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

void handleFileDelete(AppRenderer *appRenderer) {
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                    &appRenderer->currentId, &count);
    if (!arr || count == 0) return;
    int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (idx < 0 || idx >= count) return;

    FfonElement *elem = arr[idx];

    // Script provider: in-memory element deletion
    Provider *provider = providerGetActive(appRenderer);
    if (provider && provider->createElement &&
        !provider->createFile && !provider->createDirectory) {

        // "Add element:" section handling
        if (elem->type == FFON_OBJECT &&
            strcmp(elem->data.object->key, "Add element:") == 0) {
            // Check if this is a clone (another "Add element:" exists after it)
            bool isClone = false;
            for (int i = idx + 1; i < count; i++) {
                if (arr[i]->type == FFON_OBJECT &&
                    strcmp(arr[i]->data.object->key, "Add element:") == 0) {
                    isClone = true;
                    break;
                }
            }
            if (!isClone) return;  // Don't delete the original "Add element:"
        } else {
            // Only allow deletion of opt elements (tagged with <one-opt> or <opt>)
            const char *ek = (elem->type == FFON_STRING) ?
                elem->data.string : elem->data.object->key;
            if (!providerTagHasOneOpt(ek) && !providerTagHasOpt(ek))
                return;  // Mandatory element, don't delete
        }

        // Save one-opt key before deletion so we can restore the button
        const char *elementKey = (elem->type == FFON_STRING) ?
            elem->data.string : elem->data.object->key;
        char *oneOptKey = NULL;
        if (providerTagHasOneOpt(elementKey)) {
            oneOptKey = providerTagStripOneOpt(elementKey);
        }

        // Remove the element
        updateState(appRenderer, TASK_DELETE, HISTORY_NONE);

        // Restore one-opt button in "Add element:" section
        if (oneOptKey) {
            int depth = appRenderer->currentId.depth;
            FfonElement **siblings = NULL;
            int sibCount = 0;
            FfonObject *parentObj = NULL;

            if (depth == 1) {
                siblings = appRenderer->ffon;
                sibCount = appRenderer->ffonCount;
            } else {
                IdArray pid;
                idArrayCopy(&pid, &appRenderer->currentId);
                idArrayPop(&pid);
                int pc;
                FfonElement **pa = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &pid, &pc);
                int pi = pid.ids[pid.depth - 1];
                if (pa && pi >= 0 && pi < pc && pa[pi]->type == FFON_OBJECT) {
                    parentObj = pa[pi]->data.object;
                    siblings = parentObj->elements;
                    sibCount = parentObj->count;
                }
            }

            if (siblings) {
                // Find existing "Add element:" (the last one is the original)
                FfonObject *addElemObj = NULL;
                for (int i = sibCount - 1; i >= 0; i--) {
                    if (siblings[i]->type == FFON_OBJECT &&
                        strcmp(siblings[i]->data.object->key, "Add element:") == 0) {
                        addElemObj = siblings[i]->data.object;
                        break;
                    }
                }

                // Reconstruct button string: <button>one-opt:KEY</button>KEY
                size_t btnLen = BUTTON_TAG_OPEN_LEN + 8 + strlen(oneOptKey) +
                                BUTTON_TAG_CLOSE_LEN + strlen(oneOptKey) + 1;
                char *btnStr = malloc(btnLen);
                if (btnStr) {
                    snprintf(btnStr, btnLen, BUTTON_TAG_OPEN "one-opt:%s" BUTTON_TAG_CLOSE "%s",
                             oneOptKey, oneOptKey);

                    if (addElemObj) {
                        // Add button back to existing "Add element:"
                        FfonElement *btnElem = ffonElementCreateString(btnStr);
                        if (btnElem) ffonObjectAddElement(addElemObj, btnElem);
                    } else {
                        // Recreate "Add element:" as last child
                        FfonElement *addSection = ffonElementCreateObject("Add element:");
                        if (addSection) {
                            FfonElement *btnElem = ffonElementCreateString(btnStr);
                            if (btnElem) ffonObjectAddElement(addSection->data.object, btnElem);
                            if (parentObj) {
                                ffonObjectAddElement(parentObj, addSection);
                            } else if (depth == 1) {
                                // Top-level: append to ffon array
                                if (appRenderer->ffonCount >= appRenderer->ffonCapacity) {
                                    appRenderer->ffonCapacity *= 2;
                                    FfonElement **newFfon = realloc(appRenderer->ffon,
                                        appRenderer->ffonCapacity * sizeof(FfonElement*));
                                    if (newFfon) appRenderer->ffon = newFfon;
                                }
                                if (appRenderer->ffonCount < appRenderer->ffonCapacity) {
                                    appRenderer->ffon[appRenderer->ffonCount++] = addSection;
                                }
                            }
                        }
                    }
                    free(btnStr);
                }
            }
            free(oneOptKey);
        }

        createListCurrentLayer(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->needsRedraw = true;
        return;
    }

    const char *elementKey = (elem->type == FFON_STRING) ?
        elem->data.string : elem->data.object->key;

    char *name = providerTagExtractContent(elementKey);
    if (!name) return;

    bool success = providerDeleteItem(appRenderer, name);
    free(name);

    if (success) {
        updateState(appRenderer, TASK_DELETE, HISTORY_NONE);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->needsRedraw = true;
    }
}

static void insertOperatorPlaceholder(AppRenderer *appRenderer, int insertIdx) {
    int depth = appRenderer->currentId.depth;

    // For script providers with createElement: clone "Add element:" instead
    Provider *provider = providerGetActive(appRenderer);
    if (provider && provider->createElement &&
        !provider->createFile && !provider->createDirectory) {
        // Find "Add element:" among siblings in the current layer
        FfonElement *addElemSection = NULL;
        if (depth == 1) {
            for (int i = 0; i < appRenderer->ffonCount; i++) {
                if (appRenderer->ffon[i]->type == FFON_OBJECT &&
                    strcmp(appRenderer->ffon[i]->data.object->key, "Add element:") == 0) {
                    addElemSection = appRenderer->ffon[i];
                    break;
                }
            }
        } else {
            IdArray pid;
            idArrayCopy(&pid, &appRenderer->currentId);
            idArrayPop(&pid);
            int pc;
            FfonElement **pa = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &pid, &pc);
            int pi = pid.ids[pid.depth - 1];
            if (pa && pi >= 0 && pi < pc && pa[pi]->type == FFON_OBJECT) {
                FfonObject *parentObj = pa[pi]->data.object;
                for (int i = 0; i < parentObj->count; i++) {
                    if (parentObj->elements[i]->type == FFON_OBJECT &&
                        strcmp(parentObj->elements[i]->data.object->key, "Add element:") == 0) {
                        addElemSection = parentObj->elements[i];
                        break;
                    }
                }
            }
        }
        if (!addElemSection) return;

        FfonElement *clone = ffonElementClone(addElemSection);
        if (!clone) return;

        if (depth == 1) {
            if (appRenderer->ffonCount >= appRenderer->ffonCapacity) {
                appRenderer->ffonCapacity *= 2;
                FfonElement **newFfon = realloc(appRenderer->ffon,
                    appRenderer->ffonCapacity * sizeof(FfonElement*));
                if (!newFfon) { ffonElementDestroy(clone); return; }
                appRenderer->ffon = newFfon;
            }
            memmove(&appRenderer->ffon[insertIdx + 1],
                    &appRenderer->ffon[insertIdx],
                    (appRenderer->ffonCount - insertIdx) * sizeof(FfonElement*));
            appRenderer->ffon[insertIdx] = clone;
            appRenderer->ffonCount++;
        } else {
            IdArray pid;
            idArrayCopy(&pid, &appRenderer->currentId);
            idArrayPop(&pid);
            int pc;
            FfonElement **pa = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &pid, &pc);
            int pi = pid.ids[pid.depth - 1];
            ffonObjectInsertElement(pa[pi]->data.object, clone, insertIdx);
        }

        appRenderer->currentId.ids[depth - 1] = insertIdx;
        createListCurrentLayer(appRenderer);
        appRenderer->listIndex = insertIdx;
        appRenderer->scrollOffset = 0;
        appRenderer->needsRedraw = true;
        return;
    }

    FfonElement *placeholder = ffonElementCreateString("<input></input>");
    if (!placeholder) return;

    if (depth == 1) {
        if (appRenderer->ffonCount >= appRenderer->ffonCapacity) {
            appRenderer->ffonCapacity *= 2;
            FfonElement **newFfon = realloc(appRenderer->ffon,
                appRenderer->ffonCapacity * sizeof(FfonElement*));
            if (!newFfon) { ffonElementDestroy(placeholder); return; }
            appRenderer->ffon = newFfon;
        }
        memmove(&appRenderer->ffon[insertIdx + 1],
                &appRenderer->ffon[insertIdx],
                (appRenderer->ffonCount - insertIdx) * sizeof(FfonElement*));
        appRenderer->ffon[insertIdx] = placeholder;
        appRenderer->ffonCount++;
    } else {
        IdArray parentId;
        idArrayCopy(&parentId, &appRenderer->currentId);
        idArrayPop(&parentId);
        int parentCount;
        FfonElement **parentArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                              &parentId, &parentCount);
        int parentIdx = parentId.ids[parentId.depth - 1];
        if (!parentArr || parentIdx < 0 || parentIdx >= parentCount ||
            parentArr[parentIdx]->type != FFON_OBJECT) {
            ffonElementDestroy(placeholder);
            return;
        }
        ffonObjectInsertElement(parentArr[parentIdx]->data.object, placeholder, insertIdx);
    }

    appRenderer->currentId.ids[depth - 1] = insertIdx;
    appRenderer->prefixedInsertMode = true;

    // Only use prefix mode (- for file, + for dir) if provider supports item creation
    if (!provider || (!provider->createFile && !provider->createDirectory)) {
        appRenderer->prefixedInsertMode = false;
    }

    createListCurrentLayer(appRenderer);
    appRenderer->listIndex = insertIdx;
    appRenderer->scrollOffset = 0;
    handleI(appRenderer);
}

void handleCtrlIOperator(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate != COORDINATE_OPERATOR_GENERAL) return;

    int depth = appRenderer->currentId.depth;
    int count;
    getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);

    int insertIdx;
    if (count == 0 && depth > 1) {
        // Empty directory: insert first child at index 0
        insertIdx = 0;
    } else if (count > 0) {
        insertIdx = appRenderer->currentId.ids[depth - 1];
    } else {
        return;
    }

    insertOperatorPlaceholder(appRenderer, insertIdx);
}

void handleCtrlAOperator(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate != COORDINATE_OPERATOR_GENERAL) return;

    int depth = appRenderer->currentId.depth;
    int count;
    getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);

    int insertIdx;
    if (count == 0 && depth > 1) {
        // Empty directory: append first child at index 0
        insertIdx = 0;
    } else if (count > 0) {
        insertIdx = appRenderer->currentId.ids[depth - 1] + 1;
    } else {
        return;
    }

    insertOperatorPlaceholder(appRenderer, insertIdx);
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
        } else if ((appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
                    appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) &&
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
        } else if (appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
            // Navigate into the selected search result (not stale currentId)
            ListItem *list = appRenderer->filteredListCount > 0 ?
                             appRenderer->filteredListCurrentLayer : appRenderer->totalListCurrentLayer;
            int count = appRenderer->filteredListCount > 0 ?
                        appRenderer->filteredListCount : appRenderer->totalListCount;
            if (appRenderer->listIndex >= 0 && appRenderer->listIndex < count) {
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
                } else {
                    idArrayCopy(&appRenderer->currentId, &list[appRenderer->listIndex].id);
                }
                if (providerNavigateRight(appRenderer)) {
                    createListExtendedSearch(appRenderer);
                    appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                    appRenderer->scrollOffset = appRenderer->listIndex;
                    accesskitSpeakCurrentElement(appRenderer);
                    appRenderer->needsRedraw = true;
                }
            }
        } else if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH) {
            // Navigate into the selected search result (not stale currentId)
            ListItem *list = appRenderer->filteredListCount > 0 ?
                             appRenderer->filteredListCurrentLayer : appRenderer->totalListCurrentLayer;
            int count = appRenderer->filteredListCount > 0 ?
                        appRenderer->filteredListCount : appRenderer->totalListCount;
            if (appRenderer->listIndex >= 0 && appRenderer->listIndex < count) {
                idArrayCopy(&appRenderer->currentId, &list[appRenderer->listIndex].id);
                if (providerNavigateRight(appRenderer)) {
                    createListCurrentLayer(appRenderer);
                    appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                    appRenderer->scrollOffset = appRenderer->listIndex;
                    accesskitSpeakCurrentElement(appRenderer);
                    appRenderer->needsRedraw = true;
                }
            }
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
        if (appRenderer->pendingFileBrowserSaveAs) {
            // Cancel file-browser save-as: remove placeholder and return to source provider
            int depth = appRenderer->currentId.depth;
            int idx = appRenderer->currentId.ids[depth - 1];
            if (depth >= 2) {
                IdArray parentId;
                idArrayCopy(&parentId, &appRenderer->currentId);
                idArrayPop(&parentId);
                int parentCount;
                FfonElement **parentArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                                      &parentId, &parentCount);
                int parentIdx = parentId.ids[parentId.depth - 1];
                if (parentArr && parentIdx >= 0 && parentIdx < parentCount &&
                    parentArr[parentIdx]->type == FFON_OBJECT) {
                    FfonObject *parentObj = parentArr[parentIdx]->data.object;
                    ffonElementDestroy(parentObj->elements[idx]);
                    memmove(&parentObj->elements[idx], &parentObj->elements[idx + 1],
                            (parentObj->count - idx - 1) * sizeof(FfonElement*));
                    parentObj->count--;
                }
            }
            idArrayCopy(&appRenderer->currentId, &appRenderer->saveAsReturnId);
            appRenderer->pendingFileBrowserSaveAs = false;
            appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
            appRenderer->previousCoordinate = COORDINATE_OPERATOR_GENERAL;
            accesskitSpeakModeChange(appRenderer, NULL);
            createListCurrentLayer(appRenderer);
            appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            appRenderer->scrollOffset = 0;
            appRenderer->needsRedraw = true;
            return;
        }
        if (appRenderer->prefixedInsertMode) {
            // Remove the empty placeholder element inserted by Ctrl+I/Ctrl+A
            int depth = appRenderer->currentId.depth;
            int idx = appRenderer->currentId.ids[depth - 1];

            if (depth == 1) {
                ffonElementDestroy(appRenderer->ffon[idx]);
                memmove(&appRenderer->ffon[idx], &appRenderer->ffon[idx + 1],
                        (appRenderer->ffonCount - idx - 1) * sizeof(FfonElement*));
                appRenderer->ffonCount--;
                if (appRenderer->ffonCount > 0 && idx >= appRenderer->ffonCount)
                    appRenderer->currentId.ids[0] = appRenderer->ffonCount - 1;
            } else {
                IdArray parentId;
                idArrayCopy(&parentId, &appRenderer->currentId);
                idArrayPop(&parentId);
                int parentCount;
                FfonElement **parentArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                                      &parentId, &parentCount);
                int parentIdx = parentId.ids[parentId.depth - 1];
                if (parentArr && parentIdx >= 0 && parentIdx < parentCount &&
                    parentArr[parentIdx]->type == FFON_OBJECT) {
                    FfonObject *parentObj = parentArr[parentIdx]->data.object;
                    ffonElementDestroy(parentObj->elements[idx]);
                    memmove(&parentObj->elements[idx], &parentObj->elements[idx + 1],
                            (parentObj->count - idx - 1) * sizeof(FfonElement*));
                    parentObj->count--;
                    if (parentObj->count > 0 && idx >= parentObj->count)
                        appRenderer->currentId.ids[depth - 1] = parentObj->count - 1;
                }
            }
            appRenderer->prefixedInsertMode = false;
            createListCurrentLayer(appRenderer);
            appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            appRenderer->scrollOffset = 0;
        }
        // Operator mode: Escape discards changes
        appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    } else if (appRenderer->currentCoordinate == COORDINATE_COMMAND) {
        // Cancel command mode (or open with / save as)
        appRenderer->pendingSaveAs = false;
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
    } else if (appRenderer->currentCoordinate == COORDINATE_DASHBOARD) {
        appRenderer->currentCoordinate = appRenderer->previousCoordinate;
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        accesskitSpeakModeChange(appRenderer, NULL);
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
    } else if (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL &&
               appRenderer->pendingFileBrowserOpen) {
        // Cancel file-browser open: return to source provider
        idArrayCopy(&appRenderer->currentId, &appRenderer->saveAsReturnId);
        appRenderer->pendingFileBrowserOpen = false;
        createListCurrentLayer(appRenderer);
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->scrollOffset = 0;
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

// --- File browser cut / copy / paste ---

void handleFileCut(AppRenderer *appRenderer) {
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                    &appRenderer->currentId, &count);
    if (!arr || count == 0) return;
    int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (idx < 0 || idx >= count) return;

    FfonElement *elem = arr[idx];
    const char *elementKey = (elem->type == FFON_STRING) ?
        elem->data.string : elem->data.object->key;

    char *name = providerTagExtractContent(elementKey);
    if (!name) return;

    const char *srcDir = providerGetCurrentPath(appRenderer);
    if (!srcDir) { free(name); return; }

    char cacheDir[MAX_URI_LENGTH];
    char *cacheBase = platformGetCacheHome();
    snprintf(cacheDir, sizeof(cacheDir), "%ssicompass/clipboard",
             cacheBase ? cacheBase : "/tmp/");
    free(cacheBase);
    platformMakeDirs(cacheDir);

    if (!providerCopyItem(appRenderer, srcDir, name, cacheDir, name)) {
        setErrorMessage(appRenderer, "Cut: failed to copy file to clipboard cache");
        free(name);
        return;
    }

    if (!providerDeleteItem(appRenderer, name)) {
        setErrorMessage(appRenderer, "Cut: failed to delete original file");
        free(name);
        return;
    }

    snprintf(appRenderer->fileClipboardPath, sizeof(appRenderer->fileClipboardPath),
             "%s/%s", cacheDir, name);
    appRenderer->fileClipboardIsCut = true;
    free(name);

    updateState(appRenderer, TASK_DELETE, HISTORY_NONE);
    appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    appRenderer->needsRedraw = true;
}

void handleFileCopy(AppRenderer *appRenderer) {
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                    &appRenderer->currentId, &count);
    if (!arr || count == 0) return;
    int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (idx < 0 || idx >= count) return;

    FfonElement *elem = arr[idx];
    const char *elementKey = (elem->type == FFON_STRING) ?
        elem->data.string : elem->data.object->key;

    char *name = providerTagExtractContent(elementKey);
    if (!name) return;

    const char *srcDir = providerGetCurrentPath(appRenderer);
    if (!srcDir) { free(name); return; }

    snprintf(appRenderer->fileClipboardPath, sizeof(appRenderer->fileClipboardPath),
             "%s/%s", srcDir, name);
    appRenderer->fileClipboardIsCut = false;
    free(name);

    appRenderer->needsRedraw = true;
}

void handleFilePaste(AppRenderer *appRenderer) {
    if (appRenderer->fileClipboardPath[0] == '\0') return;

    // Extract srcName (last path component) and srcDir (everything before it)
    const char *slash = strrchr(appRenderer->fileClipboardPath, '/');
    if (!slash) return;

    char srcDir[MAX_URI_LENGTH];
    int dirLen = (int)(slash - appRenderer->fileClipboardPath);
    strncpy(srcDir, appRenderer->fileClipboardPath, dirLen);
    srcDir[dirLen] = '\0';

    const char *srcName = slash + 1;
    if (srcName[0] == '\0') return;

    const char *destDir = providerGetCurrentPath(appRenderer);
    if (!destDir) return;

    // Resolve destination name — append " (copy N)" if name already exists
    char destName[MAX_URI_LENGTH];
    strncpy(destName, srcName, sizeof(destName) - 1);
    destName[sizeof(destName) - 1] = '\0';

    char destFull[MAX_URI_LENGTH];
    snprintf(destFull, sizeof(destFull), "%s/%s", destDir, destName);

    struct stat st;
    int copyNum = 0;
    while (stat(destFull, &st) == 0) {
        copyNum++;
        snprintf(destName, sizeof(destName), "%s (copy %d)", srcName, copyNum);
        snprintf(destFull, sizeof(destFull), "%s/%s", destDir, destName);
    }

    if (!providerCopyItem(appRenderer, srcDir, srcName, destDir, destName)) {
        setErrorMessage(appRenderer, "Paste: failed to copy file");
        return;
    }

    providerRefreshCurrentDirectory(appRenderer);
    createListCurrentLayer(appRenderer);

    // Move cursor to the pasted element
    int pastedIdx = -1;
    int listCount;
    FfonElement **listArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                        &appRenderer->currentId, &listCount);
    if (listArr) {
        for (int i = 0; i < listCount; i++) {
            const char *key = (listArr[i]->type == FFON_STRING)
                ? listArr[i]->data.string
                : listArr[i]->data.object->key;
            char *extracted = providerTagExtractContent(key);
            if (extracted && strcmp(extracted, destName) == 0) {
                pastedIdx = i;
                free(extracted);
                break;
            }
            free(extracted);
        }
    }
    if (pastedIdx >= 0) {
        appRenderer->currentId.ids[appRenderer->currentId.depth - 1] = pastedIdx;
        appRenderer->listIndex = pastedIdx;
    }
    appRenderer->needsRedraw = true;
}

void handleDashboard(AppRenderer *appRenderer) {
    Provider *provider = providerGetActive(appRenderer);
    if (!provider || !provider->dashboardImagePath) return;

    strncpy(appRenderer->dashboardImagePath, provider->dashboardImagePath,
            sizeof(appRenderer->dashboardImagePath) - 1);
    appRenderer->dashboardImagePath[sizeof(appRenderer->dashboardImagePath) - 1] = '\0';

    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    appRenderer->currentCoordinate = COORDINATE_DASHBOARD;
    accesskitSpeakModeChange(appRenderer, NULL);
    appRenderer->needsRedraw = true;
}

// Sanitize a provider name for use as a filename (spaces → underscores, strip unsafe chars)
static void sanitizeFilename(const char *name, char *out, size_t outSize) {
    size_t j = 0;
    for (size_t i = 0; name[i] && j < outSize - 1; i++) {
        char c = name[i];
        if (c == ' ') c = '_';
        else if (c == '/' || c == '\\' || c == ':' || c == '*' ||
                 c == '?' || c == '"' || c == '<' || c == '>' || c == '|')
            continue;
        out[j++] = c;
    }
    out[j] = '\0';
}

// Resolve the configured save folder to an absolute path.
// Returns a newly allocated string (caller must free), or NULL on failure.
static char* resolveSaveFolder(AppRenderer *appRenderer) {
    const char *folder = appRenderer->saveFolderPath;
    if (folder[0] == '\0') {
        // No setting configured, fall back to Downloads
        return platformGetDownloadsDir();
    }
    if (folder[0] == '/') {
        // Absolute path
        return strdup(folder);
    }
    // Relative to home directory
    const char *home = getenv("HOME");
    if (!home || home[0] == '\0') return NULL;
    size_t len = strlen(home) + 1 + strlen(folder) + 1;
    char *path = malloc(len);
    if (!path) return NULL;
    snprintf(path, len, "%s/%s", home, folder);
    return path;
}

// Build the default save path: <saveFolder>/<sanitized_provider_name>.json
// Returns true on success, false on failure (sets error message).
static bool buildProviderSavePath(AppRenderer *appRenderer, char *filepath, size_t size) {
    int rootIdx = appRenderer->currentId.ids[0];
    if (rootIdx < 0 || rootIdx >= appRenderer->ffonCount) {
        setErrorMessage(appRenderer, "No active provider");
        return false;
    }
    Provider *provider = appRenderer->providers[rootIdx];
    if (!provider) {
        setErrorMessage(appRenderer, "No active provider");
        return false;
    }
    char *saveDir = resolveSaveFolder(appRenderer);
    if (!saveDir) {
        setErrorMessage(appRenderer, "Cannot determine save folder");
        return false;
    }
    struct stat st;
    if (stat(saveDir, &st) != 0 || !S_ISDIR(st.st_mode)) {
        char msg[256];
        snprintf(msg, sizeof(msg), "Save folder does not exist: %s", saveDir);
        setErrorMessage(appRenderer, msg);
        free(saveDir);
        return false;
    }
    char safeName[256];
    sanitizeFilename(provider->name, safeName, sizeof(safeName));
    snprintf(filepath, size, "%s/%s.json", saveDir, safeName);
    free(saveDir);
    return true;
}

void handleSaveProviderConfig(AppRenderer *appRenderer) {
    if (appRenderer->currentSavePath[0] == '\0') {
        handleFileBrowserSaveAs(appRenderer);
        return;
    }
    char filepath[MAX_URI_LENGTH];
    if (!buildProviderSavePath(appRenderer, filepath, sizeof(filepath))) return;

    int rootIdx = appRenderer->currentId.ids[0];
    FfonElement *rootElem = appRenderer->ffon[rootIdx];
    if (!rootElem || rootElem->type != FFON_OBJECT) {
        setErrorMessage(appRenderer, "Nothing to save");
        return;
    }

    FfonObject *rootObj = rootElem->data.object;
    json_object *array = ffonElementsToJsonArray(rootObj->elements, rootObj->count);
    if (json_object_to_file_ext(filepath, array, JSON_C_TO_STRING_PRETTY) == 0) {
        snprintf(appRenderer->currentSavePath, sizeof(appRenderer->currentSavePath), "%s", filepath);
        char msg[256];
        snprintf(msg, sizeof(msg), "Saved to %s", filepath);
        setErrorMessage(appRenderer, msg);
    } else {
        setErrorMessage(appRenderer, "Failed to write file");
    }
    json_object_put(array);
    appRenderer->needsRedraw = true;
}

// Load a JSON config file into the provider at rootIdx. Returns true on success.
static bool loadProviderConfigFromFile(AppRenderer *appRenderer, const char *filepath, int rootIdx) {
    FfonElement *rootElem = appRenderer->ffon[rootIdx];
    if (!rootElem || rootElem->type != FFON_OBJECT) {
        setErrorMessage(appRenderer, "No provider to load into");
        return false;
    }

    int count = 0;
    FfonElement **newChildren = loadJsonFileToElements(filepath, &count);
    if (!newChildren || count == 0) {
        char msg[256];
        snprintf(msg, sizeof(msg), "No file found: %s", filepath);
        setErrorMessage(appRenderer, msg);
        return false;
    }

    // Replace children of root object
    FfonObject *rootObj = rootElem->data.object;
    for (int i = 0; i < rootObj->count; i++) {
        ffonElementDestroy(rootObj->elements[i]);
    }
    rootObj->count = 0;
    for (int i = 0; i < count; i++) {
        ffonObjectAddElement(rootObj, newChildren[i]);
    }
    free(newChildren);

    // Reset provider path
    Provider *provider = appRenderer->providers[rootIdx];
    if (provider && provider->setCurrentPath) {
        provider->setCurrentPath(provider, "/");
    }

    // Clear undo history
    for (int i = 0; i < appRenderer->undoHistoryCount; i++) {
        if (appRenderer->undoHistory[i].prevElement)
            ffonElementDestroy(appRenderer->undoHistory[i].prevElement);
        if (appRenderer->undoHistory[i].newElement)
            ffonElementDestroy(appRenderer->undoHistory[i].newElement);
    }
    appRenderer->undoHistoryCount = 0;
    appRenderer->undoPosition = 0;

    snprintf(appRenderer->currentSavePath, sizeof(appRenderer->currentSavePath), "%s", filepath);
    char msg[256];
    snprintf(msg, sizeof(msg), "Loaded from %s", filepath);
    setErrorMessage(appRenderer, msg);
    return true;
}

static void handleFileBrowserOpen(AppRenderer *appRenderer);

void handleLoadProviderConfig(AppRenderer *appRenderer) {
    handleFileBrowserOpen(appRenderer);
}

void handleSaveAsProviderConfig(AppRenderer *appRenderer) {
    // Enter command mode with pre-filled filename for "save as"
    Provider *provider = providerGetActive(appRenderer);
    if (!provider) {
        setErrorMessage(appRenderer, "No active provider");
        return;
    }

    appRenderer->pendingSaveAs = true;
    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    appRenderer->currentCoordinate = COORDINATE_COMMAND;
    appRenderer->currentCommand = COMMAND_NONE;
    accesskitSpeakModeChange(appRenderer, NULL);

    // Pre-fill input buffer with sanitized provider name
    char safeName[256];
    sanitizeFilename(provider->name, safeName, sizeof(safeName));
    int len = strlen(safeName);
    if (len >= appRenderer->inputBufferCapacity) len = appRenderer->inputBufferCapacity - 1;
    memcpy(appRenderer->inputBuffer, safeName, len);
    appRenderer->inputBuffer[len] = '\0';
    appRenderer->inputBufferSize = len;
    appRenderer->cursorPosition = len;
    appRenderer->selectionAnchor = -1;
    appRenderer->scrollOffset = 0;
    appRenderer->listIndex = 0;

    // Clear the list so only the text input is shown
    clearListCurrentLayer(appRenderer);
    appRenderer->needsRedraw = true;
}

static void handleFileBrowserSaveAs(AppRenderer *appRenderer) {
    // Save current navigation state to return to after save-as
    appRenderer->saveAsSourceRootIdx = appRenderer->currentId.ids[0];
    idArrayCopy(&appRenderer->saveAsReturnId, &appRenderer->currentId);

    // Find file browser provider index
    int fbIdx = -1;
    for (int i = 0; i < appRenderer->ffonCount; i++) {
        if (appRenderer->providers[i] &&
            strcmp(appRenderer->providers[i]->name, "filebrowser") == 0) {
            fbIdx = i;
            break;
        }
    }
    if (fbIdx < 0) {
        setErrorMessage(appRenderer, "File browser not available");
        return;
    }

    // Resolve save folder
    char *saveDir = resolveSaveFolder(appRenderer);
    if (!saveDir) {
        setErrorMessage(appRenderer, "Cannot determine save folder");
        return;
    }
    struct stat st;
    if (stat(saveDir, &st) != 0 || !S_ISDIR(st.st_mode)) {
        char msg[256];
        snprintf(msg, sizeof(msg), "Save folder does not exist: %s", saveDir);
        setErrorMessage(appRenderer, msg);
        free(saveDir);
        return;
    }

    // Navigate file browser to the save folder
    providerNavigateToPath(appRenderer, fbIdx, saveDir, "");
    free(saveDir);

    // Build the list for the save folder
    createListCurrentLayer(appRenderer);

    // Insert <input></input> placeholder at position 0 for filename entry
    int depth = appRenderer->currentId.depth;
    if (depth >= 2) {
        IdArray parentId;
        idArrayCopy(&parentId, &appRenderer->currentId);
        idArrayPop(&parentId);
        int parentCount;
        FfonElement **parentArr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                                               &parentId, &parentCount);
        int parentIdx = parentId.ids[parentId.depth - 1];
        if (parentArr && parentIdx >= 0 && parentIdx < parentCount &&
            parentArr[parentIdx]->type == FFON_OBJECT) {
            FfonElement *inputElem = ffonElementCreateString("<input></input>");
            ffonObjectInsertElement(parentArr[parentIdx]->data.object, inputElem, 0);
        }
    }

    // Point cursor at the new placeholder element
    appRenderer->currentId.ids[depth - 1] = 0;
    appRenderer->pendingFileBrowserSaveAs = true;
    appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    createListCurrentLayer(appRenderer);
    appRenderer->listIndex = 0;
    appRenderer->scrollOffset = 0;

    // Enter insert mode on the placeholder
    handleI(appRenderer);
}

static void handleFileBrowserOpen(AppRenderer *appRenderer) {
    // Save current navigation state to return to after open
    appRenderer->saveAsSourceRootIdx = appRenderer->currentId.ids[0];
    idArrayCopy(&appRenderer->saveAsReturnId, &appRenderer->currentId);

    // Find file browser provider index
    int fbIdx = -1;
    for (int i = 0; i < appRenderer->ffonCount; i++) {
        if (appRenderer->providers[i] &&
            strcmp(appRenderer->providers[i]->name, "filebrowser") == 0) {
            fbIdx = i;
            break;
        }
    }
    if (fbIdx < 0) {
        setErrorMessage(appRenderer, "File browser not available");
        return;
    }

    // Resolve save folder
    char *saveDir = resolveSaveFolder(appRenderer);
    if (!saveDir) {
        setErrorMessage(appRenderer, "Cannot determine save folder");
        return;
    }
    struct stat st;
    if (stat(saveDir, &st) != 0 || !S_ISDIR(st.st_mode)) {
        char msg[256];
        snprintf(msg, sizeof(msg), "Save folder does not exist: %s", saveDir);
        setErrorMessage(appRenderer, msg);
        free(saveDir);
        return;
    }

    // Navigate file browser to the save folder
    providerNavigateToPath(appRenderer, fbIdx, saveDir, "");
    free(saveDir);

    appRenderer->pendingFileBrowserOpen = true;
    createListCurrentLayer(appRenderer);
    appRenderer->listIndex = 0;
    appRenderer->scrollOffset = 0;
    appRenderer->needsRedraw = true;
}

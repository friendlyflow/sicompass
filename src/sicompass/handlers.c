#include "view.h"
#include "provider.h"
#include <filebrowser.h>
#include <platform.h>
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

void handleTab(AppRenderer *appRenderer) {
    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    appRenderer->currentCoordinate = COORDINATE_SIMPLE_SEARCH;
    accesskitSpeakModeChange(appRenderer, NULL);

    // Clear input buffer for searching
    appRenderer->inputBuffer[0] = '\0';
    appRenderer->inputBufferSize = 0;
    appRenderer->cursorPosition = 0;

    createListCurrentLayer(appRenderer);
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

void handleEnter(AppRenderer *appRenderer, History history) {
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
                char *oldContent = providerGetEditableContent(elementKey);
                if (oldContent) {
                    const char *newContent = appRenderer->inputBuffer;
                    // Only commit if changed
                    if (strcmp(oldContent, newContent) != 0) {
                        if (providerCommitEdit(elementKey, oldContent, newContent)) {
                            // Update element with new key
                            char *newKey = providerFormatUpdatedKey(elementKey, newContent);
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
        // Get current element to check if it's a string (file) or object (directory)
        int count;
        FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
        if (arr && count > 0) {
            int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                FfonElement *elem = arr[idx];
                if (elem->type == FFON_STRING) {
                    // Open file with default program
                    char *filename = providerGetEditableContent(elem->data.string);
                    const char *path = providerGetCurrentPath(elem->data.string);
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
            idArrayCopy(&appRenderer->currentId, &list[appRenderer->listIndex].id);
        }
        appRenderer->currentCoordinate = appRenderer->previousCoordinate;
        accesskitSpeakModeChange(appRenderer, NULL);
        createListCurrentLayer(appRenderer);
        // Sync listIndex with current position (after createListCurrentLayer which resets it)
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        appRenderer->needsRedraw = true;
    } else if (appRenderer->currentCoordinate == COORDINATE_COMMAND) {
        // Execute selected command
        ListItem *list = appRenderer->filteredListCount > 0 ?
                         appRenderer->filteredListCurrentLayer : appRenderer->totalListCurrentLayer;
        int count = appRenderer->filteredListCount > 0 ?
                    appRenderer->filteredListCount : appRenderer->totalListCount;

        if (appRenderer->listIndex >= 0 && appRenderer->listIndex < count) {
            const char *cmd = list[appRenderer->listIndex].value;
            if (strcmp(cmd, "editor mode") == 0) {
                appRenderer->currentCommand = COMMAND_EDITOR_MODE;
            } else if (strcmp(cmd, "operator mode") == 0) {
                appRenderer->currentCommand = COMMAND_OPERATOR_MODE;
            }
            handleCommand(appRenderer);
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
    accesskitSpeakModeChange(appRenderer, NULL);

    // Clear input buffer for searching
    appRenderer->inputBuffer[0] = '\0';
    appRenderer->inputBufferSize = 0;
    appRenderer->cursorPosition = 0;

    createListCurrentLayer(appRenderer);
    appRenderer->needsRedraw = true;
}

void handleUp(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        if (appRenderer->listIndex > 0) {
            appRenderer->listIndex--;
            accesskitSpeakCurrentItem(appRenderer);
        }
    } else if (appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT &&
               appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT) {
        updateState(appRenderer, TASK_K_ARROW_UP, HISTORY_NONE);
        // Sync listIndex with current position in hierarchy
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        accesskitSpeakCurrentItem(appRenderer);
    }
    appRenderer->needsRedraw = true;
}

void handleDown(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        int maxIndex = (appRenderer->filteredListCount > 0) ?
                        appRenderer->filteredListCount - 1 :
                        appRenderer->totalListCount - 1;
        if (appRenderer->listIndex < maxIndex) {
            appRenderer->listIndex++;
            accesskitSpeakCurrentItem(appRenderer);
        }
    } else if (appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT &&
               appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT) {
        updateState(appRenderer, TASK_J_ARROW_DOWN, HISTORY_NONE);
        // Sync listIndex with current position in hierarchy
        appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        accesskitSpeakCurrentItem(appRenderer);
    }
    appRenderer->needsRedraw = true;
}

void handleLeft(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        if (appRenderer->cursorPosition > 0) {
            // Move backward by one UTF-8 character
            appRenderer->cursorPosition = utf8_move_backward(
                appRenderer->inputBuffer,
                appRenderer->cursorPosition
            );

            // Reset caret to visible when user presses left arrow
            uint64_t currentTime = SDL_GetTicks();
            caretReset(appRenderer->caretState, currentTime);

            accesskitSpeakCurrentItem(appRenderer);

            appRenderer->needsRedraw = true;
        }
    } else {
        // Use provider for navigation
        if (providerNavigateLeft(appRenderer)) {
            // Rebuild list for new location
            createListCurrentLayer(appRenderer);
            // Sync listIndex with current position in hierarchy
            appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            accesskitSpeakCurrentItem(appRenderer);
            appRenderer->needsRedraw = true;
        }
    }
}

void handleRight(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
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

            accesskitSpeakCurrentItem(appRenderer);

            appRenderer->needsRedraw = true;
        }
    } else {
        // Use provider for navigation (fetches children dynamically)
        if (providerNavigateRight(appRenderer)) {
            // Rebuild list for new location
            createListCurrentLayer(appRenderer);
            // When entering a child, start at the first item
            appRenderer->listIndex = 0;
            accesskitSpeakCurrentItem(appRenderer);
            appRenderer->needsRedraw = true;
        }
    }
}

void handleI(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {

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
                char *content = providerGetEditableContent(elementKey);
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
        idArrayInit(&appRenderer->currentInsertId);
        appRenderer->needsRedraw = true;
    }
}

void handleA(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {

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
                char *content = providerGetEditableContent(elementKey);
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
        idArrayInit(&appRenderer->currentInsertId);
        appRenderer->needsRedraw = true;
    }
}

void handleFind(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate != COORDINATE_SIMPLE_SEARCH &&
        appRenderer->currentCoordinate != COORDINATE_COMMAND) {
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        appRenderer->currentCoordinate = COORDINATE_EXTENDED_SEARCH;
        accesskitSpeakModeChange(appRenderer, NULL);

        // Clear input buffer for searching
        appRenderer->inputBuffer[0] = '\0';
        appRenderer->inputBufferSize = 0;
        appRenderer->cursorPosition = 0;

        appRenderer->needsRedraw = true;
    }
}

void handleEscape(AppRenderer *appRenderer) {
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
                char *oldContent = providerGetEditableContent(elementKey);
                if (oldContent) {
                    const char *newContent = appRenderer->inputBuffer;
                    if (strcmp(oldContent, newContent) != 0) {
                        if (providerCommitEdit(elementKey, oldContent, newContent)) {
                            char *newKey = providerFormatUpdatedKey(elementKey, newContent);
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
    }

    appRenderer->needsRedraw = true;
}

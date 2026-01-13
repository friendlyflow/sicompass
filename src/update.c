#include "view.h"
#include <string.h>
#include <stdlib.h>

// Helper function to strip trailing colon from a key
static char* stripTrailingColon(const char *line) {
    if (!line) return strdup("");

    size_t len = strlen(line);
    if (len > 0 && line[len - 1] == ':') {
        // Create a copy without the trailing colon
        char *result = malloc(len);
        if (result) {
            strncpy(result, line, len - 1);
            result[len - 1] = '\0';
        }
        return result;
    }
    return strdup(line);
}

void updateState(AppRenderer *appRenderer, Task task, History history) {
    // Get current line
    char line[MAX_LINE_LENGTH] = "";

    if (history == HISTORY_UNDO || history == HISTORY_REDO) {
        if (appRenderer->undoPosition < appRenderer->undoHistoryCount) {
            strncpy(line, appRenderer->undoHistory[appRenderer->undoHistoryCount - appRenderer->undoPosition].line,
                   MAX_LINE_LENGTH - 1);
        }
    } else {
        // Get line from current element or input buffer
        if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
            appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT) {
            strncpy(line, appRenderer->inputBuffer, MAX_LINE_LENGTH - 1);
        } else {
            int count;
            FfonElement **arr = getFfonAtId(appRenderer, &appRenderer->currentId, &count);
            if (arr && count > 0) {
                int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                if (idx >= 0 && idx < count) {
                    const FfonElement *elem = arr[idx];
                    if (elem->type == FFON_STRING) {
                        strncpy(line, elem->data.string, MAX_LINE_LENGTH - 1);
                    } else {
                        strncpy(line, elem->data.object->key, MAX_LINE_LENGTH - 1);
                    }
                }
            }
        }
    }

    bool isKey = isLineKey(line);
    updateIds(appRenderer, isKey, task, history);
    updateFfon(appRenderer, line, isKey, task, history);
    updateHistory(appRenderer, task, isKey, line, history);
}

void updateIds(AppRenderer *appRenderer, bool isKey, Task task, History history) {
    idArrayCopy(&appRenderer->previousId, &appRenderer->currentId);

    if (history == HISTORY_UNDO || history == HISTORY_REDO) {
        return;
    }

    int maxId = getMaxIdInCurrent(appRenderer);
    int currentIdx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];

    switch (task) {
        case TASK_K_ARROW_UP:
            if (currentIdx > 0) {
                appRenderer->currentId.ids[appRenderer->currentId.depth - 1]--;
            }
            break;

        case TASK_J_ARROW_DOWN:
            if (currentIdx < maxId) {
                appRenderer->currentId.ids[appRenderer->currentId.depth - 1]++;
            }
            break;

        case TASK_H_ARROW_LEFT:
            if (appRenderer->currentId.depth > 1) {
                idArrayPop(&appRenderer->currentId);
            }
            break;

        case TASK_L_ARROW_RIGHT:
            if (nextLayerExists(appRenderer)) {
                idArrayPush(&appRenderer->currentId, 0);
            }
            break;

        case TASK_APPEND:
            if (appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
                appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {
                if (!isKey) {
                    appRenderer->currentId.ids[appRenderer->currentId.depth - 1]++;
                } else {
                    if (nextLayerExists(appRenderer)) {
                        appRenderer->currentId.ids[appRenderer->currentId.depth - 1]++;
                    } else {
                        idArrayPush(&appRenderer->currentId, 0);
                    }
                }
            }
            break;

        case TASK_APPEND_APPEND:
            if (appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
                appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {
                appRenderer->currentId.ids[appRenderer->currentId.depth - 1] = maxId + 1;
            }
            break;

        case TASK_INSERT:
            // Position stays the same
            break;

        case TASK_INSERT_INSERT:
            if (appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
                appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {
                appRenderer->currentId.ids[appRenderer->currentId.depth - 1] = 0;
            }
            break;

        case TASK_DELETE:
            // Position handled in updateFfon
            break;

        case TASK_INPUT:
            // Position stays the same
            break;

        default:
            break;
    }
}

void updateFfon(AppRenderer *appRenderer, const char *line, bool isKey, Task task, History history) {
    if (appRenderer->currentId.depth == 0) return;

    // Navigate to parent array
    int count;
    FfonElement **arr = getFfonAtId(appRenderer, &appRenderer->currentId, &count);
    if (!arr) return;

    int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];

    // Get parent object if we're nested
    FfonObject *parentObj = NULL;
    if (appRenderer->currentId.depth > 1) {
        FfonElement **parentArr = getFfonAtId(appRenderer, &appRenderer->currentId, &count);
        if (parentArr) {
            int parentIdx = appRenderer->currentId.ids[appRenderer->currentId.depth - 2];
            if (parentIdx >= 0 && parentIdx < count) {
                FfonElement *parentElem = parentArr[parentIdx];
                if (parentElem && parentElem->type == FFON_OBJECT) {
                    parentObj = parentElem->data.object;
                }
            }
        }
    }

    switch (task) {
        case TASK_APPEND:
        case TASK_APPEND_APPEND:
        case TASK_INSERT:
        case TASK_INSERT_INSERT: {
            if (isKey) {
                // Convert to object or update key
                if (idx >= 0 && idx < count && arr[idx]->type == FFON_OBJECT) {
                    // Update key (strip trailing colon)
                    free(arr[idx]->data.object->key);
                    arr[idx]->data.object->key = stripTrailingColon(line);
                } else {
                    // Convert string to object (strip trailing colon)
                    char *keyWithoutColon = stripTrailingColon(line);
                    FfonElement *newElem = ffonElementCreateObject(keyWithoutColon);
                    free(keyWithoutColon);
                    if (newElem) {
                        ffonObjectAddElement(newElem->data.object,
                                               ffonElementCreateString(""));

                        if (parentObj) {
                            // Insert in parent object
                            if (history != HISTORY_REDO) {
                                ffonObjectAddElement(parentObj, ffonElementCreateString(""));
                            }
                            if (idx >= 0 && idx < parentObj->count) {
                                ffonElementDestroy(parentObj->elements[idx]);
                                parentObj->elements[idx] = newElem;
                            }
                        }
                    }
                }
            } else {
                // Update or insert string element
                if (parentObj) {
                    if (idx >= 0 && idx < parentObj->count) {
                        ffonElementDestroy(parentObj->elements[idx]);
                        parentObj->elements[idx] = ffonElementCreateString(line);
                    }
                    if (history != HISTORY_REDO) {
                        ffonObjectAddElement(parentObj, ffonElementCreateString(""));
                    }
                }
            }
            break;
        }

        case TASK_DELETE: {
            if (parentObj && idx >= 0 && idx < parentObj->count) {
                // Remove element
                ffonElementDestroy(parentObj->elements[idx]);

                // Shift elements down
                for (int i = idx; i < parentObj->count - 1; i++) {
                    parentObj->elements[i] = parentObj->elements[i + 1];
                }
                parentObj->count--;

                // Adjust currentId
                if (appRenderer->currentId.ids[appRenderer->currentId.depth - 1] > 0) {
                    appRenderer->currentId.ids[appRenderer->currentId.depth - 1]--;
                }

                // If empty, add one empty element
                if (parentObj->count == 0) {
                    ffonObjectAddElement(parentObj, ffonElementCreateString(""));
                }
            }
            break;
        }

        case TASK_INPUT:
        case TASK_H_ARROW_LEFT:
        case TASK_L_ARROW_RIGHT:
        case TASK_K_ARROW_UP:
        case TASK_J_ARROW_DOWN: {
            // For navigation, save current content at the PREVIOUS position (before the move)
            int prevCount;
            FfonElement **prevArr = getFfonAtId(appRenderer, &appRenderer->previousId, &prevCount);
            if (prevArr && prevCount > 0) {
                int prevIdx = appRenderer->previousId.ids[appRenderer->previousId.depth - 1];
                if (prevIdx >= 0 && prevIdx < prevCount) {
                    if (prevArr[prevIdx]->type == FFON_STRING) {
                        free(prevArr[prevIdx]->data.string);
                        prevArr[prevIdx]->data.string = strdup(line);
                    } else if (prevArr[prevIdx]->type == FFON_OBJECT) {
                        // Strip trailing colon when saving object key
                        free(prevArr[prevIdx]->data.object->key);
                        prevArr[prevIdx]->data.object->key = stripTrailingColon(line);
                    }
                }
            }
            break;
        }

        default:
            break;
    }
}

void updateHistory(AppRenderer *appRenderer, Task task, bool isKey, const char *line, History history) {
    if (history != HISTORY_NONE) return;

    if (task == TASK_APPEND || task == TASK_APPEND_APPEND ||
        task == TASK_INSERT || task == TASK_INSERT_INSERT ||
        task == TASK_DELETE || task == TASK_INPUT) {

        if (appRenderer->undoHistoryCount >= UNDO_HISTORY_SIZE) {
            // Remove oldest entry
            free(appRenderer->undoHistory[0].line);
            memmove(&appRenderer->undoHistory[0], &appRenderer->undoHistory[1],
                   sizeof(UndoEntry) * (UNDO_HISTORY_SIZE - 1));
            appRenderer->undoHistoryCount--;
        }

        UndoEntry *entry = &appRenderer->undoHistory[appRenderer->undoHistoryCount++];
        idArrayCopy(&entry->id, &appRenderer->currentId);
        entry->task = task;
        entry->isKey = isKey;
        entry->line = strdup(line ? line : "");

        appRenderer->undoPosition = 0;
    }
}

void handleHistoryAction(AppRenderer *appRenderer, History history) {
    if (appRenderer->undoHistoryCount == 0) {
        setErrorMessage(appRenderer, "No undo history");
        return;
    }

    if (history == HISTORY_UNDO) {
        // Save current state before undo
        int count;
        FfonElement **arr = getFfonAtId(appRenderer, &appRenderer->currentId, &count);
        if (arr && count > 0) {
            int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                FfonElement *elem = arr[idx];
                char line[MAX_LINE_LENGTH] = "";
                if (elem->type == FFON_STRING) {
                    strncpy(line, elem->data.string, MAX_LINE_LENGTH - 1);
                } else {
                    strncpy(line, elem->data.object->key, MAX_LINE_LENGTH - 1);
                }

                bool isKey = isLineKey(line);
                updateIds(appRenderer, isKey, TASK_NONE, HISTORY_NONE);
                updateFfon(appRenderer, line, isKey, TASK_NONE, HISTORY_NONE);
            }
        }

        if (appRenderer->undoPosition < appRenderer->undoHistoryCount) {
            appRenderer->undoPosition++;
        }

        UndoEntry *entry = &appRenderer->undoHistory[appRenderer->undoHistoryCount - appRenderer->undoPosition];
        idArrayCopy(&appRenderer->currentId, &entry->id);

        // Reverse the operation
        switch (entry->task) {
            case TASK_APPEND:
            case TASK_APPEND_APPEND:
            case TASK_INSERT:
            case TASK_INSERT_INSERT:
                handleDelete(appRenderer, history);
                break;

            case TASK_DELETE:
                if (appRenderer->currentId.ids[appRenderer->currentId.depth - 1] == 0) {
                    handleCtrlI(appRenderer, history);
                } else {
                    handleCtrlA(appRenderer, history);
                }
                break;

            default:
                break;
        }
    } else if (history == HISTORY_REDO) {
        if (appRenderer->undoPosition > 0) {
            UndoEntry *entry = &appRenderer->undoHistory[appRenderer->undoHistoryCount - appRenderer->undoPosition];
            idArrayCopy(&appRenderer->currentId, &entry->id);

            // Redo the operation
            switch (entry->task) {
                case TASK_APPEND:
                case TASK_APPEND_APPEND:
                    handleCtrlA(appRenderer, history);
                    break;

                case TASK_INSERT:
                case TASK_INSERT_INSERT:
                    handleCtrlI(appRenderer, history);
                    break;

                case TASK_DELETE:
                    handleDelete(appRenderer, history);
                    break;

                default:
                    break;
            }

            appRenderer->undoPosition--;
        }
    }

    appRenderer->needsRedraw = true;
}

void handleCcp(AppRenderer *appRenderer, Task task) {
    int count;
    FfonElement **arr = getFfonAtId(appRenderer, &appRenderer->currentId, &count);
    if (!arr || count == 0) return;

    int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (idx < 0 || idx >= count) return;

    if (task == TASK_PASTE) {
        if (appRenderer->clipboard) {
            // Insert clipboard content
            FfonElement *newElem = ffonElementClone(appRenderer->clipboard);
            if (newElem) {
                // Add to parent
                FfonObject *parentObj = NULL;
                if (appRenderer->currentId.depth > 1) {
                    int parentCount;
                    FfonElement **parentArr = getFfonAtId(appRenderer, &appRenderer->currentId, &parentCount);
                    if (parentArr) {
                        int parentIdx = appRenderer->currentId.ids[appRenderer->currentId.depth - 2];
                        if (parentIdx >= 0 && parentIdx < parentCount &&
                            parentArr[parentIdx]->type == FFON_OBJECT) {
                            parentObj = parentArr[parentIdx]->data.object;
                        }
                    }
                }

                if (parentObj) {
                    ffonObjectAddElement(parentObj, newElem);
                    updateHistory(appRenderer, TASK_PASTE, false, "", HISTORY_NONE);
                }
            }
        }
    } else {
        // Copy or cut
        FfonElement *elem = arr[idx];

        if (appRenderer->clipboard) {
            ffonElementDestroy(appRenderer->clipboard);
        }

        if (elem->type == FFON_OBJECT && !nextLayerExists(appRenderer)) {
            // Copy the object's contents
            appRenderer->clipboard = ffonElementClone(elem);
        } else {
            appRenderer->clipboard = ffonElementClone(elem);
        }

        if (task == TASK_CUT) {
            handleDelete(appRenderer, HISTORY_NONE);
            updateHistory(appRenderer, TASK_CUT, false, "", HISTORY_NONE);
        }
    }

    appRenderer->needsRedraw = true;
}

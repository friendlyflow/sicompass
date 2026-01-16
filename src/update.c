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
    printf("update state, task=%d, previous_id=", task);
    for (int i = 0; i < appRenderer->previousId.depth; i++) printf("%d ", appRenderer->previousId.ids[i]);
    printf(", current_id=");
    for (int i = 0; i < appRenderer->currentId.depth; i++) printf("%d ", appRenderer->currentId.ids[i]);
    printf("\n");

    // Get current line
    char line[MAX_LINE_LENGTH] = "";
    bool currentElemIsObject = false;

    if (history == HISTORY_UNDO || history == HISTORY_REDO) {
        if (appRenderer->undoPosition > 0 && appRenderer->undoPosition <= appRenderer->undoHistoryCount) {
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
                        currentElemIsObject = true;
                    }
                }
            }
        }
    }

    printf("update state line: '%s'\n", line);

    bool isKey = isLineKey(line) || currentElemIsObject;
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

// Helper function to navigate to a specific level in the tree
static void navigateToLevel(AppRenderer *appRenderer, int depth,
                            FfonElement ***out_ffon, int *out_count, FfonObject **out_parent) {
    FfonElement **ffon = appRenderer->ffon;
    int count = appRenderer->ffonCount;
    FfonObject *parent = NULL;

    for (int i = 0; i < depth; i++) {
        int idx = appRenderer->previousId.ids[i];
        if (idx < 0 || idx >= count || ffon[idx]->type != FFON_OBJECT) {
            *out_ffon = ffon;
            *out_count = count;
            *out_parent = parent;
            return;
        }

        parent = ffon[idx]->data.object;
        ffon = parent->elements;
        count = parent->count;
    }

    *out_ffon = ffon;
    *out_count = count;
    *out_parent = parent;
}

void updateFfon(AppRenderer *appRenderer, const char *line, bool isKey, Task task, History history) {
    printf("update ffon struct, line='%s', previous_id=", line);
    for (int i = 0; i < appRenderer->previousId.depth; i++) printf("%d ", appRenderer->previousId.ids[i]);
    printf(", current_id=");
    for (int i = 0; i < appRenderer->currentId.depth; i++) printf("%d ", appRenderer->currentId.ids[i]);
    printf("\n");
    printf("update, isKey=%d, task=%d\n", isKey, task);

    // Handle special case: empty root structure
    if (appRenderer->ffonCount == 0 && appRenderer->previousId.depth == 1) {
        if (task == TASK_APPEND || task == TASK_APPEND_APPEND ||
            task == TASK_INSERT || task == TASK_INSERT_INSERT ||
            task == TASK_INPUT) {
            // Create a new root element
            FfonElement *newElem;
            if (isKey) {
                char *strippedKey = stripTrailingColon(line);
                newElem = ffonElementCreateObject(strippedKey);
                free(strippedKey);
                ffonObjectAddElement(newElem->data.object, ffonElementCreateString(""));
            } else {
                newElem = ffonElementCreateString(line);
            }

            // Add to root
            appRenderer->ffon = realloc(appRenderer->ffon, sizeof(FfonElement *));
            if (appRenderer->ffon) {
                appRenderer->ffon[0] = newElem;
                appRenderer->ffonCount = 1;
            }
            return;
        }
    }

    for (int i = 0; i < appRenderer->previousId.depth; i++) {
        // Re-navigate to current level on each iteration to avoid stale pointers
        FfonElement **_ffon;
        int _ffon_count;
        FfonObject *_parentObj;
        navigateToLevel(appRenderer, i, &_ffon, &_ffon_count, &_parentObj);
        printf("beestje, _ffon_count=%d, i=%d, depth=%d\n", _ffon_count, i, appRenderer->previousId.depth);

        // Sanity check: if _ffon_count is unreasonably large, we have memory corruption
        if (_ffon_count < 0 || _ffon_count > 1000) {
            printf("ERROR: Invalid _ffon_count=%d, _ffon=%p, _parentObj=%p\n",
                   _ffon_count, (void*)_ffon, (void*)_parentObj);
            break;
        }

        bool isEditorCoordinate = (appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
                                   appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
                                   appRenderer->currentCoordinate == COORDINATE_EDITOR_NORMAL ||
                                   appRenderer->currentCoordinate == COORDINATE_EDITOR_VISUAL);

        if (isKey && isEditorCoordinate) {
            printf("beestje1\n");

            if (task == TASK_DELETE && i == appRenderer->currentId.depth - 1) {
                printf("beestje10, previous_id=");
                for (int j = 0; j < appRenderer->previousId.depth; j++) printf("%d ", appRenderer->previousId.ids[j]);
                printf(", current_id=");
                for (int j = 0; j < appRenderer->currentId.depth; j++) printf("%d ", appRenderer->currentId.ids[j]);
                printf("\n");

                // Remove element at previous_id[i]
                int removeIdx = appRenderer->previousId.ids[i];
                if (removeIdx >= 0 && removeIdx < _ffon_count) {
                    int originalCount = _ffon_count;
                    ffonElementDestroy(_ffon[removeIdx]);
                    for (int j = removeIdx; j < _ffon_count - 1; j++) {
                        _ffon[j] = _ffon[j + 1];
                    }
                    _ffon_count--;

                    // Update the parent object's count to match
                    if (_parentObj) {
                        _parentObj->count = _ffon_count;
                    } else if (i == 0) {
                        // Update root level count
                        appRenderer->ffonCount = _ffon_count;
                    }

                    // Only insert empty string if this was the only element and not at root
                    if (i != 0 && _parentObj && originalCount == 1) {
                        ffonObjectInsertElement(_parentObj, ffonElementCreateString(""), 0);
                    } else if (_ffon_count > 0) {
                        // Move cursor to previous element if it exists, otherwise stay at same index (which is now next element)
                        if (removeIdx > 0) {
                            appRenderer->currentId.ids[i]--;
                        }
                        // else: removeIdx == 0, so currentId already points to what was the next element
                    }
                }
                break;  // Don't continue to check other branches after delete
            }

            int prevIdx = appRenderer->previousId.ids[i];
            if (prevIdx >= 0 && prevIdx < _ffon_count && _ffon[prevIdx]->type == FFON_OBJECT) {
                printf("beestje11, previous_id=");
                for (int j = 0; j < appRenderer->previousId.depth; j++) printf("%d ", appRenderer->previousId.ids[j]);
                printf(", current_id=");
                for (int j = 0; j < appRenderer->currentId.depth; j++) printf("%d ", appRenderer->currentId.ids[j]);
                printf(", i=%d\n", i);

                if (i < appRenderer->previousId.depth - 1) {
                    printf("beestje111\n");
                    // Navigation is now handled by navigateToLevel at the start of each iteration
                    // Just continue to next iteration
                    continue;
                } else {
                    printf("beestje112\n");
                    if (task == TASK_APPEND || task == TASK_APPEND_APPEND ||
                        task == TASK_INSERT || task == TASK_INSERT_INSERT) {
                        printf("beestje1121, previous_id=");
                        for (int j = 0; j < appRenderer->previousId.depth; j++) printf("%d ", appRenderer->previousId.ids[j]);
                        printf(", current_id=");
                        for (int j = 0; j < appRenderer->currentId.depth; j++) printf("%d ", appRenderer->currentId.ids[j]);
                        printf("\n");

                        // Get the old object's children
                        FfonElement **oldChildren = _ffon[prevIdx]->data.object->elements;
                        int oldCount = _ffon[prevIdx]->data.object->count;

                        // Create new object with new key
                        char *strippedKey = stripTrailingColon(line);
                        FfonElement *newElem = ffonElementCreateObject(strippedKey);
                        free(strippedKey);

                        // Transfer children to new object
                        for (int j = 0; j < oldCount; j++) {
                            ffonObjectAddElement(newElem->data.object, oldChildren[j]);
                        }

                        // Free old object structure (but not children, we transferred them)
                        _ffon[prevIdx]->data.object->count = 0; // Don't destroy children
                        ffonElementDestroy(_ffon[prevIdx]);
                        _ffon[prevIdx] = newElem;

                        // Insert new sibling element
                        if (_parentObj && history != HISTORY_REDO) {
                            int insertIdx = appRenderer->currentId.ids[i];
                            ffonObjectInsertElement(_parentObj, ffonElementCreateString(""), insertIdx);
                        } else if (!_parentObj && i == 0 && history != HISTORY_REDO) {
                            // At root level, insert a new empty string sibling
                            int insertIdx = appRenderer->currentId.ids[i];

                            // Expand root array
                            FfonElement **newArray = realloc(appRenderer->ffon,
                                                            sizeof(FfonElement *) * (appRenderer->ffonCount + 1));
                            if (newArray) {
                                appRenderer->ffon = newArray;

                                // Shift elements to make room
                                for (int k = appRenderer->ffonCount; k > insertIdx; k--) {
                                    appRenderer->ffon[k] = appRenderer->ffon[k - 1];
                                }

                                // Insert new empty string
                                appRenderer->ffon[insertIdx] = ffonElementCreateString("");
                                appRenderer->ffonCount++;
                            }
                        }
                    } else if (task == TASK_H_ARROW_LEFT || task == TASK_L_ARROW_RIGHT ||
                               task == TASK_K_ARROW_UP || task == TASK_J_ARROW_DOWN ||
                               task == TASK_INPUT) {
                        printf("beestje1123\n");

                        // Get the old object's children
                        FfonElement **oldChildren = _ffon[prevIdx]->data.object->elements;
                        int oldCount = _ffon[prevIdx]->data.object->count;

                        // Create new object with new key
                        char *strippedKey = stripTrailingColon(line);
                        FfonElement *newElem = ffonElementCreateObject(strippedKey);
                        free(strippedKey);

                        // Transfer children to new object
                        for (int j = 0; j < oldCount; j++) {
                            ffonObjectAddElement(newElem->data.object, oldChildren[j]);
                        }

                        // Free old object structure (but not children)
                        _ffon[prevIdx]->data.object->count = 0;
                        ffonElementDestroy(_ffon[prevIdx]);
                        _ffon[prevIdx] = newElem;

                        // If we previously navigated into this object, update our pointers
                        if (_parentObj && _parentObj == _ffon[prevIdx]->data.object) {
                            _ffon = _parentObj->elements;
                            _ffon_count = _parentObj->count;
                        }

                        break;
                    }
                }
            } else {
                printf("beestje12, previous_id=");
                for (int j = 0; j < appRenderer->previousId.depth; j++) printf("%d ", appRenderer->previousId.ids[j]);
                printf(", current_id=");
                for (int j = 0; j < appRenderer->currentId.depth; j++) printf("%d ", appRenderer->currentId.ids[j]);
                printf("\n");

                if (task == TASK_APPEND || task == TASK_APPEND_APPEND ||
                    task == TASK_INSERT || task == TASK_INSERT_INSERT) {
                    printf("beestje121, previous_id=");
                    for (int j = 0; j < appRenderer->previousId.depth; j++) printf("%d ", appRenderer->previousId.ids[j]);
                    printf(", current_id=");
                    for (int j = 0; j < appRenderer->currentId.depth; j++) printf("%d ", appRenderer->currentId.ids[j]);
                    printf("\n");

                    // Create object with line as key and an empty string as its child
                    char *strippedKey = stripTrailingColon(line);
                    FfonElement *newElem = ffonElementCreateObject(strippedKey);
                    free(strippedKey);
                    ffonObjectAddElement(newElem->data.object, ffonElementCreateString(""));

                    if (prevIdx >= 0 && prevIdx < _ffon_count) {
                        ffonElementDestroy(_ffon[prevIdx]);
                        _ffon[prevIdx] = newElem;
                    }

                    // Insert a new sibling at root level
                    if (!_parentObj && i == 0 && history != HISTORY_REDO) {
                        int insertIdx = appRenderer->currentId.ids[i];

                        // Expand root array
                        FfonElement **newArray = realloc(appRenderer->ffon,
                                                        sizeof(FfonElement *) * (appRenderer->ffonCount + 1));
                        if (newArray) {
                            appRenderer->ffon = newArray;

                            // Shift elements to make room
                            for (int k = appRenderer->ffonCount; k > insertIdx; k--) {
                                appRenderer->ffon[k] = appRenderer->ffon[k - 1];
                            }

                            // Insert new empty string
                            appRenderer->ffon[insertIdx] = ffonElementCreateString("");
                            appRenderer->ffonCount++;
                        }
                    }
                } else if (task == TASK_DELETE) {
                    printf("beestje122, previous_id=");
                    for (int j = 0; j < appRenderer->previousId.depth; j++) printf("%d ", appRenderer->previousId.ids[j]);
                    printf(", current_id=");
                    for (int j = 0; j < appRenderer->currentId.depth; j++) printf("%d ", appRenderer->currentId.ids[j]);
                    printf("\n");

                    if (appRenderer->currentId.ids[appRenderer->currentId.depth - 1] > 0) {
                        int removeIdx = appRenderer->previousId.ids[i];
                        if (removeIdx >= 0 && removeIdx < _ffon_count) {
                            ffonElementDestroy(_ffon[removeIdx]);
                            for (int j = removeIdx; j < _ffon_count - 1; j++) {
                                _ffon[j] = _ffon[j + 1];
                            }
                            _ffon_count--;

                            // Update the parent object's count to match
                            if (_parentObj) {
                                _parentObj->count = _ffon_count;
                            } else if (i == 0) {
                                // Update root level count
                                appRenderer->ffonCount = _ffon_count;
                            }

                            appRenderer->currentId.ids[appRenderer->currentId.depth - 1]--;
                        }
                    }
                } else if (task == TASK_H_ARROW_LEFT || task == TASK_L_ARROW_RIGHT ||
                           task == TASK_K_ARROW_UP || task == TASK_J_ARROW_DOWN ||
                           task == TASK_INPUT) {
                    printf("beestje123\n");

                    char *strippedKey = stripTrailingColon(line);
                    FfonElement *newElem;
                    if (nextLayerExists(appRenderer)) {
                        newElem = ffonElementCreateObject(strippedKey);
                        // Copy existing values if they exist
                        if (prevIdx >= 0 && prevIdx < _ffon_count && _ffon[prevIdx]->type == FFON_OBJECT) {
                            for (int j = 0; j < _ffon[prevIdx]->data.object->count; j++) {
                                ffonObjectAddElement(newElem->data.object,
                                                    ffonElementClone(_ffon[prevIdx]->data.object->elements[j]));
                            }
                        }
                    } else {
                        newElem = ffonElementCreateObject(strippedKey);
                        ffonObjectAddElement(newElem->data.object, ffonElementCreateString(""));
                    }
                    free(strippedKey);

                    if (prevIdx >= 0 && prevIdx < _ffon_count) {
                        ffonElementDestroy(_ffon[prevIdx]);
                        _ffon[prevIdx] = newElem;
                    }
                }

                break;
            }
        } else if (!isKey && isEditorCoordinate) {
            printf("beestje2, i=%d\n", i);

            int prevIdx = appRenderer->previousId.ids[i];
            if (i < appRenderer->previousId.depth - 1 && prevIdx >= 0 && prevIdx < _ffon_count &&
                _ffon[prevIdx]->type == FFON_OBJECT) {
                printf("beestje21, previous_id=");
                for (int j = 0; j < appRenderer->previousId.depth; j++) printf("%d ", appRenderer->previousId.ids[j]);
                printf(", current_id=");
                for (int j = 0; j < appRenderer->currentId.depth; j++) printf("%d ", appRenderer->currentId.ids[j]);
                printf(", prevIdx=%d\n", prevIdx);
                // Navigation is now handled by navigateToLevel at the start of each iteration
                // Just continue to next iteration
                continue;
            } else {
                printf("beestje22\n");

                if (task == TASK_APPEND || task == TASK_APPEND_APPEND ||
                    task == TASK_INSERT || task == TASK_INSERT_INSERT) {
                    printf("beestje221, previous_id=");
                    for (int j = 0; j < appRenderer->previousId.depth; j++) printf("%d ", appRenderer->previousId.ids[j]);
                    printf(", current_id=");
                    for (int j = 0; j < appRenderer->currentId.depth; j++) printf("%d ", appRenderer->currentId.ids[j]);
                    printf(", line='%s'\n", line);

                    if (prevIdx >= 0 && prevIdx < _ffon_count) {
                        ffonElementDestroy(_ffon[prevIdx]);
                        _ffon[prevIdx] = ffonElementCreateString(line);
                    }

                    if (_parentObj && history != HISTORY_REDO) {
                        // Insert empty string at current position (updateIds already moved the cursor)
                        int insertIdx = appRenderer->currentId.ids[i];
                        ffonObjectInsertElement(_parentObj, ffonElementCreateString(""), insertIdx);
                    } else if (!_parentObj && i == 0 && history != HISTORY_REDO) {
                        // At root level, insert a new empty string sibling
                        int insertIdx = appRenderer->currentId.ids[i];

                        // Expand root array
                        FfonElement **newArray = realloc(appRenderer->ffon,
                                                        sizeof(FfonElement *) * (appRenderer->ffonCount + 1));
                        if (newArray) {
                            appRenderer->ffon = newArray;

                            // Shift elements to make room
                            for (int k = appRenderer->ffonCount; k > insertIdx; k--) {
                                appRenderer->ffon[k] = appRenderer->ffon[k - 1];
                            }

                            // Insert new empty string
                            appRenderer->ffon[insertIdx] = ffonElementCreateString("");
                            appRenderer->ffonCount++;
                        }
                    }
                } else if (task == TASK_DELETE) {
                    printf("beestje222, previous_id=");
                    for (int j = 0; j < appRenderer->previousId.depth; j++) printf("%d ", appRenderer->previousId.ids[j]);
                    printf(", current_id=");
                    for (int j = 0; j < appRenderer->currentId.depth; j++) printf("%d ", appRenderer->currentId.ids[j]);
                    printf("\n");

                    int removeIdx = appRenderer->previousId.ids[i];
                    if (removeIdx >= 0 && removeIdx < _ffon_count) {
                        ffonElementDestroy(_ffon[removeIdx]);
                        for (int j = removeIdx; j < _ffon_count - 1; j++) {
                            _ffon[j] = _ffon[j + 1];
                        }
                        _ffon_count--;

                        // Update the parent object's count to match
                        if (_parentObj) {
                            _parentObj->count = _ffon_count;
                        } else if (i == 0) {
                            // Update root level count
                            appRenderer->ffonCount = _ffon_count;
                        }
                    }

                    if (appRenderer->currentId.ids[appRenderer->currentId.depth - 1] == 0 &&
                        _ffon_count == 0 && _parentObj) {
                        printf("beestje2221\n");
                        ffonObjectInsertElement(_parentObj, ffonElementCreateString(""), appRenderer->previousId.ids[i]);
                    }

                    if (appRenderer->currentId.ids[appRenderer->currentId.depth - 1] > 0) {
                        appRenderer->currentId.ids[appRenderer->currentId.depth - 1]--;
                    }
                } else if (task == TASK_H_ARROW_LEFT || task == TASK_L_ARROW_RIGHT ||
                           task == TASK_K_ARROW_UP || task == TASK_J_ARROW_DOWN ||
                           task == TASK_INPUT) {
                    printf("beestje223, previous_id=");
                    for (int j = 0; j < appRenderer->previousId.depth; j++) printf("%d ", appRenderer->previousId.ids[j]);
                    printf(", current_id=");
                    for (int j = 0; j < appRenderer->currentId.depth; j++) printf("%d ", appRenderer->currentId.ids[j]);
                    printf("\n");

                    if (prevIdx >= 0 && prevIdx < _ffon_count) {
                        ffonElementDestroy(_ffon[prevIdx]);
                        _ffon[prevIdx] = ffonElementCreateString(line);
                    }
                }

                break;
            }
        }
    }

    printf("hee, previous_id=");
    for (int i = 0; i < appRenderer->previousId.depth; i++) printf("%d ", appRenderer->previousId.ids[i]);
    printf(", current_id=");
    for (int i = 0; i < appRenderer->currentId.depth; i++) printf("%d ", appRenderer->currentId.ids[i]);
    printf("\n");
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

    fprintf(stderr, "undoPosition: %d", appRenderer->undoPosition);

    if (history == HISTORY_UNDO) {
        // Save current state before undo
        handleEscape(appRenderer);

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
            // appRenderer->undoPosition--;

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

            // appRenderer->undoPosition--;
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

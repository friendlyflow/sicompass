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

    // Capture element BEFORE modification for undo history
    FfonElement *prevElement = NULL;
    IdArray historyId;
    idArrayCopy(&historyId, &appRenderer->currentId);

    if (history == HISTORY_NONE &&
        (task == TASK_DELETE || task == TASK_INPUT)) {
        int count;
        FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
        if (arr && count > 0) {
            int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                prevElement = ffonElementClone(arr[idx]);
            }
        }
    }

    // Get current line (still needed for updateFfon/updateIds)
    char line[MAX_LINE_LENGTH] = "";
    bool currentElemIsObject = false;

    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT) {
        strncpy(line, appRenderer->inputBuffer, MAX_LINE_LENGTH - 1);
    } else {
        int count;
        FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
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

    printf("update state line: '%s'\n", line);

    bool isKey = isLineKey(line) || currentElemIsObject;
    updateIds(appRenderer, isKey, task, history);
    updateFfon(appRenderer, line, isKey, task, history);

    // Capture element AFTER modification for undo history
    FfonElement *newElement = NULL;
    if (history == HISTORY_NONE &&
        (task == TASK_APPEND || task == TASK_APPEND_APPEND ||
         task == TASK_INSERT || task == TASK_INSERT_INSERT ||
         task == TASK_INPUT)) {
        int count;
        FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
        if (arr && count > 0) {
            int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                newElement = ffonElementClone(arr[idx]);
            }
        }
    }

    // For APPEND/INSERT, the history id is the new element's position (currentId)
    // For DELETE, the history id is the original position (captured before updateIds)
    // For INPUT, position doesn't change so either works
    const IdArray *recordId = (task == TASK_DELETE) ? &historyId : &appRenderer->currentId;
    updateHistory(appRenderer, task, recordId, prevElement, newElement, history);

    // Clean up local clones (updateHistory clones them internally)
    if (prevElement) ffonElementDestroy(prevElement);
    if (newElement) ffonElementDestroy(newElement);

    createListCurrentLayer(appRenderer);
}

void updateIds(AppRenderer *appRenderer, bool isKey, Task task, History history) {
    idArrayCopy(&appRenderer->previousId, &appRenderer->currentId);

    if (history == HISTORY_UNDO || history == HISTORY_REDO) {
        return;
    }

    int maxId = getFfonMaxIdAtPath(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId);
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
            if (nextFfonLayerExists(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->previousId)) {
                idArrayPush(&appRenderer->currentId, 0);
            }
            break;

        case TASK_APPEND:
            if (appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
                appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) {
                if (!isKey) {
                    appRenderer->currentId.ids[appRenderer->currentId.depth - 1]++;
                } else {
                    if (nextFfonLayerExists(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->previousId)) {
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
                    if (nextFfonLayerExists(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->previousId)) {
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
        } else if (task == TASK_DELETE) {
            // Non-editor delete (e.g. file browser in COORDINATE_OPERATOR_GENERAL)
            int prevIdx = appRenderer->previousId.ids[i];
            if (i < appRenderer->previousId.depth - 1 && prevIdx >= 0 && prevIdx < _ffon_count &&
                _ffon[prevIdx]->type == FFON_OBJECT) {
                // Intermediate level: navigate deeper next iteration
                continue;
            } else {
                // Target level: remove the element
                int removeIdx = appRenderer->previousId.ids[i];
                if (removeIdx >= 0 && removeIdx < _ffon_count) {
                    ffonElementDestroy(_ffon[removeIdx]);
                    for (int j = removeIdx; j < _ffon_count - 1; j++) {
                        _ffon[j] = _ffon[j + 1];
                    }
                    _ffon_count--;
                    if (_parentObj) {
                        _parentObj->count = _ffon_count;
                    } else if (i == 0) {
                        appRenderer->ffonCount = _ffon_count;
                    }
                    if (appRenderer->currentId.ids[appRenderer->currentId.depth - 1] > 0) {
                        appRenderer->currentId.ids[appRenderer->currentId.depth - 1]--;
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

void updateHistory(AppRenderer *appRenderer, Task task, const IdArray *id, FfonElement *prevElement, FfonElement *newElement, History history) {
    if (history != HISTORY_NONE) return;

    if (task == TASK_APPEND || task == TASK_APPEND_APPEND ||
        task == TASK_INSERT || task == TASK_INSERT_INSERT ||
        task == TASK_DELETE || task == TASK_INPUT ||
        task == TASK_CUT || task == TASK_PASTE) {

        // Truncate redo entries beyond current position
        if (appRenderer->undoPosition > 0) {
            int newCount = appRenderer->undoHistoryCount - appRenderer->undoPosition;
            for (int i = newCount; i < appRenderer->undoHistoryCount; i++) {
                if (appRenderer->undoHistory[i].prevElement) {
                    ffonElementDestroy(appRenderer->undoHistory[i].prevElement);
                    appRenderer->undoHistory[i].prevElement = NULL;
                }
                if (appRenderer->undoHistory[i].newElement) {
                    ffonElementDestroy(appRenderer->undoHistory[i].newElement);
                    appRenderer->undoHistory[i].newElement = NULL;
                }
            }
            appRenderer->undoHistoryCount = newCount;
        }

        if (appRenderer->undoHistoryCount >= UNDO_HISTORY_SIZE) {
            // Remove oldest entry
            if (appRenderer->undoHistory[0].prevElement) {
                ffonElementDestroy(appRenderer->undoHistory[0].prevElement);
            }
            if (appRenderer->undoHistory[0].newElement) {
                ffonElementDestroy(appRenderer->undoHistory[0].newElement);
            }
            memmove(&appRenderer->undoHistory[0], &appRenderer->undoHistory[1],
                   sizeof(UndoEntry) * (UNDO_HISTORY_SIZE - 1));
            appRenderer->undoHistoryCount--;
        }

        UndoEntry *entry = &appRenderer->undoHistory[appRenderer->undoHistoryCount++];
        idArrayCopy(&entry->id, id);
        entry->task = task;
        entry->prevElement = prevElement ? ffonElementClone(prevElement) : NULL;
        entry->newElement = newElement ? ffonElementClone(newElement) : NULL;

        appRenderer->undoPosition = 0;
    }
}

// Helper: Insert a cloned element at a given IdArray position
static void insertElementAtId(AppRenderer *appRenderer, const IdArray *id, FfonElement *elem) {
    int insertIdx = id->ids[id->depth - 1];

    if (id->depth == 1) {
        // Root level
        if (appRenderer->ffonCount >= appRenderer->ffonCapacity) {
            appRenderer->ffonCapacity = appRenderer->ffonCapacity ? appRenderer->ffonCapacity * 2 : 4;
            appRenderer->ffon = realloc(appRenderer->ffon,
                appRenderer->ffonCapacity * sizeof(FfonElement*));
        }
        for (int i = appRenderer->ffonCount; i > insertIdx; i--) {
            appRenderer->ffon[i] = appRenderer->ffon[i - 1];
        }
        appRenderer->ffon[insertIdx] = ffonElementClone(elem);
        appRenderer->ffonCount++;
    } else {
        // Navigate to parent object
        FfonElement **ffon = appRenderer->ffon;
        int count = appRenderer->ffonCount;
        FfonObject *parent = NULL;

        for (int i = 0; i < id->depth - 1; i++) {
            int idx = id->ids[i];
            if (idx < 0 || idx >= count || ffon[idx]->type != FFON_OBJECT) return;
            parent = ffon[idx]->data.object;
            ffon = parent->elements;
            count = parent->count;
        }

        if (parent) {
            ffonObjectInsertElement(parent, ffonElementClone(elem), insertIdx);
        }
    }
}

// Helper: Replace the element at a given IdArray position with a clone
static void replaceElementAtId(AppRenderer *appRenderer, const IdArray *id, FfonElement *elem) {
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, id, &count);
    if (!arr) return;

    int replaceIdx = id->ids[id->depth - 1];
    if (replaceIdx >= 0 && replaceIdx < count) {
        ffonElementDestroy(arr[replaceIdx]);
        arr[replaceIdx] = ffonElementClone(elem);
    }
}

void handleHistoryAction(AppRenderer *appRenderer, History history) {
    if (appRenderer->undoHistoryCount == 0) {
        setErrorMessage(appRenderer, "No undo history");
        return;
    }

    // Exit insert mode first
    handleEscape(appRenderer);

    if (history == HISTORY_UNDO) {
        if (appRenderer->undoPosition >= appRenderer->undoHistoryCount) {
            setErrorMessage(appRenderer, "Nothing to undo");
            return;
        }

        appRenderer->undoPosition++;
        UndoEntry *entry = &appRenderer->undoHistory[appRenderer->undoHistoryCount - appRenderer->undoPosition];

        switch (entry->task) {
            case TASK_APPEND:
            case TASK_APPEND_APPEND:
            case TASK_INSERT:
            case TASK_INSERT_INSERT:
                // These created an element. Undo = remove it.
                idArrayCopy(&appRenderer->currentId, &entry->id);
                handleDelete(appRenderer, history);
                if (appRenderer->currentId.ids[appRenderer->currentId.depth - 1] > 0) {
                    appRenderer->currentId.ids[appRenderer->currentId.depth - 1]--;
                }
                break;

            case TASK_DELETE:
            case TASK_CUT:
                // These removed an element. Undo = re-insert prevElement.
                if (entry->prevElement) {
                    insertElementAtId(appRenderer, &entry->id, entry->prevElement);
                    idArrayCopy(&appRenderer->currentId, &entry->id);
                }
                break;

            case TASK_INPUT:
            case TASK_PASTE:
                // These replaced an element. Undo = replace with prevElement.
                if (entry->prevElement) {
                    replaceElementAtId(appRenderer, &entry->id, entry->prevElement);
                    idArrayCopy(&appRenderer->currentId, &entry->id);
                }
                break;

            default:
                break;
        }
    } else if (history == HISTORY_REDO) {
        if (appRenderer->undoPosition <= 0) {
            setErrorMessage(appRenderer, "Nothing to redo");
            return;
        }

        UndoEntry *entry = &appRenderer->undoHistory[appRenderer->undoHistoryCount - appRenderer->undoPosition];
        appRenderer->undoPosition--;

        switch (entry->task) {
            case TASK_APPEND:
            case TASK_APPEND_APPEND:
            case TASK_INSERT:
            case TASK_INSERT_INSERT:
                // These created an element. Redo = re-insert newElement.
                if (entry->newElement) {
                    insertElementAtId(appRenderer, &entry->id, entry->newElement);
                    idArrayCopy(&appRenderer->currentId, &entry->id);
                }
                break;

            case TASK_DELETE:
            case TASK_CUT:
                // These removed an element. Redo = remove again.
                idArrayCopy(&appRenderer->currentId, &entry->id);
                handleDelete(appRenderer, history);
                // Adjust cursor if it's now out of bounds
                {
                    int count;
                    getFfonAtId(appRenderer->ffon, appRenderer->ffonCount,
                               &appRenderer->currentId, &count);
                    int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
                    if (idx >= count && count > 0) {
                        appRenderer->currentId.ids[appRenderer->currentId.depth - 1] = count - 1;
                    }
                }
                break;

            case TASK_INPUT:
            case TASK_PASTE:
                // These replaced an element. Redo = replace with newElement.
                if (entry->newElement) {
                    replaceElementAtId(appRenderer, &entry->id, entry->newElement);
                    idArrayCopy(&appRenderer->currentId, &entry->id);
                }
                break;

            default:
                break;
        }
    }

    createListCurrentLayer(appRenderer);
    appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    appRenderer->needsRedraw = true;
}

void handleCtrlX(AppRenderer *appRenderer) {
    // Text mode: cut selected text to system clipboard
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND) {

        if (!hasSelection(appRenderer)) return;

        int start, end;
        getSelectionRange(appRenderer, &start, &end);

        int len = end - start;
        char *selectedText = malloc(len + 1);
        if (!selectedText) return;
        memcpy(selectedText, &appRenderer->inputBuffer[start], len);
        selectedText[len] = '\0';

        SDL_SetClipboardText(selectedText);
        free(selectedText);

        deleteSelection(appRenderer);
        caretReset(appRenderer->caretState, SDL_GetTicks());

        if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
            appRenderer->currentCoordinate == COORDINATE_COMMAND ||
            appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
            populateListCurrentLayer(appRenderer, appRenderer->inputBuffer);
        }

        appRenderer->needsRedraw = true;
        return;
    }

    // Element mode: cut FFON element
    FfonElement **_ffon = appRenderer->ffon;
    int _ffon_count = appRenderer->ffonCount;
    FfonObject *parentObj = NULL;

    for (int i = 0; i < appRenderer->currentId.depth - 1; i++) {
        int idx = appRenderer->currentId.ids[i];
        if (idx < 0 || idx >= _ffon_count || _ffon[idx]->type != FFON_OBJECT) {
            return;
        }
        parentObj = _ffon[idx]->data.object;
        _ffon = parentObj->elements;
        _ffon_count = parentObj->count;
    }

    int currentIdx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (currentIdx < 0 || currentIdx >= _ffon_count) return;

    FfonElement *elem = _ffon[currentIdx];

    // Clone for undo history BEFORE destroying
    FfonElement *prevElement = ffonElementClone(elem);

    // Capture the id before cursor adjustment
    IdArray historyId;
    idArrayCopy(&historyId, &appRenderer->currentId);

    // Store in clipboard
    if (appRenderer->clipboard) {
        ffonElementDestroy(appRenderer->clipboard);
    }
    appRenderer->clipboard = ffonElementClone(elem);

    // Remove element at current position
    ffonElementDestroy(_ffon[currentIdx]);
    for (int j = currentIdx; j < _ffon_count - 1; j++) {
        _ffon[j] = _ffon[j + 1];
    }
    _ffon_count--;

    if (parentObj) {
        parentObj->count = _ffon_count;
    } else {
        appRenderer->ffonCount = _ffon_count;
    }

    // Adjust cursor position
    if (currentIdx > 0) {
        appRenderer->currentId.ids[appRenderer->currentId.depth - 1]--;
    }

    updateHistory(appRenderer, TASK_CUT, &historyId, prevElement, NULL, HISTORY_NONE);
    ffonElementDestroy(prevElement);

    appRenderer->needsRedraw = true;
}

void handleCtrlC(AppRenderer *appRenderer) {
    // Text mode: copy selected text to system clipboard
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND) {

        if (!hasSelection(appRenderer)) return;

        int start, end;
        getSelectionRange(appRenderer, &start, &end);

        int len = end - start;
        char *selectedText = malloc(len + 1);
        if (!selectedText) return;
        memcpy(selectedText, &appRenderer->inputBuffer[start], len);
        selectedText[len] = '\0';

        SDL_SetClipboardText(selectedText);
        free(selectedText);

        appRenderer->needsRedraw = true;
        return;
    }

    // Element mode: copy FFON element
    FfonElement **_ffon = appRenderer->ffon;
    int _ffon_count = appRenderer->ffonCount;

    for (int i = 0; i < appRenderer->currentId.depth - 1; i++) {
        int idx = appRenderer->currentId.ids[i];
        if (idx < 0 || idx >= _ffon_count || _ffon[idx]->type != FFON_OBJECT) {
            return;
        }
        FfonObject *obj = _ffon[idx]->data.object;
        _ffon = obj->elements;
        _ffon_count = obj->count;
    }

    int currentIdx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (currentIdx < 0 || currentIdx >= _ffon_count) return;

    FfonElement *elem = _ffon[currentIdx];

    if (appRenderer->clipboard) {
        ffonElementDestroy(appRenderer->clipboard);
    }
    appRenderer->clipboard = ffonElementClone(elem);

    appRenderer->needsRedraw = true;
}

void handleCtrlV(AppRenderer *appRenderer) {
    // Text mode: paste from system clipboard
    if (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
        appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND) {

        if (!SDL_HasClipboardText()) return;

        char *text = SDL_GetClipboardText();
        if (!text || text[0] == '\0') {
            SDL_free(text);
            return;
        }

        // Delete selection if active
        if (hasSelection(appRenderer)) {
            deleteSelection(appRenderer);
        }

        // Insert clipboard text at cursor position
        int len = strlen(text);
        while (appRenderer->inputBufferSize + len >= appRenderer->inputBufferCapacity) {
            int newCapacity = appRenderer->inputBufferCapacity * 2;
            char *newBuffer = realloc(appRenderer->inputBuffer, newCapacity);
            if (!newBuffer) {
                SDL_free(text);
                return;
            }
            appRenderer->inputBuffer = newBuffer;
            appRenderer->inputBufferCapacity = newCapacity;
        }

        memmove(&appRenderer->inputBuffer[appRenderer->cursorPosition + len],
                &appRenderer->inputBuffer[appRenderer->cursorPosition],
                appRenderer->inputBufferSize - appRenderer->cursorPosition + 1);
        memcpy(&appRenderer->inputBuffer[appRenderer->cursorPosition], text, len);
        appRenderer->inputBufferSize += len;
        appRenderer->cursorPosition += len;

        SDL_free(text);
        caretReset(appRenderer->caretState, SDL_GetTicks());

        if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
            appRenderer->currentCoordinate == COORDINATE_COMMAND ||
            appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
            populateListCurrentLayer(appRenderer, appRenderer->inputBuffer);
        }

        appRenderer->needsRedraw = true;
        return;
    }

    // Element mode: paste FFON element
    if (!appRenderer->clipboard) return;

    FfonElement **_ffon = appRenderer->ffon;
    int _ffon_count = appRenderer->ffonCount;

    for (int i = 0; i < appRenderer->currentId.depth - 1; i++) {
        int idx = appRenderer->currentId.ids[i];
        if (idx < 0 || idx >= _ffon_count || _ffon[idx]->type != FFON_OBJECT) {
            return;
        }
        FfonObject *obj = _ffon[idx]->data.object;
        _ffon = obj->elements;
        _ffon_count = obj->count;
    }

    int currentIdx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (currentIdx < 0 || currentIdx >= _ffon_count) return;

    // Clone the element being replaced for undo
    FfonElement *prevElement = ffonElementClone(_ffon[currentIdx]);

    // Replace with clipboard content
    FfonElement *newElem = ffonElementClone(appRenderer->clipboard);
    if (newElem) {
        ffonElementDestroy(_ffon[currentIdx]);
        _ffon[currentIdx] = newElem;

        updateHistory(appRenderer, TASK_PASTE, &appRenderer->currentId, prevElement, newElem, HISTORY_NONE);
    }

    if (prevElement) ffonElementDestroy(prevElement);

    appRenderer->needsRedraw = true;
}

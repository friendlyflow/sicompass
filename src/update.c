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
    printf("update sfon struct, line='%s', previous_id=", line);
    for (int i = 0; i < appRenderer->previousId.depth; i++) printf("%d ", appRenderer->previousId.ids[i]);
    printf(", current_id=");
    for (int i = 0; i < appRenderer->currentId.depth; i++) printf("%d ", appRenderer->currentId.ids[i]);
    printf("\n");
    printf("update, isKey=%d, task=%d\n", isKey, task);

    FfonElement **_ffon = appRenderer->ffon;
    int _ffon_count = appRenderer->ffonCount;
    FfonObject *_parentObj = NULL;  // Track parent object for insertions

    for (int i = 0; i < appRenderer->previousId.depth; i++) {
        printf("beestje\n");

        bool isEditorCoordinate = (appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL ||
                                   appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
                                   appRenderer->currentCoordinate == COORDINATE_EDITOR_NORMAL ||
                                   appRenderer->currentCoordinate == COORDINATE_EDITOR_VISUAL);

        if (isKey && isEditorCoordinate) {
            printf("beestje1\n");

            if (task == TASK_DELETE && i == appRenderer->currentId.depth - 1) {
                printf("beestje10\n");

                // Remove element at current_id[i]
                int removeIdx = appRenderer->currentId.ids[i];
                if (removeIdx >= 0 && removeIdx < _ffon_count) {
                    ffonElementDestroy(_ffon[removeIdx]);
                    for (int j = removeIdx; j < _ffon_count - 1; j++) {
                        _ffon[j] = _ffon[j + 1];
                    }
                    _ffon_count--;
                }

                // Insert empty string if not at root
                if (i != 0 && removeIdx >= 0 && removeIdx <= _ffon_count) {
                    // Need to insert into parent object
                    FfonObject *parentObj = NULL;
                    if (i > 0 && _ffon[appRenderer->currentId.ids[i-1]]->type == FFON_OBJECT) {
                        parentObj = _ffon[appRenderer->currentId.ids[i-1]]->data.object;
                        ffonObjectInsertElement(parentObj, ffonElementCreateString(""), removeIdx);
                    }
                }
            }

            int prevIdx = appRenderer->previousId.ids[i];
            if (prevIdx >= 0 && prevIdx < _ffon_count && _ffon[prevIdx]->type == FFON_OBJECT) {
                printf("beestje11, i=%d\n", i);

                if (i < appRenderer->previousId.depth - 1) {
                    printf("beestje111\n");
                    _ffon = _ffon[prevIdx]->data.object->elements;
                    _ffon_count = _ffon[prevIdx]->data.object->count;
                } else {
                    printf("beestje112\n");
                    if (task == TASK_APPEND || task == TASK_APPEND_APPEND ||
                        task == TASK_INSERT || task == TASK_INSERT_INSERT) {
                        printf("beestje1121\n");

                        // Get the old object's children
                        FfonElement **oldChildren = _ffon[prevIdx]->data.object->elements;
                        int oldCount = _ffon[prevIdx]->data.object->count;

                        // Create new object with new key
                        FfonElement *newElem = ffonElementCreateObject(line);

                        // Transfer children to new object
                        for (int j = 0; j < oldCount; j++) {
                            ffonObjectAddElement(newElem->data.object, oldChildren[j]);
                        }

                        // Free old object structure (but not children, we transferred them)
                        _ffon[prevIdx]->data.object->count = 0; // Don't destroy children
                        ffonElementDestroy(_ffon[prevIdx]);
                        _ffon[prevIdx] = newElem;

                        // Insert empty string at current_id[i]
                        ffonObjectInsertElement(newElem->data.object, ffonElementCreateString(""),
                                               appRenderer->currentId.ids[i]);
                    } else if (task == TASK_H_ARROW_LEFT || task == TASK_L_ARROW_RIGHT ||
                               task == TASK_K_ARROW_UP || task == TASK_J_ARROW_DOWN ||
                               task == TASK_INPUT) {
                        printf("beestje1123\n");

                        // Get the old object's children
                        FfonElement **oldChildren = _ffon[prevIdx]->data.object->elements;
                        int oldCount = _ffon[prevIdx]->data.object->count;

                        // Create new object with new key
                        FfonElement *newElem = ffonElementCreateObject(line);

                        // Transfer children to new object
                        for (int j = 0; j < oldCount; j++) {
                            ffonObjectAddElement(newElem->data.object, oldChildren[j]);
                        }

                        // Free old object structure (but not children)
                        _ffon[prevIdx]->data.object->count = 0;
                        ffonElementDestroy(_ffon[prevIdx]);
                        _ffon[prevIdx] = newElem;

                        break;
                    }
                }
            } else {
                printf("beestje12\n");

                if (task == TASK_APPEND || task == TASK_APPEND_APPEND ||
                    task == TASK_INSERT || task == TASK_INSERT_INSERT) {
                    printf("beestje121 previous_id=");
                    for (int j = 0; j < appRenderer->previousId.depth; j++) printf("%d ", appRenderer->previousId.ids[j]);
                    printf(", current_id=");
                    for (int j = 0; j < appRenderer->currentId.depth; j++) printf("%d ", appRenderer->currentId.ids[j]);
                    printf(", _ffon_count=%d, line='%s'\n", _ffon_count, line);

                    if (prevIdx >= 0 && prevIdx < _ffon_count) {
                        ffonElementDestroy(_ffon[prevIdx]);
                        _ffon[prevIdx] = ffonElementCreateString(line);
                    }

                    if (history != HISTORY_REDO && i < appRenderer->currentId.depth && _parentObj) {
                        // Insert empty string at current_id[i] position (like splice in JS)
                        int insertIdx = appRenderer->currentId.ids[i];
                        ffonObjectInsertElement(_parentObj, ffonElementCreateString(""), insertIdx);
                    }
                } else if (task == TASK_DELETE) {
                    printf("beestje122\n");

                    if (appRenderer->currentId.ids[appRenderer->currentId.depth - 1] > 0) {
                        int removeIdx = appRenderer->currentId.ids[i];
                        if (removeIdx >= 0 && removeIdx < _ffon_count) {
                            ffonElementDestroy(_ffon[removeIdx]);
                            for (int j = removeIdx; j < _ffon_count - 1; j++) {
                                _ffon[j] = _ffon[j + 1];
                            }
                            _ffon_count--;
                            appRenderer->currentId.ids[appRenderer->currentId.depth - 1]--;
                        }
                    }
                } else if (task == TASK_H_ARROW_LEFT || task == TASK_L_ARROW_RIGHT ||
                           task == TASK_K_ARROW_UP || task == TASK_J_ARROW_DOWN ||
                           task == TASK_INPUT) {
                    printf("beestje123\n");

                    FfonElement *newElem;
                    if (nextLayerExists(appRenderer)) {
                        newElem = ffonElementCreateObject(line);
                        // Copy existing values if they exist
                        if (prevIdx >= 0 && prevIdx < _ffon_count && _ffon[prevIdx]->type == FFON_OBJECT) {
                            for (int j = 0; j < _ffon[prevIdx]->data.object->count; j++) {
                                ffonObjectAddElement(newElem->data.object,
                                                    ffonElementClone(_ffon[prevIdx]->data.object->elements[j]));
                            }
                        }
                    } else {
                        newElem = ffonElementCreateObject(line);
                        ffonObjectAddElement(newElem->data.object, ffonElementCreateString(""));
                    }

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
                printf("beestje21\n");

                _parentObj = _ffon[prevIdx]->data.object;
                _ffon = _ffon[prevIdx]->data.object->elements;
                _ffon_count = _ffon[prevIdx]->data.object->count;
                continue;
            } else {
                printf("beestje22\n");

                if (task == TASK_APPEND || task == TASK_APPEND_APPEND ||
                    task == TASK_INSERT || task == TASK_INSERT_INSERT) {
                    printf("beestje221 previous_id=");
                    for (int j = 0; j < appRenderer->previousId.depth; j++) printf("%d ", appRenderer->previousId.ids[j]);
                    printf(", current_id=");
                    for (int j = 0; j < appRenderer->currentId.depth; j++) printf("%d ", appRenderer->currentId.ids[j]);
                    printf(", _ffon_count=%d, line='%s'\n", _ffon_count, line);

                    if (prevIdx >= 0 && prevIdx < _ffon_count) {
                        ffonElementDestroy(_ffon[prevIdx]);
                        _ffon[prevIdx] = ffonElementCreateString(line);
                    }

                    if (history != HISTORY_REDO && i < appRenderer->currentId.depth && _parentObj) {
                        // Insert empty string at current_id[i] position (like splice in JS)
                        int insertIdx = appRenderer->currentId.ids[i];
                        ffonObjectInsertElement(_parentObj, ffonElementCreateString(""), insertIdx);
                    }
                } else if (task == TASK_DELETE) {
                    printf("beestje222\n");

                    int removeIdx = appRenderer->currentId.ids[i];
                    if (removeIdx >= 0 && removeIdx < _ffon_count) {
                        ffonElementDestroy(_ffon[removeIdx]);
                        for (int j = removeIdx; j < _ffon_count - 1; j++) {
                            _ffon[j] = _ffon[j + 1];
                        }
                        _ffon_count--;
                    }

                    if (appRenderer->currentId.ids[appRenderer->currentId.depth - 1] == 0 &&
                        _ffon_count == 0) {
                        printf("beestje2221\n");
                        // Insert empty string - need parent object context here
                        if (i > 0) {
                            // Get parent and insert
                        }
                    }

                    if (appRenderer->currentId.ids[appRenderer->currentId.depth - 1] > 0) {
                        appRenderer->currentId.ids[appRenderer->currentId.depth - 1]--;
                    }
                } else if (task == TASK_H_ARROW_LEFT || task == TASK_L_ARROW_RIGHT ||
                           task == TASK_K_ARROW_UP || task == TASK_J_ARROW_DOWN ||
                           task == TASK_INPUT) {
                    printf("beestje223\n");

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

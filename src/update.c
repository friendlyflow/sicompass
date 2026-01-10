#include "view.h"
#include <string.h>
#include <stdlib.h>

void updateState(EditorState *state, Task task, History history) {
    // Get current line
    char line[MAX_LINE_LENGTH] = "";

    if (history == HISTORY_UNDO || history == HISTORY_REDO) {
        if (state->undoPosition < state->undoHistoryCount) {
            strncpy(line, state->undoHistory[state->undoHistoryCount - state->undoPosition].line,
                   MAX_LINE_LENGTH - 1);
        }
    } else {
        // Get line from current element or input buffer
        if (state->currentCoordinate == COORDINATE_LEFT_EDITOR_INSERT ||
            state->currentCoordinate == COORDINATE_LEFT_VISITOR_INSERT) {
            strncpy(line, state->inputBuffer, MAX_LINE_LENGTH - 1);
        } else {
            int count;
            FfonElement **arr = getFfonAtId(state, &state->currentId, &count);
            if (arr && count > 0) {
                int idx = state->currentId.ids[state->currentId.depth - 1];
                if (idx >= 0 && idx < count) {
                    FfonElement *elem = arr[idx];
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
    updateIds(state, isKey, task, history);
    updateFfon(state, line, isKey, task, history);
    updateHistory(state, task, isKey, line, history);
}

void updateIds(EditorState *state, bool isKey, Task task, History history) {
    idArrayCopy(&state->previousId, &state->currentId);

    if (history == HISTORY_UNDO || history == HISTORY_REDO) {
        return;
    }

    int maxId = getMaxIdInCurrent(state);
    int currentIdx = state->currentId.ids[state->currentId.depth - 1];

    switch (task) {
        case TASK_K_ARROW_UP:
            if (currentIdx > 0) {
                state->currentId.ids[state->currentId.depth - 1]--;
            }
            break;

        case TASK_J_ARROW_DOWN:
            if (currentIdx < maxId) {
                state->currentId.ids[state->currentId.depth - 1]++;
            }
            break;

        case TASK_H_ARROW_LEFT:
            if (state->currentId.depth > 1) {
                idArrayPop(&state->currentId);
            }
            break;

        case TASK_L_ARROW_RIGHT:
            if (nextLayerExists(state)) {
                idArrayPush(&state->currentId, 0);
            }
            break;

        case TASK_APPEND:
            if (state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL ||
                state->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL) {
                if (!isKey) {
                    state->currentId.ids[state->currentId.depth - 1]++;
                } else {
                    if (nextLayerExists(state)) {
                        state->currentId.ids[state->currentId.depth - 1]++;
                    } else {
                        idArrayPush(&state->currentId, 0);
                    }
                }
            }
            break;

        case TASK_APPEND_APPEND:
            if (state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL ||
                state->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL) {
                state->currentId.ids[state->currentId.depth - 1] = maxId + 1;
            }
            break;

        case TASK_INSERT:
            // Position stays the same
            break;

        case TASK_INSERT_INSERT:
            if (state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL ||
                state->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL) {
                state->currentId.ids[state->currentId.depth - 1] = 0;
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

void updateFfon(EditorState *state, const char *line, bool isKey, Task task, History history) {
    if (state->currentId.depth == 0) return;

    // Navigate to parent array
    int count;
    FfonElement **arr = getFfonAtId(state, &state->currentId, &count);
    if (!arr) return;

    int idx = state->currentId.ids[state->currentId.depth - 1];

    // Get parent object if we're nested
    FfonObject *parentObj = NULL;
    if (state->currentId.depth > 1) {
        FfonElement **parentArr = getFfonAtId(state, &state->currentId, &count);
        if (parentArr) {
            int parentIdx = state->currentId.ids[state->currentId.depth - 2];
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
                    // Update key
                    free(arr[idx]->data.object->key);
                    arr[idx]->data.object->key = strdup(line);
                } else {
                    // Convert string to object
                    FfonElement *newElem = ffonElementCreateObject(line);
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
                if (state->currentId.ids[state->currentId.depth - 1] > 0) {
                    state->currentId.ids[state->currentId.depth - 1]--;
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
            // Update current element with line content
            if (idx >= 0 && idx < count) {
                if (arr[idx]->type == FFON_STRING) {
                    free(arr[idx]->data.string);
                    arr[idx]->data.string = strdup(line);
                } else if (arr[idx]->type == FFON_OBJECT) {
                    free(arr[idx]->data.object->key);
                    arr[idx]->data.object->key = strdup(line);
                }
            }
            break;
        }

        default:
            break;
    }
}

void updateHistory(EditorState *state, Task task, bool isKey, const char *line, History history) {
    if (history != HISTORY_NONE) return;

    if (task == TASK_APPEND || task == TASK_APPEND_APPEND ||
        task == TASK_INSERT || task == TASK_INSERT_INSERT ||
        task == TASK_DELETE || task == TASK_INPUT) {

        if (state->undoHistoryCount >= UNDO_HISTORY_SIZE) {
            // Remove oldest entry
            free(state->undoHistory[0].line);
            memmove(&state->undoHistory[0], &state->undoHistory[1],
                   sizeof(UndoEntry) * (UNDO_HISTORY_SIZE - 1));
            state->undoHistoryCount--;
        }

        UndoEntry *entry = &state->undoHistory[state->undoHistoryCount++];
        idArrayCopy(&entry->id, &state->currentId);
        entry->task = task;
        entry->isKey = isKey;
        entry->line = strdup(line ? line : "");

        state->undoPosition = 0;
    }
}

void handleHistoryAction(EditorState *state, History history) {
    if (state->undoHistoryCount == 0) {
        setErrorMessage(state, "No undo history");
        return;
    }

    if (history == HISTORY_UNDO) {
        // Save current state before undo
        int count;
        FfonElement **arr = getFfonAtId(state, &state->currentId, &count);
        if (arr && count > 0) {
            int idx = state->currentId.ids[state->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                FfonElement *elem = arr[idx];
                char line[MAX_LINE_LENGTH] = "";
                if (elem->type == FFON_STRING) {
                    strncpy(line, elem->data.string, MAX_LINE_LENGTH - 1);
                } else {
                    strncpy(line, elem->data.object->key, MAX_LINE_LENGTH - 1);
                }

                bool isKey = isLineKey(line);
                updateIds(state, isKey, TASK_NONE, HISTORY_NONE);
                updateFfon(state, line, isKey, TASK_NONE, HISTORY_NONE);
            }
        }

        if (state->undoPosition < state->undoHistoryCount) {
            state->undoPosition++;
        }

        UndoEntry *entry = &state->undoHistory[state->undoHistoryCount - state->undoPosition];
        idArrayCopy(&state->currentId, &entry->id);

        // Reverse the operation
        switch (entry->task) {
            case TASK_APPEND:
            case TASK_APPEND_APPEND:
            case TASK_INSERT:
            case TASK_INSERT_INSERT:
                handleDelete(state, history);
                break;

            case TASK_DELETE:
                if (state->currentId.ids[state->currentId.depth - 1] == 0) {
                    handleCtrlI(state, history);
                } else {
                    handleCtrlA(state, history);
                }
                break;

            default:
                break;
        }
    } else if (history == HISTORY_REDO) {
        if (state->undoPosition > 0) {
            UndoEntry *entry = &state->undoHistory[state->undoHistoryCount - state->undoPosition];
            idArrayCopy(&state->currentId, &entry->id);

            // Redo the operation
            switch (entry->task) {
                case TASK_APPEND:
                case TASK_APPEND_APPEND:
                    handleCtrlA(state, history);
                    break;

                case TASK_INSERT:
                case TASK_INSERT_INSERT:
                    handleCtrlI(state, history);
                    break;

                case TASK_DELETE:
                    handleDelete(state, history);
                    break;

                default:
                    break;
            }

            state->undoPosition--;
        }
    }

    state->needsRedraw = true;
}

void handleCcp(EditorState *state, Task task) {
    int count;
    FfonElement **arr = getFfonAtId(state, &state->currentId, &count);
    if (!arr || count == 0) return;

    int idx = state->currentId.ids[state->currentId.depth - 1];
    if (idx < 0 || idx >= count) return;

    if (task == TASK_PASTE) {
        if (state->clipboard) {
            // Insert clipboard content
            FfonElement *newElem = ffonElementClone(state->clipboard);
            if (newElem) {
                // Add to parent
                FfonObject *parentObj = NULL;
                if (state->currentId.depth > 1) {
                    int parentCount;
                    FfonElement **parentArr = getFfonAtId(state, &state->currentId, &parentCount);
                    if (parentArr) {
                        int parentIdx = state->currentId.ids[state->currentId.depth - 2];
                        if (parentIdx >= 0 && parentIdx < parentCount &&
                            parentArr[parentIdx]->type == FFON_OBJECT) {
                            parentObj = parentArr[parentIdx]->data.object;
                        }
                    }
                }

                if (parentObj) {
                    ffonObjectAddElement(parentObj, newElem);
                    updateHistory(state, TASK_PASTE, false, "", HISTORY_NONE);
                }
            }
        }
    } else {
        // Copy or cut
        FfonElement *elem = arr[idx];

        if (state->clipboard) {
            ffonElementDestroy(state->clipboard);
        }

        if (elem->type == FFON_OBJECT && !nextLayerExists(state)) {
            // Copy the object's contents
            state->clipboard = ffonElementClone(elem);
        } else {
            state->clipboard = ffonElementClone(elem);
        }

        if (task == TASK_CUT) {
            handleDelete(state, HISTORY_NONE);
            updateHistory(state, TASK_CUT, false, "", HISTORY_NONE);
        }
    }

    state->needsRedraw = true;
}

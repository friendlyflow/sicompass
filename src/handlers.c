#include "view.h"
#include <string.h>
#include <SDL3/SDL.h>

void handleTab(EditorState *state) {
    state->previousCoordinate = state->currentCoordinate;
    state->currentCoordinate = COORDINATE_RIGHT_INFO;

    createListRight(state);
    state->needsRedraw = true;
}

void handleCtrlA(EditorState *state, History history) {
    uint64_t now = SDL_GetTicks();

    if (now - state->lastKeypressTime <= DELTA_MS) {
        state->lastKeypressTime = 0;
        handleHistoryAction(state, HISTORY_UNDO);
        updateState(state, TASK_APPEND_APPEND, HISTORY_NONE);
    } else {
        updateState(state, TASK_APPEND, history);
    }

    state->lastKeypressTime = now;
    state->needsRedraw = true;
}

void handleEnter(EditorState *state, History history) {
    uint64_t now = SDL_GetTicks();

    if (state->currentCoordinate == COORDINATE_RIGHT_INFO) {
        // Get selected item from list
        if (state->listIndex >= 0 && state->listIndex < state->filteredListCount) {
            idArrayCopy(&state->currentId, &state->filteredListRight[state->listIndex].id);
        }
        state->currentCoordinate = state->previousCoordinate;
        state->needsRedraw = true;
    } else if (state->currentCoordinate == COORDINATE_RIGHT_COMMAND) {
        // Execute selected command
        if (state->listIndex >= 0 && state->listIndex < state->filteredListCount) {
            const char *cmd = state->filteredListRight[state->listIndex].value;
            if (strcmp(cmd, "editor mode") == 0) {
                state->currentCommand = COMMAND_EDITOR_MODE;
            } else if (strcmp(cmd, "visitor mode") == 0) {
                state->currentCommand = COMMAND_VISITOR_MODE;
            }
            handleCommand(state);
        }
    }

    state->lastKeypressTime = now;
}

void handleCtrlEnter(EditorState *state, History history) {
    if (state->currentCoordinate == COORDINATE_LEFT_EDITOR_INSERT) {
        updateState(state, TASK_INPUT, HISTORY_NONE);
        state->currentCoordinate = COORDINATE_LEFT_EDITOR_GENERAL;
        handleRight(state);
        handleA(state);
    }
}

void handleCtrlI(EditorState *state, History history) {
    uint64_t now = SDL_GetTicks();

    if (now - state->lastKeypressTime <= DELTA_MS) {
        state->lastKeypressTime = 0;
        handleHistoryAction(state, HISTORY_UNDO);
        updateState(state, TASK_INSERT_INSERT, HISTORY_NONE);
    } else {
        updateState(state, TASK_INSERT, history);
    }

    state->lastKeypressTime = now;
    state->needsRedraw = true;
}

void handleDelete(EditorState *state, History history) {
    updateState(state, TASK_DELETE, history);
    state->needsRedraw = true;
}

void handleColon(EditorState *state) {
    state->previousCoordinate = state->currentCoordinate;
    state->currentCoordinate = COORDINATE_RIGHT_COMMAND;

    createListRight(state);
    state->needsRedraw = true;
}

void handleUp(EditorState *state) {
    if (state->currentCoordinate == COORDINATE_RIGHT_INFO ||
        state->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
        state->currentCoordinate == COORDINATE_RIGHT_FIND) {
        if (state->listIndex > 0) {
            state->listIndex--;
        }
    } else if (state->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        updateState(state, TASK_K_ARROW_UP, HISTORY_NONE);
    }
    state->needsRedraw = true;
}

void handleDown(EditorState *state) {
    if (state->currentCoordinate == COORDINATE_RIGHT_INFO ||
        state->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
        state->currentCoordinate == COORDINATE_RIGHT_FIND) {
        int maxIndex = (state->filteredListCount > 0) ?
                        state->filteredListCount - 1 :
                        state->totalListCount - 1;
        if (state->listIndex < maxIndex) {
            state->listIndex++;
        }
    } else if (state->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        updateState(state, TASK_J_ARROW_DOWN, HISTORY_NONE);
    }
    state->needsRedraw = true;
}

void handleLeft(EditorState *state) {
    if (state->currentCoordinate == COORDINATE_RIGHT_INFO ||
        state->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
        state->currentCoordinate == COORDINATE_RIGHT_FIND) {
        // Nothing to do
    } else if (state->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        updateState(state, TASK_H_ARROW_LEFT, HISTORY_NONE);
        state->needsRedraw = true;
    }
}

void handleRight(EditorState *state) {
    if (state->currentCoordinate == COORDINATE_RIGHT_INFO ||
        state->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
        state->currentCoordinate == COORDINATE_RIGHT_FIND) {
        // Nothing to do
    } else if (state->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        updateState(state, TASK_L_ARROW_RIGHT, HISTORY_NONE);
        state->needsRedraw = true;
    }
}

void handleI(EditorState *state) {
    if (state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        idArrayCopy(&state->currentInsertId, &state->currentId);
        state->previousCoordinate = state->currentCoordinate;
        state->currentCoordinate = COORDINATE_LEFT_EDITOR_INSERT;

        // Get current line content
        int count;
        SfonElement **arr = getSfonAtId(state, &state->currentId, &count);
        if (arr && count > 0) {
            int idx = state->currentId.ids[state->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                SfonElement *elem = arr[idx];
                if (elem->type == SFON_STRING) {
                    strncpy(state->inputBuffer, elem->data.string,
                           state->inputBufferCapacity - 1);
                    state->inputBufferSize = strlen(state->inputBuffer);
                } else {
                    strncpy(state->inputBuffer, elem->data.object->key,
                           state->inputBufferCapacity - 1);
                    state->inputBufferSize = strlen(state->inputBuffer);
                }
            }
        }

        state->cursorPosition = 0;
        idArrayInit(&state->currentInsertId);
        state->needsRedraw = true;
    }
}

void handleA(EditorState *state) {
    if (state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        idArrayCopy(&state->currentInsertId, &state->currentId);
        state->previousCoordinate = state->currentCoordinate;
        state->currentCoordinate = COORDINATE_LEFT_EDITOR_INSERT;

        // Get current line content
        int count;
        SfonElement **arr = getSfonAtId(state, &state->currentId, &count);
        if (arr && count > 0) {
            int idx = state->currentId.ids[state->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                SfonElement *elem = arr[idx];
                if (elem->type == SFON_STRING) {
                    strncpy(state->inputBuffer, elem->data.string,
                           state->inputBufferCapacity - 1);
                    state->inputBufferSize = strlen(state->inputBuffer);
                } else {
                    strncpy(state->inputBuffer, elem->data.object->key,
                           state->inputBufferCapacity - 1);
                    state->inputBufferSize = strlen(state->inputBuffer);
                }
            }
        }

        state->cursorPosition = state->inputBufferSize;
        idArrayInit(&state->currentInsertId);
        state->needsRedraw = true;
    }
}

void handleFind(EditorState *state) {
    if (state->currentCoordinate != COORDINATE_RIGHT_INFO &&
        state->currentCoordinate != COORDINATE_RIGHT_COMMAND) {
        state->previousCoordinate = state->currentCoordinate;
        state->currentCoordinate = COORDINATE_RIGHT_FIND;
        state->needsRedraw = true;
    }
}

void handleEscape(EditorState *state) {
    if (state->previousCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
        state->previousCoordinate == COORDINATE_LEFT_VISITOR_INSERT) {
        if (state->currentCoordinate == COORDINATE_LEFT_VISITOR_INSERT) {
            updateState(state, TASK_INPUT, HISTORY_NONE);
        }
        state->currentCoordinate = COORDINATE_LEFT_VISITOR_GENERAL;
    } else {
        if (state->currentCoordinate == COORDINATE_LEFT_EDITOR_INSERT) {
            updateState(state, TASK_INPUT, HISTORY_NONE);
        }
        state->currentCoordinate = COORDINATE_LEFT_EDITOR_GENERAL;
    }

    state->previousCoordinate = state->currentCoordinate;
    state->needsRedraw = true;
}

void handleCommand(EditorState *state) {
    switch (state->currentCommand) {
        case COMMAND_EDITOR_MODE:
            state->previousCoordinate = state->currentCoordinate;
            state->currentCoordinate = COORDINATE_LEFT_EDITOR_GENERAL;
            break;

        case COMMAND_VISITOR_MODE:
            state->previousCoordinate = state->currentCoordinate;
            state->currentCoordinate = COORDINATE_LEFT_VISITOR_GENERAL;
            break;
    }

    state->needsRedraw = true;
}

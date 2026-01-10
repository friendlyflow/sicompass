#include "view.h"
#include <string.h>
#include <SDL3/SDL.h>

void handleTab(AppRenderer *appRenderer) {
    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    appRenderer->currentCoordinate = COORDINATE_RIGHT_INFO;

    createListRight(appRenderer);
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

    if (appRenderer->currentCoordinate == COORDINATE_RIGHT_INFO) {
        // Get selected item from list
        if (appRenderer->listIndex >= 0 && appRenderer->listIndex < appRenderer->filteredListCount) {
            idArrayCopy(&appRenderer->currentId, &appRenderer->filteredListRight[appRenderer->listIndex].id);
        }
        appRenderer->currentCoordinate = appRenderer->previousCoordinate;
        appRenderer->needsRedraw = true;
    } else if (appRenderer->currentCoordinate == COORDINATE_RIGHT_COMMAND) {
        // Execute selected command
        if (appRenderer->listIndex >= 0 && appRenderer->listIndex < appRenderer->filteredListCount) {
            const char *cmd = appRenderer->filteredListRight[appRenderer->listIndex].value;
            if (strcmp(cmd, "editor mode") == 0) {
                appRenderer->currentCommand = COMMAND_EDITOR_MODE;
            } else if (strcmp(cmd, "visitor mode") == 0) {
                appRenderer->currentCommand = COMMAND_VISITOR_MODE;
            }
            handleCommand(appRenderer);
        }
    }

    appRenderer->lastKeypressTime = now;
}

void handleCtrlEnter(AppRenderer *appRenderer, History history) {
    if (appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_INSERT) {
        updateState(appRenderer, TASK_INPUT, HISTORY_NONE);
        appRenderer->currentCoordinate = COORDINATE_LEFT_EDITOR_GENERAL;
        handleRight(appRenderer);
        handleA(appRenderer);
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
    appRenderer->currentCoordinate = COORDINATE_RIGHT_COMMAND;

    createListRight(appRenderer);
    appRenderer->needsRedraw = true;
}

void handleUp(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_RIGHT_INFO ||
        appRenderer->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_RIGHT_FIND) {
        if (appRenderer->listIndex > 0) {
            appRenderer->listIndex--;
        }
    } else if (appRenderer->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        updateState(appRenderer, TASK_K_ARROW_UP, HISTORY_NONE);
    }
    appRenderer->needsRedraw = true;
}

void handleDown(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_RIGHT_INFO ||
        appRenderer->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_RIGHT_FIND) {
        int maxIndex = (appRenderer->filteredListCount > 0) ?
                        appRenderer->filteredListCount - 1 :
                        appRenderer->totalListCount - 1;
        if (appRenderer->listIndex < maxIndex) {
            appRenderer->listIndex++;
        }
    } else if (appRenderer->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        updateState(appRenderer, TASK_J_ARROW_DOWN, HISTORY_NONE);
    }
    appRenderer->needsRedraw = true;
}

void handleLeft(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_RIGHT_INFO ||
        appRenderer->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_RIGHT_FIND) {
        // Nothing to do
    } else if (appRenderer->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        updateState(appRenderer, TASK_H_ARROW_LEFT, HISTORY_NONE);
        appRenderer->needsRedraw = true;
    }
}

void handleRight(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_RIGHT_INFO ||
        appRenderer->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_RIGHT_FIND) {
        // Nothing to do
    } else if (appRenderer->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        updateState(appRenderer, TASK_L_ARROW_RIGHT, HISTORY_NONE);
        appRenderer->needsRedraw = true;
    }
}

void handleI(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        idArrayCopy(&appRenderer->currentInsertId, &appRenderer->currentId);
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        appRenderer->currentCoordinate = COORDINATE_LEFT_EDITOR_INSERT;

        // Get current line content
        int count;
        FfonElement **arr = getFfonAtId(appRenderer, &appRenderer->currentId, &count);
        if (arr && count > 0) {
            int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                FfonElement *elem = arr[idx];
                if (elem->type == FFON_STRING) {
                    strncpy(appRenderer->inputBuffer, elem->data.string,
                           appRenderer->inputBufferCapacity - 1);
                    appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
                } else {
                    strncpy(appRenderer->inputBuffer, elem->data.object->key,
                           appRenderer->inputBufferCapacity - 1);
                    appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
                }
            }
        }

        appRenderer->cursorPosition = 0;
        idArrayInit(&appRenderer->currentInsertId);
        appRenderer->needsRedraw = true;
    }
}

void handleA(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        idArrayCopy(&appRenderer->currentInsertId, &appRenderer->currentId);
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        appRenderer->currentCoordinate = COORDINATE_LEFT_EDITOR_INSERT;

        // Get current line content
        int count;
        FfonElement **arr = getFfonAtId(appRenderer, &appRenderer->currentId, &count);
        if (arr && count > 0) {
            int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
            if (idx >= 0 && idx < count) {
                FfonElement *elem = arr[idx];
                if (elem->type == FFON_STRING) {
                    strncpy(appRenderer->inputBuffer, elem->data.string,
                           appRenderer->inputBufferCapacity - 1);
                    appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
                } else {
                    strncpy(appRenderer->inputBuffer, elem->data.object->key,
                           appRenderer->inputBufferCapacity - 1);
                    appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
                }
            }
        }

        appRenderer->cursorPosition = appRenderer->inputBufferSize;
        idArrayInit(&appRenderer->currentInsertId);
        appRenderer->needsRedraw = true;
    }
}

void handleFind(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate != COORDINATE_RIGHT_INFO &&
        appRenderer->currentCoordinate != COORDINATE_RIGHT_COMMAND) {
        appRenderer->previousCoordinate = appRenderer->currentCoordinate;
        appRenderer->currentCoordinate = COORDINATE_RIGHT_FIND;
        appRenderer->needsRedraw = true;
    }
}

void handleEscape(AppRenderer *appRenderer) {
    if (appRenderer->previousCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
        appRenderer->previousCoordinate == COORDINATE_LEFT_VISITOR_INSERT) {
        if (appRenderer->currentCoordinate == COORDINATE_LEFT_VISITOR_INSERT) {
            updateState(appRenderer, TASK_INPUT, HISTORY_NONE);
        }
        appRenderer->currentCoordinate = COORDINATE_LEFT_VISITOR_GENERAL;
    } else {
        if (appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_INSERT) {
            updateState(appRenderer, TASK_INPUT, HISTORY_NONE);
        }
        appRenderer->currentCoordinate = COORDINATE_LEFT_EDITOR_GENERAL;
    }

    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    appRenderer->needsRedraw = true;
}

void handleCommand(AppRenderer *appRenderer) {
    switch (appRenderer->currentCommand) {
        case COMMAND_EDITOR_MODE:
            appRenderer->previousCoordinate = appRenderer->currentCoordinate;
            appRenderer->currentCoordinate = COORDINATE_LEFT_EDITOR_GENERAL;
            break;

        case COMMAND_VISITOR_MODE:
            appRenderer->previousCoordinate = appRenderer->currentCoordinate;
            appRenderer->currentCoordinate = COORDINATE_LEFT_VISITOR_GENERAL;
            break;
    }

    appRenderer->needsRedraw = true;
}

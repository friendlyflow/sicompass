#include "view.h"
#include <string.h>

void handleKeys(EditorState *state, SDL_Event *event) {
    SDL_Keycode key = event->key.key;
    SDL_Keymod mod = event->key.mod;

    bool ctrl = (mod & SDL_KMOD_CTRL) != 0;
    bool shift = (mod & SDL_KMOD_SHIFT) != 0;
    bool alt = (mod & SDL_KMOD_ALT) != 0;

    // Tab
    if (!ctrl && !shift && !alt && key == SDLK_TAB) {
        handleTab(state);
    }
    // Ctrl+A or Enter in editor general mode
    else if (((ctrl && !shift && !alt && key == SDLK_A) ||
              (!ctrl && !shift && !alt && key == SDLK_RETURN)) &&
             state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        handleCtrlA(state, HISTORY_NONE);
    }
    // Ctrl+Shift+A in editor insert mode
    else if (ctrl && shift && !alt && key == SDLK_A &&
             state->currentCoordinate == COORDINATE_LEFT_EDITOR_INSERT) {
        handleEscape(state);
        handleCtrlA(state, HISTORY_NONE);
        handleA(state);
    }
    // Enter
    else if (!ctrl && !shift && !alt && key == SDLK_RETURN) {
        handleEnter(state, HISTORY_NONE);
    }
    // Ctrl+Enter
    else if (ctrl && !shift && !alt && key == SDLK_RETURN) {
        handleCtrlEnter(state, HISTORY_NONE);
    }
    // Ctrl+I in editor general mode
    else if (ctrl && !shift && !alt && key == SDLK_I &&
             state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        handleCtrlI(state, HISTORY_NONE);
    }
    // Ctrl+Shift+I in editor insert mode
    else if (ctrl && shift && !alt && key == SDLK_I &&
             state->currentCoordinate == COORDINATE_LEFT_EDITOR_INSERT) {
        handleEscape(state);
        handleCtrlI(state, HISTORY_NONE);
        handleI(state);
    }
    // Ctrl+D (delete)
    else if (ctrl && !shift && !alt && key == SDLK_D &&
             state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        handleDelete(state, HISTORY_NONE);
    }
    // Colon (command mode)
    else if (!ctrl && !shift && !alt && key == SDLK_SEMICOLON &&
             (shift || event->key.key == SDLK_COLON) &&
             state->currentCoordinate != COORDINATE_LEFT_VISITOR_INSERT &&
             state->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        handleColon(state);
    }
    // K or Up arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_K && (state->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
                                 state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL)) ||
              (key == SDLK_UP &&
               state->currentCoordinate != COORDINATE_LEFT_VISITOR_INSERT &&
               state->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT))) {
        handleUp(state);
    }
    // J or Down arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_J && (state->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
                                 state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL)) ||
              (key == SDLK_DOWN &&
               state->currentCoordinate != COORDINATE_LEFT_VISITOR_INSERT &&
               state->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT))) {
        handleDown(state);
    }
    // H or Left arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_H && (state->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
                                 state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL)) ||
              key == SDLK_LEFT)) {
        handleLeft(state);
    }
    // L or Right arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_L && (state->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
                                 state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL)) ||
              key == SDLK_RIGHT)) {
        handleRight(state);
    }
    // I (insert mode)
    else if (!ctrl && !shift && !alt && key == SDLK_I) {
        handleI(state);
    }
    // A (append mode)
    else if (!ctrl && !shift && !alt && key == SDLK_A) {
        handleA(state);
    }
    // Ctrl+Z (undo)
    else if (ctrl && !shift && !alt && key == SDLK_Z) {
        handleHistoryAction(state, HISTORY_UNDO);
    }
    // Ctrl+Shift+Z (redo)
    else if (ctrl && shift && !alt && key == SDLK_Z) {
        handleHistoryAction(state, HISTORY_REDO);
    }
    // Ctrl+X (cut)
    else if (ctrl && !shift && !alt && key == SDLK_X &&
             state->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT &&
             state->currentCoordinate != COORDINATE_LEFT_VISITOR_INSERT &&
             state->currentCoordinate != COORDINATE_LEFT_VISITOR_GENERAL) {
        handleCcp(state, TASK_CUT);
    }
    // Ctrl+C (copy)
    else if (ctrl && !shift && !alt && key == SDLK_C &&
             state->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT &&
             state->currentCoordinate != COORDINATE_LEFT_VISITOR_INSERT &&
             state->currentCoordinate != COORDINATE_LEFT_VISITOR_GENERAL) {
        handleCcp(state, TASK_COPY);
    }
    // Ctrl+V (paste)
    else if (ctrl && !shift && !alt && key == SDLK_V &&
             state->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT &&
             state->currentCoordinate != COORDINATE_LEFT_VISITOR_INSERT &&
             state->currentCoordinate != COORDINATE_LEFT_VISITOR_GENERAL) {
        handleCcp(state, TASK_PASTE);
    }
    // Ctrl+F (find)
    else if (ctrl && !shift && !alt && key == SDLK_F) {
        handleFind(state);
    }
    // Escape
    else if (!ctrl && !shift && !alt && key == SDLK_ESCAPE) {
        handleEscape(state);
    }
    // E (editor mode)
    else if (!ctrl && !shift && !alt && key == SDLK_E &&
             (state->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
              state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL)) {
        state->currentCommand = COMMAND_EDITOR_MODE;
        handleCommand(state);
    }
    // V (visitor mode)
    else if (!ctrl && !shift && !alt && key == SDLK_V &&
             (state->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
              state->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL)) {
        state->currentCommand = COMMAND_VISITOR_MODE;
        handleCommand(state);
    }
    // Backspace in insert modes
    else if (!ctrl && !shift && !alt && key == SDLK_BACKSPACE &&
             (state->currentCoordinate == COORDINATE_LEFT_EDITOR_INSERT ||
              state->currentCoordinate == COORDINATE_LEFT_VISITOR_INSERT ||
              state->currentCoordinate == COORDINATE_RIGHT_INFO ||
              state->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
              state->currentCoordinate == COORDINATE_RIGHT_FIND)) {
        if (state->inputBufferSize > 0) {
            state->inputBuffer[--state->inputBufferSize] = '\0';
            if (state->cursorPosition > 0) state->cursorPosition--;
            state->needsRedraw = true;
        }
    }
}

void handleInput(EditorState *state, const char *text) {
    if (!text) return;

    int len = strlen(text);
    if (state->inputBufferSize + len >= state->inputBufferCapacity) {
        // Resize buffer
        int newCapacity = state->inputBufferCapacity * 2;
        char *newBuffer = realloc(state->inputBuffer, newCapacity);
        if (!newBuffer) return;

        state->inputBuffer = newBuffer;
        state->inputBufferCapacity = newCapacity;
    }

    strcat(state->inputBuffer, text);
    state->inputBufferSize += len;
    state->cursorPosition += len;
    state->needsRedraw = true;
}

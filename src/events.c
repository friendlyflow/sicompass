#include "view.h"
#include <string.h>

void handleKeys(AppRenderer *appRenderer, SDL_Event *event) {
    SDL_Keycode key = event->key.key;
    SDL_Keymod mod = event->key.mod;

    bool ctrl = (mod & SDL_KMOD_CTRL) != 0;
    bool shift = (mod & SDL_KMOD_SHIFT) != 0;
    bool alt = (mod & SDL_KMOD_ALT) != 0;

    // Tab
    if (!ctrl && !shift && !alt && key == SDLK_TAB) {
        handleTab(appRenderer);
    }
    // Ctrl+A or Enter in editor general mode
    else if (((ctrl && !shift && !alt && key == SDLK_A) ||
              (!ctrl && !shift && !alt && key == SDLK_RETURN)) &&
             appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        handleCtrlA(appRenderer, HISTORY_NONE);
    }
    // Ctrl+Shift+A in editor insert mode
    else if (ctrl && shift && !alt && key == SDLK_A &&
             appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_INSERT) {
        handleEscape(appRenderer);
        handleCtrlA(appRenderer, HISTORY_NONE);
        handleA(appRenderer);
    }
    // Enter
    else if (!ctrl && !shift && !alt && key == SDLK_RETURN) {
        handleEnter(appRenderer, HISTORY_NONE);
    }
    // Ctrl+Enter
    else if (ctrl && !shift && !alt && key == SDLK_RETURN) {
        handleCtrlEnter(appRenderer, HISTORY_NONE);
    }
    // Ctrl+I in editor general mode
    else if (ctrl && !shift && !alt && key == SDLK_I &&
             appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        handleCtrlI(appRenderer, HISTORY_NONE);
    }
    // Ctrl+Shift+I in editor insert mode
    else if (ctrl && shift && !alt && key == SDLK_I &&
             appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_INSERT) {
        handleEscape(appRenderer);
        handleCtrlI(appRenderer, HISTORY_NONE);
        handleI(appRenderer);
    }
    // Ctrl+D (delete)
    else if (ctrl && !shift && !alt && key == SDLK_D &&
             appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL) {
        handleDelete(appRenderer, HISTORY_NONE);
    }
    // Colon (command mode)
    else if (!ctrl && !shift && !alt && key == SDLK_SEMICOLON &&
             (shift || event->key.key == SDLK_COLON) &&
             appRenderer->currentCoordinate != COORDINATE_LEFT_VISITOR_INSERT &&
             appRenderer->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT) {
        handleColon(appRenderer);
    }
    // K or Up arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_K && (appRenderer->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
                                 appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL)) ||
              (key == SDLK_UP &&
               appRenderer->currentCoordinate != COORDINATE_LEFT_VISITOR_INSERT &&
               appRenderer->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT))) {
        handleUp(appRenderer);
    }
    // J or Down arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_J && (appRenderer->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
                                 appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL)) ||
              (key == SDLK_DOWN &&
               appRenderer->currentCoordinate != COORDINATE_LEFT_VISITOR_INSERT &&
               appRenderer->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT))) {
        handleDown(appRenderer);
    }
    // H or Left arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_H && (appRenderer->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
                                 appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL)) ||
              key == SDLK_LEFT)) {
        handleLeft(appRenderer);
    }
    // L or Right arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_L && (appRenderer->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
                                 appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL)) ||
              key == SDLK_RIGHT)) {
        handleRight(appRenderer);
    }
    // I (insert mode)
    else if (!ctrl && !shift && !alt && key == SDLK_I) {
        handleI(appRenderer);
    }
    // A (append mode)
    else if (!ctrl && !shift && !alt && key == SDLK_A) {
        handleA(appRenderer);
    }
    // Ctrl+Z (undo)
    else if (ctrl && !shift && !alt && key == SDLK_Z) {
        handleHistoryAction(appRenderer, HISTORY_UNDO);
    }
    // Ctrl+Shift+Z (redo)
    else if (ctrl && shift && !alt && key == SDLK_Z) {
        handleHistoryAction(appRenderer, HISTORY_REDO);
    }
    // Ctrl+X (cut)
    else if (ctrl && !shift && !alt && key == SDLK_X &&
             appRenderer->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT &&
             appRenderer->currentCoordinate != COORDINATE_LEFT_VISITOR_INSERT &&
             appRenderer->currentCoordinate != COORDINATE_LEFT_VISITOR_GENERAL) {
        handleCcp(appRenderer, TASK_CUT);
    }
    // Ctrl+C (copy)
    else if (ctrl && !shift && !alt && key == SDLK_C &&
             appRenderer->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT &&
             appRenderer->currentCoordinate != COORDINATE_LEFT_VISITOR_INSERT &&
             appRenderer->currentCoordinate != COORDINATE_LEFT_VISITOR_GENERAL) {
        handleCcp(appRenderer, TASK_COPY);
    }
    // Ctrl+V (paste)
    else if (ctrl && !shift && !alt && key == SDLK_V &&
             appRenderer->currentCoordinate != COORDINATE_LEFT_EDITOR_INSERT &&
             appRenderer->currentCoordinate != COORDINATE_LEFT_VISITOR_INSERT &&
             appRenderer->currentCoordinate != COORDINATE_LEFT_VISITOR_GENERAL) {
        handleCcp(appRenderer, TASK_PASTE);
    }
    // Ctrl+F (find)
    else if (ctrl && !shift && !alt && key == SDLK_F) {
        handleFind(appRenderer);
    }
    // Escape
    else if (!ctrl && !shift && !alt && key == SDLK_ESCAPE) {
        handleEscape(appRenderer);
    }
    // E (editor mode)
    else if (!ctrl && !shift && !alt && key == SDLK_E &&
             (appRenderer->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
              appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL)) {
        appRenderer->currentCommand = COMMAND_EDITOR_MODE;
        handleCommand(appRenderer);
    }
    // V (visitor mode)
    else if (!ctrl && !shift && !alt && key == SDLK_V &&
             (appRenderer->currentCoordinate == COORDINATE_LEFT_VISITOR_GENERAL ||
              appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_GENERAL)) {
        appRenderer->currentCommand = COMMAND_VISITOR_MODE;
        handleCommand(appRenderer);
    }
    // Backspace in insert modes
    else if (!ctrl && !shift && !alt && key == SDLK_BACKSPACE &&
             (appRenderer->currentCoordinate == COORDINATE_LEFT_EDITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_LEFT_VISITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_RIGHT_INFO ||
              appRenderer->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_RIGHT_FIND)) {
        if (appRenderer->inputBufferSize > 0) {
            appRenderer->inputBuffer[--appRenderer->inputBufferSize] = '\0';
            if (appRenderer->cursorPosition > 0) appRenderer->cursorPosition--;

            // Update filter when backspacing in right panel modes
            if (appRenderer->currentCoordinate == COORDINATE_RIGHT_INFO ||
                appRenderer->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
                appRenderer->currentCoordinate == COORDINATE_RIGHT_FIND) {
                populateListRight(appRenderer, appRenderer->inputBuffer);
            }

            appRenderer->needsRedraw = true;
        }
    }
}

void handleInput(AppRenderer *appRenderer, const char *text) {
    if (!text) return;

    int len = strlen(text);
    if (appRenderer->inputBufferSize + len >= appRenderer->inputBufferCapacity) {
        // Resize buffer
        int newCapacity = appRenderer->inputBufferCapacity * 2;
        char *newBuffer = realloc(appRenderer->inputBuffer, newCapacity);
        if (!newBuffer) return;

        appRenderer->inputBuffer = newBuffer;
        appRenderer->inputBufferCapacity = newCapacity;
    }

    strcat(appRenderer->inputBuffer, text);
    appRenderer->inputBufferSize += len;
    appRenderer->cursorPosition += len;

    // Filter the list when in right panel modes
    if (appRenderer->currentCoordinate == COORDINATE_RIGHT_INFO ||
        appRenderer->currentCoordinate == COORDINATE_RIGHT_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_RIGHT_FIND) {
        populateListRight(appRenderer, appRenderer->inputBuffer);
    }

    appRenderer->needsRedraw = true;
}

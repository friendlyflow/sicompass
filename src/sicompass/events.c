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
    // Ctrl+A in text input modes - select all text
    else if (ctrl && !shift && !alt && key == SDLK_A &&
             (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH)) {
        handleSelectAll(appRenderer);
    }
    // Ctrl+A or Enter in editor general mode
    else if (((ctrl && !shift && !alt && key == SDLK_A) ||
              (!ctrl && !shift && !alt && key == SDLK_RETURN)) &&
             appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL) {
        handleCtrlA(appRenderer, HISTORY_NONE);
    }
    // Ctrl+Shift+A in editor insert mode
    else if (ctrl && shift && !alt && key == SDLK_A &&
             appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT) {
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
             appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL) {
        handleCtrlI(appRenderer, HISTORY_NONE);
    }
    // Ctrl+Shift+I in editor insert mode
    else if (ctrl && shift && !alt && key == SDLK_I &&
             appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT) {
        handleEscape(appRenderer);
        handleCtrlI(appRenderer, HISTORY_NONE);
        handleI(appRenderer);
    }
    // Ctrl+D (delete)
    else if (ctrl && !shift && !alt && key == SDLK_D &&
             appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL) {
        handleDelete(appRenderer, HISTORY_NONE);
    }
    // Colon (command mode)
    else if (!ctrl && !shift && !alt && key == SDLK_COLON &&
             appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT &&
             appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT) {
        handleColon(appRenderer);
    }
    // K or Up arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_K && (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
                                 appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL)) ||
              (key == SDLK_UP &&
               appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT &&
               appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT))) {
        handleUp(appRenderer);
    }
    // J or Down arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_J && (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
                                 appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL)) ||
              (key == SDLK_DOWN &&
               appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT &&
               appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT))) {
        handleDown(appRenderer);
    }
    // H or Left arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_H && (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
                                 appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL)) ||
              key == SDLK_LEFT)) {
        handleLeft(appRenderer);
    }
    // Shift+Left in text input modes - extend selection left
    else if (!ctrl && shift && !alt && key == SDLK_LEFT &&
             (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH)) {
        handleShiftLeft(appRenderer);
    }
    // L or Right arrow
    else if (!ctrl && !shift && !alt &&
             ((key == SDLK_L && (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
                                 appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL)) ||
              key == SDLK_RIGHT)) {
        handleRight(appRenderer);
    }
    // Shift+Right in text input modes - extend selection right
    else if (!ctrl && shift && !alt && key == SDLK_RIGHT &&
             (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH)) {
        handleShiftRight(appRenderer);
    }
    // Home in text input modes
    else if (!ctrl && !shift && !alt && key == SDLK_HOME &&
             (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH)) {
        handleHome(appRenderer);
    }
    // End in text input modes
    else if (!ctrl && !shift && !alt && key == SDLK_END &&
             (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH)) {
        handleEnd(appRenderer);
    }
    // Shift+Home in text input modes - extend selection to start
    else if (!ctrl && shift && !alt && key == SDLK_HOME &&
             (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH)) {
        handleShiftHome(appRenderer);
    }
    // Shift+End in text input modes - extend selection to end
    else if (!ctrl && shift && !alt && key == SDLK_END &&
             (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH)) {
        handleShiftEnd(appRenderer);
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
    // Ctrl+X (cut) - mode-aware: text in insert/search/command, elements in editor general
    else if (ctrl && !shift && !alt && key == SDLK_X) {
        handleCtrlX(appRenderer);
    }
    // Ctrl+C (copy) - mode-aware: text in insert/search/command, elements in editor general
    else if (ctrl && !shift && !alt && key == SDLK_C) {
        handleCtrlC(appRenderer);
    }
    // Ctrl+V (paste) - mode-aware: text in insert/search/command, elements in editor general
    else if (ctrl && !shift && !alt && key == SDLK_V) {
        handleCtrlV(appRenderer);
    }
    // Ctrl+F (find)
    else if (ctrl && !shift && !alt && key == SDLK_F) {
        handleCtrlF(appRenderer);
    }
    // Escape
    else if (!ctrl && !shift && !alt && key == SDLK_ESCAPE) {
        handleEscape(appRenderer);
    }
    // E (editor mode)
    else if (!ctrl && !shift && !alt && key == SDLK_E &&
             (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
              appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL)) {
        appRenderer->currentCommand = COMMAND_EDITOR_MODE;
        handleCommand(appRenderer);
    }
    // O (operator mode)
    else if (!ctrl && !shift && !alt && key == SDLK_O &&
             (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL ||
              appRenderer->currentCoordinate == COORDINATE_EDITOR_GENERAL)) {
        appRenderer->currentCommand = COMMAND_OPERATOR_MODE;
        handleCommand(appRenderer);
    }
    // Backspace in insert modes
    else if (!ctrl && !shift && !alt && key == SDLK_BACKSPACE &&
             (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH)) {
        // If there's an active selection, delete it
        if (hasSelection(appRenderer)) {
            deleteSelection(appRenderer);
            caretReset(appRenderer->caretState, SDL_GetTicks());

            // Update search when deleting selection in right panel modes
            if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
                appRenderer->currentCoordinate == COORDINATE_COMMAND ||
                appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
                populateListCurrentLayer(appRenderer, appRenderer->inputBuffer);
            }

            appRenderer->needsRedraw = true;
        } else if (appRenderer->inputBufferSize > 0 && appRenderer->cursorPosition > 0) {
            // Delete character before cursor
            memmove(&appRenderer->inputBuffer[appRenderer->cursorPosition - 1],
                   &appRenderer->inputBuffer[appRenderer->cursorPosition],
                   appRenderer->inputBufferSize - appRenderer->cursorPosition + 1);
            appRenderer->inputBufferSize--;
            appRenderer->cursorPosition--;

            // Reset caret to visible when user presses backspace
            uint64_t currentTime = SDL_GetTicks();
            caretReset(appRenderer->caretState, currentTime);

            // Update search when backspacing in right panel modes
            if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
                appRenderer->currentCoordinate == COORDINATE_COMMAND ||
                appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
                populateListCurrentLayer(appRenderer, appRenderer->inputBuffer);
            }

            appRenderer->needsRedraw = true;
        }
    }
    // Delete key in insert modes
    else if (!ctrl && !shift && !alt && key == SDLK_DELETE &&
             (appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
              appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
              appRenderer->currentCoordinate == COORDINATE_COMMAND ||
              appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH)) {
        // If there's an active selection, delete it
        if (hasSelection(appRenderer)) {
            deleteSelection(appRenderer);
            caretReset(appRenderer->caretState, SDL_GetTicks());

            // Update search when deleting selection in right panel modes
            if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
                appRenderer->currentCoordinate == COORDINATE_COMMAND ||
                appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
                populateListCurrentLayer(appRenderer, appRenderer->inputBuffer);
            }

            appRenderer->needsRedraw = true;
        } else if (appRenderer->cursorPosition < appRenderer->inputBufferSize) {
            // Delete character at cursor
            memmove(&appRenderer->inputBuffer[appRenderer->cursorPosition],
                   &appRenderer->inputBuffer[appRenderer->cursorPosition + 1],
                   appRenderer->inputBufferSize - appRenderer->cursorPosition);
            appRenderer->inputBufferSize--;

            // Reset caret to visible when user presses delete
            uint64_t currentTime = SDL_GetTicks();
            caretReset(appRenderer->caretState, currentTime);

            // Update search when deleting in right panel modes
            if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
                appRenderer->currentCoordinate == COORDINATE_COMMAND ||
                appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
                populateListCurrentLayer(appRenderer, appRenderer->inputBuffer);
            }

            appRenderer->needsRedraw = true;
        }
    }
}

void handleInput(AppRenderer *appRenderer, const char *text) {
    if (!text) return;

    // Ignore colon when entering COORDINATE_COMMAND mode (buffer is empty)
    if (appRenderer->currentCoordinate == COORDINATE_COMMAND &&
        appRenderer->inputBufferSize == 0 &&
        strcmp(text, ":") == 0) {
        return;
    }

    // Replace selected text if there's an active selection
    if (hasSelection(appRenderer)) {
        deleteSelection(appRenderer);
    }

    int len = strlen(text);
    if (appRenderer->inputBufferSize + len >= appRenderer->inputBufferCapacity) {
        // Resize buffer
        int newCapacity = appRenderer->inputBufferCapacity * 2;
        char *newBuffer = realloc(appRenderer->inputBuffer, newCapacity);
        if (!newBuffer) return;

        appRenderer->inputBuffer = newBuffer;
        appRenderer->inputBufferCapacity = newCapacity;
    }

    // Insert text at cursor position instead of appending
    memmove(&appRenderer->inputBuffer[appRenderer->cursorPosition + len],
           &appRenderer->inputBuffer[appRenderer->cursorPosition],
           appRenderer->inputBufferSize - appRenderer->cursorPosition + 1);
    memcpy(&appRenderer->inputBuffer[appRenderer->cursorPosition], text, len);
    appRenderer->inputBufferSize += len;
    appRenderer->cursorPosition += len;

    // Reset caret to visible when user types
    uint64_t currentTime = SDL_GetTicks();
    caretReset(appRenderer->caretState, currentTime);

    // Search the list when in right panel modes
    if (appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
        appRenderer->currentCoordinate == COORDINATE_COMMAND ||
        appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
        populateListCurrentLayer(appRenderer, appRenderer->inputBuffer);
    }

    appRenderer->needsRedraw = true;
}

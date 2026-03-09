#include "caret.h"
#include "view.h"
#include "text.h"
#include "rectangle.h"
#include <SDL3/SDL.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

// Default blink interval in milliseconds
#define DEFAULT_BLINK_INTERVAL 800

// Caret state structure
struct CaretState {
    bool visible;
    uint64_t lastBlinkTime;
    uint32_t blinkInterval;
};

CaretState* caretCreate() {
    CaretState* caret = calloc(1, sizeof(CaretState));
    if (!caret) return NULL;

    caret->visible = true;
    caret->lastBlinkTime = SDL_GetTicks();
    caret->blinkInterval = DEFAULT_BLINK_INTERVAL;

    return caret;
}

void caretDestroy(CaretState* caret) {
    if (caret) {
        free(caret);
    }
}

void caretUpdate(CaretState* caret, uint64_t currentTime) {
    if (currentTime - caret->lastBlinkTime >= caret->blinkInterval) {
        caret->visible = !caret->visible;
        caret->lastBlinkTime = currentTime;
    }
}

void caretReset(CaretState* caret, uint64_t currentTime) {
    caret->visible = true;
    caret->lastBlinkTime = currentTime;
}

void caretRender(SiCompassApplication* app, CaretState* caret,
                 const char* text, int x, int y, int baseX,
                 int cursorPosition, uint32_t color) {
    if (!caret || !app) {
        return;
    }

    if (!caret->visible) {
        return; // Don't render if caret is in invisible phase
    }

    // Get text scale
    float scale = getTextScale(app, FONT_SIZE_PT);
    float lineHeight = getLineHeight(app, scale, TEXT_PADDING);

    // Find cursor line and column by counting newlines before cursor position
    int cursorLine = 0;
    int lineStartOffset = 0;  // byte offset of the start of the cursor's line
    if (text) {
        int len = strlen(text);
        int pos = cursorPosition < len ? cursorPosition : len;
        for (int i = 0; i < pos; i++) {
            if (text[i] == '\n') {
                cursorLine++;
                lineStartOffset = i + 1;
            }
        }
    }

    // X origin: first line uses x (after prefix), subsequent lines use baseX
    int lineX = (cursorLine == 0) ? x : baseX;

    // Calculate the X position of the caret based on column within current line
    float caretX = (float)lineX;

    if (text && cursorPosition > 0) {
        int len = strlen(text);
        int pos = cursorPosition < len ? cursorPosition : len;
        int colLen = pos - lineStartOffset;

        if (colLen > 0) {
            char tempStr[MAX_LINE_LENGTH];
            if (colLen >= MAX_LINE_LENGTH) colLen = MAX_LINE_LENGTH - 1;
            strncpy(tempStr, text + lineStartOffset, colLen);
            tempStr[colLen] = '\0';

            float minX, minY, maxX, maxY;
            calculateTextBounds(app, tempStr, (float)lineX, (float)y, scale,
                              &minX, &minY, &maxX, &maxY);
            caretX = maxX;
        }
    }

    // Caret dimensions
    float caretWidth = 2.0f;
    float caretHeight = lineHeight - (2.0f * TEXT_PADDING);
    float caretY = (float)y + cursorLine * lineHeight;

    // Render caret as a thin rectangle with no corner radius
    prepareRectangle(app, caretX, caretY, caretWidth, caretHeight,
                    color, 0.0f);
}

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
                 const char* text, int x, int y, int cursorPosition,
                 uint32_t color) {
    if (!caret || !app) {
        return;
    }

    if (!caret->visible) {
        return; // Don't render if caret is in invisible phase
    }

    // Get text scale
    float scale = getTextScale(app, FONT_SIZE_PT);

    // Calculate the X position of the caret based on cursor position
    float caretX = (float)x;

    if (text && cursorPosition > 0) {
        int len = strlen(text);

        if (len > 0) {
            // Extract the substring up to the cursor position
            int pos = cursorPosition < len ? cursorPosition : len;

            // Create a temporary string with characters before cursor
            char tempStr[MAX_LINE_LENGTH];
            strncpy(tempStr, text, pos);
            tempStr[pos] = '\0';

            // Calculate bounds of text before cursor
            float minX, minY, maxX, maxY;
            calculateTextBounds(app, tempStr, (float)x, (float)y, scale,
                              &minX, &minY, &maxX, &maxY);

            // Position caret at the end of the text before cursor
            caretX = maxX;
        }
    }

    // Get line height for caret height
    float lineHeight = getLineHeight(app, scale, TEXT_PADDING);

    // Caret dimensions
    float caretWidth = 2.0f;
    float caretHeight = lineHeight - (2.0f * TEXT_PADDING);
    float caretY = (float)y;

    // Convert color from uint32_t to vec4
    vec4 caretColor;
    caretColor[0] = ((color >> 24) & 0xFF) / 255.0f;  // Red
    caretColor[1] = ((color >> 16) & 0xFF) / 255.0f;  // Green
    caretColor[2] = ((color >> 8) & 0xFF) / 255.0f;   // Blue
    caretColor[3] = (color & 0xFF) / 255.0f;          // Alpha

    // Render caret as a thin rectangle with no corner radius
    prepareRectangle(app, caretX, caretY, caretWidth, caretHeight,
                    caretColor, 0.0f);
}

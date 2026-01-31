#pragma once

#include "main.h"

// Forward declaration
typedef struct CaretState CaretState;

// Create and destroy caret
CaretState* caretCreate();
void caretDestroy(CaretState* caret);

// Update caret blink state based on current time
void caretUpdate(CaretState* caret, uint64_t currentTime);

// Reset caret to visible state (call when user types)
void caretReset(CaretState* caret, uint64_t currentTime);

// Render the caret at the specified position
void caretRender(SiCompassApplication* app, CaretState* caret,
                 const char* text, int x, int y, int cursorPosition,
                 uint32_t color);

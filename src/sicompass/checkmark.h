#pragma once

#include <stdint.h>

typedef struct SiCompassApplication SiCompassApplication;

// Render a checkmark (tick) shape at the given position.
// x, y: top-left corner of the bounding box
// size: width and height of the bounding square
// color: RGBA packed as uint32_t
void prepareCheckmark(SiCompassApplication *app,
                      float x, float y, float size, uint32_t color);

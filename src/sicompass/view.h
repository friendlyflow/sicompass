#pragma once

#include "app_state.h"
#include "main.h"

// AccessKit constants (defined in render.c)
extern const accesskit_node_id ELEMENT_ID;
extern const Sint32 SET_FOCUS_MSG;

// Main entry point
void mainLoop(SiCompassApplication* app);

// Initialization and cleanup
SiCompassApplication* appRendererCreate(SiCompassApplication* app);
void appRendererDestroy(AppRenderer *appRenderer);

// Rendering
void updateView(SiCompassApplication *app);
void renderSimpleSearch(SiCompassApplication *app);
void renderExtendedSearch(SiCompassApplication *app);
void renderScroll(SiCompassApplication *app);
void renderScrollSearch(SiCompassApplication *app);
void renderInteraction(SiCompassApplication *app);
void renderDashboard(SiCompassApplication *app);
void renderLine(SiCompassApplication *app, FfonElement *elem, const IdArray *id, int indent, int *yPos);
int renderText(SiCompassApplication *app, const char *text, int x, int y, uint32_t color, bool highlight);

// Caret rendering (requires SiCompassApplication for font metrics)
void caretRender(SiCompassApplication* app, CaretState* caret,
                 const char* text, int x, int y, int baseX,
                 int cursorPosition, uint32_t color);

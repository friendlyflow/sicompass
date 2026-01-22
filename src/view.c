#include "view.h"
#include "provider.h"
#include <filebrowser.h>
#include <string.h>

// Filebrowser provider callback
static FfonElement** filebrowserFetchCallback(AppRenderer *appRenderer, const char *parent_key, int *out_count) {
    (void)parent_key;  // We use currentUri instead
    // currentUri already has the full path built by providerUriAppend
    return filebrowserListDirectory(appRenderer->currentUri, false, out_count);
}

// Filebrowser handleI callback - extract content from <input> tags for editing
static void filebrowserHandleI(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate != COORDINATE_EDITOR_GENERAL &&
        appRenderer->currentCoordinate != COORDINATE_OPERATOR_GENERAL) {
        return;
    }

    idArrayCopy(&appRenderer->currentInsertId, &appRenderer->currentId);
    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    appRenderer->currentCoordinate = (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) ?
        COORDINATE_OPERATOR_INSERT : COORDINATE_EDITOR_INSERT;

    // Clear the input buffer first
    appRenderer->inputBuffer[0] = '\0';
    appRenderer->inputBufferSize = 0;

    // Get current line content
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
    if (arr && count > 0) {
        int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        if (idx >= 0 && idx < count) {
            FfonElement *elem = arr[idx];
            const char *text = (elem->type == FFON_STRING) ?
                elem->data.string : elem->data.object->key;

            // Extract content from <input> tags
            char *content = filebrowserExtractInputContent(text);
            if (content) {
                strncpy(appRenderer->inputBuffer, content,
                       appRenderer->inputBufferCapacity - 1);
                appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
                free(content);
            } else {
                // No tags, use as-is
                strncpy(appRenderer->inputBuffer, text,
                       appRenderer->inputBufferCapacity - 1);
                appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
            }
        }
    }

    appRenderer->cursorPosition = 0;
    idArrayInit(&appRenderer->currentInsertId);
    appRenderer->needsRedraw = true;
}

// Filebrowser handleA callback - same as handleI but cursor at end
static void filebrowserHandleA(AppRenderer *appRenderer) {
    if (appRenderer->currentCoordinate != COORDINATE_EDITOR_GENERAL &&
        appRenderer->currentCoordinate != COORDINATE_OPERATOR_GENERAL) {
        return;
    }

    idArrayCopy(&appRenderer->currentInsertId, &appRenderer->currentId);
    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    appRenderer->currentCoordinate = (appRenderer->currentCoordinate == COORDINATE_OPERATOR_GENERAL) ?
        COORDINATE_OPERATOR_INSERT : COORDINATE_EDITOR_INSERT;

    // Clear the input buffer first
    appRenderer->inputBuffer[0] = '\0';
    appRenderer->inputBufferSize = 0;

    // Get current line content
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
    if (arr && count > 0) {
        int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
        if (idx >= 0 && idx < count) {
            FfonElement *elem = arr[idx];
            const char *text = (elem->type == FFON_STRING) ?
                elem->data.string : elem->data.object->key;

            // Extract content from <input> tags
            char *content = filebrowserExtractInputContent(text);
            if (content) {
                strncpy(appRenderer->inputBuffer, content,
                       appRenderer->inputBufferCapacity - 1);
                appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
                free(content);
            } else {
                // No tags, use as-is
                strncpy(appRenderer->inputBuffer, text,
                       appRenderer->inputBufferCapacity - 1);
                appRenderer->inputBufferSize = strlen(appRenderer->inputBuffer);
            }
        }
    }

    appRenderer->cursorPosition = appRenderer->inputBufferSize;
    idArrayInit(&appRenderer->currentInsertId);
    appRenderer->needsRedraw = true;
}

// Filebrowser handleEscape callback - rename file on disk and update element
static void filebrowserHandleEscape(AppRenderer *appRenderer) {
    printf("filebrowserHandleEscape: currentCoordinate=%d\n", appRenderer->currentCoordinate);

    if (appRenderer->currentCoordinate != COORDINATE_EDITOR_INSERT &&
        appRenderer->currentCoordinate != COORDINATE_OPERATOR_INSERT) {
        printf("filebrowserHandleEscape: not in insert mode, returning\n");
        return;
    }

    // Get current element
    int count;
    FfonElement **arr = getFfonAtId(appRenderer->ffon, appRenderer->ffonCount, &appRenderer->currentId, &count);
    if (!arr || count == 0) {
        printf("filebrowserHandleEscape: no elements found\n");
        goto cleanup;
    }

    int idx = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    if (idx < 0 || idx >= count) {
        printf("filebrowserHandleEscape: invalid index %d (count=%d)\n", idx, count);
        goto cleanup;
    }

    FfonElement *elem = arr[idx];
    const char *originalText = (elem->type == FFON_STRING) ?
        elem->data.string : elem->data.object->key;
    printf("filebrowserHandleEscape: originalText='%s'\n", originalText);

    // Check if this element has input tags (filebrowser element)
    if (!filebrowserHasInputTags(originalText)) {
        printf("filebrowserHandleEscape: no input tags found\n");
        goto cleanup;
    }

    // Extract old name from tags
    char *oldName = filebrowserExtractInputContent(originalText);
    if (!oldName) {
        printf("filebrowserHandleEscape: failed to extract old name\n");
        goto cleanup;
    }

    // Get new name from input buffer
    const char *newName = appRenderer->inputBuffer;
    printf("filebrowserHandleEscape: oldName='%s', newName='%s', uri='%s'\n",
           oldName, newName, appRenderer->currentUri);

    // Only rename if name changed
    if (strcmp(oldName, newName) != 0) {
        printf("filebrowserHandleEscape: attempting rename\n");
        // Perform the rename on disk
        if (filebrowserRename(appRenderer->currentUri, oldName, newName)) {
            printf("filebrowserHandleEscape: rename succeeded\n");
            // Update the element with new name wrapped in tags
            char newText[512];
            snprintf(newText, sizeof(newText), "<input>%s</input>", newName);

            if (elem->type == FFON_STRING) {
                free(elem->data.string);
                elem->data.string = strdup(newText);
            } else {
                free(elem->data.object->key);
                elem->data.object->key = strdup(newText);
            }
        } else {
            printf("filebrowserHandleEscape: rename failed\n");
        }
    } else {
        printf("filebrowserHandleEscape: name unchanged\n");
    }

    free(oldName);

cleanup:
    // Return to general mode
    if (appRenderer->previousCoordinate == COORDINATE_OPERATOR_GENERAL ||
        appRenderer->previousCoordinate == COORDINATE_OPERATOR_INSERT) {
        appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    } else {
        appRenderer->currentCoordinate = COORDINATE_EDITOR_GENERAL;
    }

    appRenderer->previousCoordinate = appRenderer->currentCoordinate;
    createListCurrentLayer(appRenderer);
    // Sync listIndex with current position (createListCurrentLayer resets it to 0)
    appRenderer->listIndex = appRenderer->currentId.ids[appRenderer->currentId.depth - 1];
    appRenderer->needsRedraw = true;
}

void mainLoop(SiCompassApplication* app) {
    // Initialize app renderer
    app = appRendererCreate(app);
    if (!app->appRenderer) {
        fprintf(stderr, "Failed to create editor state\n");
        return;
    }

    // Set up filebrowser provider
    providerSetFetchCallback(filebrowserFetchCallback);
    providerSetHandleICallback(filebrowserHandleI);
    providerSetHandleACallback(filebrowserHandleA);
    providerSetHandleEscapeCallback(filebrowserHandleEscape);

    // Initialize URI to root and load initial directory listing
    strncpy(app->appRenderer->currentUri, "/", MAX_URI_LENGTH);
    int count = 0;
    FfonElement **elements = filebrowserListDirectory("/", false, &count);
    if (!elements || count == 0) {
        fprintf(stderr, "Failed to load directory listing for /\n");
        if (elements) free(elements);
        appRendererDestroy(app->appRenderer);
        return;
    }

    // Create top-level "file browser" object containing the file system
    FfonElement *fileBrowserElement = ffonElementCreateObject("file browser");
    FfonObject *fileBrowserObj = fileBrowserElement->data.object;
    for (int i = 0; i < count; i++) {
        ffonObjectAddElement(fileBrowserObj, elements[i]);
    }
    free(elements);  // Free the array, elements are now owned by fileBrowserObj

    // Create root array with just the file browser object
    app->appRenderer->ffon = malloc(sizeof(FfonElement*));
    app->appRenderer->ffon[0] = fileBrowserElement;
    app->appRenderer->ffonCount = 1;
    app->appRenderer->ffonCapacity = 1;

    // Initialize current_id - start at "file browser" object
    idArrayInit(&app->appRenderer->currentId);
    idArrayPush(&app->appRenderer->currentId, 0);

    // Set initial coordinate
    app->appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    app->appRenderer->previousCoordinate = COORDINATE_OPERATOR_GENERAL;

    // Initialize list for initial coordinate
    createListCurrentLayer(app->appRenderer);

    // Initial render
    app->appRenderer->needsRedraw = true;
    updateView(app);

    // Main event loop
    SDL_Event event;
    while (app->running) {
        while (SDL_PollEvent(&event)) {
            switch (event.type) {
                case SDL_EVENT_QUIT:
                    app->running = false;
                    break;

                case SDL_EVENT_KEY_DOWN:
                    handleKeys(app->appRenderer, &event);
                    // Enable/disable text input based on current mode
                    if (app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
                        app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
                        app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
                        SDL_StartTextInput(app->window);
                    } else {
                        SDL_StopTextInput(app->window);
                    }
                    break;

                case SDL_EVENT_TEXT_INPUT:
                    if (app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
                        app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
                        app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
                        app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
                        handleInput(app->appRenderer, event.text.text);
                    }
                    break;

                case SDL_EVENT_WINDOW_RESIZED:
                case SDL_EVENT_WINDOW_MAXIMIZED:
                case SDL_EVENT_WINDOW_EXPOSED:
                    app->framebufferResized = true;
                    app->appRenderer->needsRedraw = true;
                    break;
            }
        }

        // Update caret blink state
        uint64_t currentTime = SDL_GetTicks();
        caretUpdate(app->appRenderer->caretState, currentTime);

        // Caret blinking requires continuous redraw
        if (app->appRenderer->currentCoordinate == COORDINATE_OPERATOR_INSERT ||
            app->appRenderer->currentCoordinate == COORDINATE_EDITOR_INSERT ||
            app->appRenderer->currentCoordinate == COORDINATE_SIMPLE_SEARCH ||
            app->appRenderer->currentCoordinate == COORDINATE_COMMAND ||
            app->appRenderer->currentCoordinate == COORDINATE_EXTENDED_SEARCH) {
            app->appRenderer->needsRedraw = true;
        }

        // Recreate swapchain if framebuffer was resized
        if (app->framebufferResized) {
            app->framebufferResized = false;
            recreateSwapChain(app);
            app->appRenderer->needsRedraw = true;
        }

        // Render if needed
        if (app->appRenderer->needsRedraw) {
            updateView(app);
            app->appRenderer->needsRedraw = false;

            drawFrame(app);
        }

        SDL_Delay(16); // ~60 FPS
    }

    // Cleanup
    appRendererDestroy(app->appRenderer);
}
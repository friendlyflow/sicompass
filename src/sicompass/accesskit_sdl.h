#pragma once

#include <accesskit.h>
#include <SDL3/SDL.h>
#include <stdbool.h>

// SDL adapter struct - wraps platform-specific adapters
struct accesskit_sdl_adapter {
#if defined(__APPLE__)
    accesskit_macos_subclassing_adapter *adapter;
#elif defined(_WIN32)
    accesskit_windows_subclassing_adapter *adapter;
#else
    accesskit_unix_adapter *adapter;
#endif
};

// Callback types
typedef accesskit_tree_update *(*accesskit_activation_handler_callback)(void *);
typedef void (*accesskit_action_handler_callback)(accesskit_action_request *, void *);
typedef void (*accesskit_deactivation_handler_callback)(void *);
typedef accesskit_tree_update *(*accesskit_tree_update_factory)(void *);

// Initialize the SDL adapter
void accesskit_sdl_adapter_init(
    struct accesskit_sdl_adapter *adapter,
    SDL_Window *window,
    accesskit_activation_handler_callback activation_handler,
    void *activation_handler_userdata,
    accesskit_action_handler_callback action_handler,
    void *action_handler_userdata,
    accesskit_deactivation_handler_callback deactivation_handler,
    void *deactivation_handler_userdata);

// Destroy the SDL adapter
void accesskit_sdl_adapter_destroy(struct accesskit_sdl_adapter *adapter);

// Update the accessibility tree if active
void accesskit_sdl_adapter_update_if_active(
    struct accesskit_sdl_adapter *adapter,
    accesskit_tree_update_factory update_factory,
    void *update_factory_userdata);

// Update window focus state
void accesskit_sdl_adapter_update_window_focus_state(
    struct accesskit_sdl_adapter *adapter,
    bool is_focused);

// Update root window bounds (call when window moves or resizes)
void accesskit_sdl_adapter_update_root_window_bounds(
    struct accesskit_sdl_adapter *adapter,
    SDL_Window *window);

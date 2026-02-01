#include "accesskit_sdl.h"

#if defined(__APPLE__)
#include <SDL3/SDL_syswm.h>
#elif defined(_WIN32)
#include <SDL3/SDL_syswm.h>
#endif

void accesskit_sdl_adapter_init(
    struct accesskit_sdl_adapter *adapter,
    SDL_Window *window,
    accesskit_activation_handler_callback activation_handler,
    void *activation_handler_userdata,
    accesskit_action_handler_callback action_handler,
    void *action_handler_userdata,
    accesskit_deactivation_handler_callback deactivation_handler,
    void *deactivation_handler_userdata) {

#if defined(__APPLE__)
    // macOS: Get NSView from SDL window
    SDL_PropertiesID props = SDL_GetWindowProperties(window);
    void *ns_view = SDL_GetPointerProperty(props, SDL_PROP_WINDOW_COCOA_WINDOW_POINTER, NULL);
    if (ns_view) {
        adapter->adapter = accesskit_macos_subclassing_adapter_new(
            ns_view,
            false, // is_view (false = window)
            activation_handler,
            activation_handler_userdata,
            action_handler,
            action_handler_userdata);
    } else {
        adapter->adapter = NULL;
    }
#elif defined(_WIN32)
    // Windows: Get HWND from SDL window
    SDL_PropertiesID props = SDL_GetWindowProperties(window);
    void *hwnd = SDL_GetPointerProperty(props, SDL_PROP_WINDOW_WIN32_HWND_POINTER, NULL);
    if (hwnd) {
        adapter->adapter = accesskit_windows_subclassing_adapter_new(
            hwnd,
            activation_handler,
            activation_handler_userdata,
            action_handler,
            action_handler_userdata);
    } else {
        adapter->adapter = NULL;
    }
#else
    // Unix/Linux: AT-SPI doesn't need window handle
    (void)window;
    adapter->adapter = accesskit_unix_adapter_new(
        activation_handler,
        activation_handler_userdata,
        action_handler,
        action_handler_userdata,
        deactivation_handler,
        deactivation_handler_userdata);
#endif
}

void accesskit_sdl_adapter_destroy(struct accesskit_sdl_adapter *adapter) {
    if (!adapter->adapter) {
        return;
    }

#if defined(__APPLE__)
    accesskit_macos_subclassing_adapter_free(adapter->adapter);
#elif defined(_WIN32)
    accesskit_windows_subclassing_adapter_free(adapter->adapter);
#else
    accesskit_unix_adapter_free(adapter->adapter);
#endif
    adapter->adapter = NULL;
}

void accesskit_sdl_adapter_update_if_active(
    struct accesskit_sdl_adapter *adapter,
    accesskit_tree_update_factory update_factory,
    void *update_factory_userdata) {

    if (!adapter->adapter) {
        return;
    }

#if defined(__APPLE__)
    accesskit_macos_queued_events *events =
        accesskit_macos_subclassing_adapter_update_if_active(
            adapter->adapter, update_factory, update_factory_userdata);
    if (events) {
        accesskit_macos_queued_events_raise(events);
    }
#elif defined(_WIN32)
    accesskit_windows_queued_events *events =
        accesskit_windows_subclassing_adapter_update_if_active(
            adapter->adapter, update_factory, update_factory_userdata);
    if (events) {
        accesskit_windows_queued_events_raise(events);
    }
#else
    accesskit_unix_adapter_update_if_active(
        adapter->adapter, update_factory, update_factory_userdata);
#endif
}

void accesskit_sdl_adapter_update_window_focus_state(
    struct accesskit_sdl_adapter *adapter,
    bool is_focused) {

    if (!adapter->adapter) {
        return;
    }

#if defined(__APPLE__)
    accesskit_macos_queued_events *events =
        accesskit_macos_subclassing_adapter_update_view_focus_state(
            adapter->adapter, is_focused);
    if (events) {
        accesskit_macos_queued_events_raise(events);
    }
#elif defined(_WIN32)
    // Windows doesn't have a direct focus state update
    (void)is_focused;
#else
    accesskit_unix_adapter_update_window_focus_state(adapter->adapter, is_focused);
#endif
}

void accesskit_sdl_adapter_update_root_window_bounds(
    struct accesskit_sdl_adapter *adapter,
    SDL_Window *window) {

    if (!adapter->adapter) {
        return;
    }

#if defined(__APPLE__)
    // macOS handles this automatically with subclassing adapter
    (void)window;
#elif defined(_WIN32)
    // Windows handles this automatically with subclassing adapter
    (void)window;
#else
    // Unix: Update window bounds for AT-SPI
    int x, y, w, h;
    SDL_GetWindowPosition(window, &x, &y);
    SDL_GetWindowSize(window, &w, &h);

    accesskit_rect outer = {
        .x0 = (double)x,
        .y0 = (double)y,
        .x1 = (double)(x + w),
        .y1 = (double)(y + h)
    };

    // Inner bounds (client area) - same as outer for now
    accesskit_rect inner = outer;

    accesskit_unix_adapter_set_root_window_bounds(adapter->adapter, outer, inner);
#endif
}

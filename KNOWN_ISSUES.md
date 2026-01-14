# Known Issues

## Window Maximize Not Working (Cinnamon Desktop)

**Issue**: The maximize button does not properly resize the Vulkan rendering surface on Cinnamon desktop environment.

**Symptoms**:
- Clicking the maximize button visually maximizes the window frame
- The Vulkan content remains at 800x600 and does not fill the maximized window
- Manual window resizing by dragging corners also does not work

**Root Cause**: This is a bug in SDL3's interaction with the Cinnamon window manager when using `SDL_WINDOW_VULKAN` flag. SDL does not receive proper resize events from the window manager, and the Vulkan surface size stays fixed.

**Workarounds**:
1. Use a different desktop environment (GNOME, KDE, etc.)
2. Use Wayland instead of X11
3. Wait for SDL3 to fix this issue

**References**:
- SDL version: SDL3 (pre-release)
- Confirmed on: Linux Mint Cinnamon with X11
- Related to depth/stencil validation fixes in rectangle.c and text.c

## Fixed Issues

### Vulkan Validation Errors
**Fixed**: Added `VkPipelineDepthStencilStateCreateInfo` to rectangle and text pipelines to satisfy render pass requirements.

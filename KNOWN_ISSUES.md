# Known Issues

## macOS downloads are not notarized

**Issue**: macOS refuses to open the `.dmg` on first launch and reports that
the application is damaged.

**Root Cause**: The app is ad-hoc signed but not notarized by Apple, which
needs a paid Developer ID. Gatekeeper quarantines anything downloaded from a
browser that it cannot verify.

**Workaround**: clear the quarantine attribute once after installing.

```bash
xattr -dr com.apple.quarantine /Applications/sicompass.app
```

**Fix**: obtain an Apple Developer ID and set `macos.signing-identity`,
`signing-certificate`, `signing-certificate-password` and
`notarization-credentials` for cargo-packager. All four are secrets, not
config-file values.

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

### macOS rendering and VoiceOver are now verified

**Fixed**: sicompass has been run on real Mac hardware. It renders through
MoltenVK, and VoiceOver speaks the list, mode changes, the typed-character
echo, and the `w` position report. The AccessKit Cocoa adapter needed a
macOS-specific tree to get there, see `build_tree_macos` in
`src/sicompass/src/accesskit_sdl.rs`.

### Vulkan Validation Errors
**Fixed**: Added `VkPipelineDepthStencilStateCreateInfo` to rectangle and text pipelines to satisfy render pass requirements.

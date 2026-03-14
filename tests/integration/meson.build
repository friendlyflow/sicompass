# Integration tests — link real logic code + real provider libraries
# against a headless harness (no Vulkan, no windowing, no accesskit)

integration_test_inc = include_directories(
  '../../src/sicompass',
  '../../external',
  '../../external/fff',
)

# Real source files from src/sicompass/ (logic, no rendering)
integration_sources = files(
  '../../src/sicompass/handlers.c',
  '../../src/sicompass/events.c',
  '../../src/sicompass/update.c',
  '../../src/sicompass/list.c',
  '../../src/sicompass/provider.c',
  '../../src/sicompass/unicode_search.c',
)

test_integration = executable('test_integration',
  'test_integration.c',
  'harness.c',
  integration_sources,
  include_directories: integration_test_inc,
  dependencies: [
    unity_dep,
    sdl3_dep,
    xkbcommon_dep,
    jsonc_dep,
    accesskit_dep,
    utf8proc_dep,
    lib_ffon_dep,
    lib_provider_dep,
    lib_filebrowser_dep,
    lib_settings_dep,
  ],
  link_args: [
    '-Wl,--wrap=SDL_GetClipboardText',
    '-Wl,--wrap=SDL_SetClipboardText',
    '-Wl,--wrap=SDL_HasClipboardText',
    '-Wl,--wrap=SDL_free',
    '-Wl,--wrap=platformOpenWithDefault',
  ],
)

test('integration', test_integration)

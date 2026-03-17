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

integration_deps = [
  unity_dep,
  sdl3_dep,
  jsonc_dep,
  accesskit_dep,
  utf8proc_dep,
  lib_ffon_dep,
  lib_provider_dep,
  lib_filebrowser_dep,
  lib_settings_dep,
]

integration_link_args = []

if host_machine.system() == 'linux'
  integration_deps += [xkbcommon_dep]
  integration_link_args += [
    '-Wl,--wrap=SDL_GetClipboardText',
    '-Wl,--wrap=SDL_SetClipboardText',
    '-Wl,--wrap=SDL_HasClipboardText',
    '-Wl,--wrap=SDL_free',
    '-Wl,--wrap=platformOpenWithDefault',
  ]
endif

test_integration = executable('test_integration',
  'test_integration.c',
  'harness.c',
  integration_sources,
  include_directories: integration_test_inc,
  dependencies: integration_deps,
  link_args: integration_link_args,
)

test('integration', test_integration)

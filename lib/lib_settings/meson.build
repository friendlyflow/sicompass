lib_settings_src = files('src/settings_provider.c')
lib_settings_inc = include_directories('include')

lib_settings = library('settings',
  lib_settings_src,
  include_directories: lib_settings_inc,
  dependencies: [lib_ffon_dep, lib_provider_dep, jsonc_dep],
  install: true,
)

lib_settings_dep = declare_dependency(
  link_with: lib_settings,
  include_directories: lib_settings_inc,
  dependencies: [lib_ffon_dep, lib_provider_dep],
)

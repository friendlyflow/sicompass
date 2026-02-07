lib_provider_src = files('src/config.c', 'src/provider.c', 'src/platform.c')
lib_provider_inc = include_directories('include')

# Windows needs shell32 for ShellExecuteA
if host_machine.system() == 'windows'
  shell32_dep = meson.get_compiler('c').find_library('shell32')
  lib_provider_platform_deps = [shell32_dep]
else
  lib_provider_platform_deps = []
endif

lib_provider = library('provider',
  lib_provider_src,
  include_directories: lib_provider_inc,
  dependencies: [lib_ffon_dep, jsonc_dep] + lib_provider_platform_deps,
  install: true,
)

lib_provider_dep = declare_dependency(
  link_with: lib_provider,
  include_directories: lib_provider_inc,
  dependencies: [lib_ffon_dep, jsonc_dep],
)

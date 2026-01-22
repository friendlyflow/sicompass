lib_provider_src = files('src/config.c', 'src/provider.c')
lib_provider_inc = include_directories('include')

lib_provider = library('provider',
  lib_provider_src,
  include_directories: lib_provider_inc,
  dependencies: [lib_ffon_dep],
  install: true,
)

lib_provider_dep = declare_dependency(
  link_with: lib_provider,
  include_directories: lib_provider_inc,
  dependencies: [lib_ffon_dep],
)

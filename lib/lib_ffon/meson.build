lib_ffon_src = files('src/ffon.c')
lib_ffon_inc = include_directories('include')

lib_ffon = library('ffon',
  lib_ffon_src,
  include_directories: lib_ffon_inc,
  install: true,
)

lib_ffon_dep = declare_dependency(
  link_with: lib_ffon,
  include_directories: lib_ffon_inc,
)

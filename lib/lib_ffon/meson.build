lib_ffon_src = files('src/ffon.c')
lib_ffon_inc = include_directories('include')

json_c_dep = dependency('json-c')

lib_ffon = library('ffon',
  lib_ffon_src,
  include_directories: lib_ffon_inc,
  dependencies: [json_c_dep],
  install: true,
)

lib_ffon_dep = declare_dependency(
  link_with: lib_ffon,
  include_directories: lib_ffon_inc,
  dependencies: [json_c_dep],
)

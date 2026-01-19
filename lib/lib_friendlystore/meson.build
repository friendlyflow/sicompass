lib_friendlystore_src = files('src/friendlystore.c')
lib_friendlystore_inc = include_directories('include')

lib_friendlystore = library('friendlystore',
  lib_friendlystore_src,
  include_directories: lib_friendlystore_inc,
  install: true,
)

lib_friendlystore_dep = declare_dependency(
  link_with: lib_friendlystore,
  include_directories: lib_friendlystore_inc,
)

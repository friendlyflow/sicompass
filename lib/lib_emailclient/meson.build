lib_emailclient_src = files('src/emailclient.c')
lib_emailclient_inc = include_directories('include')

lib_emailclient = library('emailclient',
  lib_emailclient_src,
  include_directories: lib_emailclient_inc,
  install: true,
)

lib_emailclient_dep = declare_dependency(
  link_with: lib_emailclient,
  include_directories: lib_emailclient_inc,
)

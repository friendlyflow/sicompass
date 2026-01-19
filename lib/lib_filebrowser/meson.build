lib_filebrowser_src = files('src/filebrowser.c')
lib_filebrowser_inc = include_directories('include')

lib_filebrowser = library('filebrowser',
  lib_filebrowser_src,
  include_directories: lib_filebrowser_inc,
  install: true,
)

lib_filebrowser_dep = declare_dependency(
  link_with: lib_filebrowser,
  include_directories: lib_filebrowser_inc,
)

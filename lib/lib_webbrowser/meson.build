lib_webbrowser_src = files('src/webbrowser.c')
lib_webbrowser_inc = include_directories('include')

lib_webbrowser = library('webbrowser',
  lib_webbrowser_src,
  include_directories: lib_webbrowser_inc,
  install: true,
)

lib_webbrowser_dep = declare_dependency(
  link_with: lib_webbrowser,
  include_directories: lib_webbrowser_inc,
)

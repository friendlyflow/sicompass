lib_chatclient_src = files('src/chatclient.c')
lib_chatclient_inc = include_directories('include')

lib_chatclient = library('chatclient',
  lib_chatclient_src,
  include_directories: lib_chatclient_inc,
  install: true,
)

lib_chatclient_dep = declare_dependency(
  link_with: lib_chatclient,
  include_directories: lib_chatclient_inc,
)

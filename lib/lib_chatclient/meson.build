lib_chatclient_src = files('src/chatclient.c', 'src/provider.c')
lib_chatclient_inc = include_directories('include', '../../external')

curl_dep = dependency('libcurl')

lib_chatclient = library('chatclient',
  lib_chatclient_src,
  include_directories: lib_chatclient_inc,
  dependencies: [lib_ffon_dep, lib_provider_dep, jsonc_dep, curl_dep],
  install: true,
)

lib_chatclient_dep = declare_dependency(
  link_whole: lib_chatclient,
  include_directories: lib_chatclient_inc,
  dependencies: [lib_ffon_dep, lib_provider_dep, jsonc_dep, curl_dep],
)

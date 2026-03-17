lib_webbrowser_src = files('src/webbrowser.c', 'src/provider.c')
lib_webbrowser_inc = include_directories('include', '../../external')

lexbor_dep = dependency('lexbor')
webbrowser_curl_dep = dependency('libcurl')

lib_webbrowser = library('webbrowser',
  lib_webbrowser_src,
  include_directories: lib_webbrowser_inc,
  dependencies: [lib_ffon_dep, lib_provider_dep, jsonc_dep, webbrowser_curl_dep, lexbor_dep],
  install: true,
)

lib_webbrowser_dep = declare_dependency(
  link_whole: lib_webbrowser,
  include_directories: lib_webbrowser_inc,
  dependencies: [lib_ffon_dep, lib_provider_dep, jsonc_dep, webbrowser_curl_dep, lexbor_dep],
)

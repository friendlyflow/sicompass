lib_emailclient_src = files('src/emailclient.c', 'src/provider.c', 'src/oauth2.c')
lib_emailclient_inc = include_directories('include', '../../external')

curl_dep = dependency('libcurl')

lib_emailclient = library('emailclient',
  lib_emailclient_src,
  include_directories: lib_emailclient_inc,
  dependencies: [lib_ffon_dep, lib_provider_dep, jsonc_dep, curl_dep],
  install: true,
)

lib_emailclient_dep = declare_dependency(
  link_whole: lib_emailclient,
  include_directories: lib_emailclient_inc,
  dependencies: [lib_ffon_dep, lib_provider_dep, jsonc_dep, curl_dep],
)

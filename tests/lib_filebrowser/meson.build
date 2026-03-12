# lib_filebrowser unit tests

filebrowser_test_inc = include_directories('../../external', '../../external/fff')

test_filebrowser = executable('test_filebrowser',
  'test_filebrowser.c',
  include_directories: filebrowser_test_inc,
  dependencies: [unity_dep, lib_filebrowser_dep],
)
test('filebrowser', test_filebrowser)

test_filebrowser_provider = executable('test_filebrowser_provider',
  'test_filebrowser_provider.c',
  include_directories: filebrowser_test_inc,
  dependencies: [unity_dep, lib_filebrowser_dep],
  link_args: [
    '-Wl,--wrap=platformGetApplications',
    '-Wl,--wrap=platformFreeApplications',
    '-Wl,--wrap=platformOpenWith',
  ],
)
test('filebrowser_provider', test_filebrowser_provider)

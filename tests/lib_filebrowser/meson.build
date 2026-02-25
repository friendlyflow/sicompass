# lib_filebrowser unit tests

filebrowser_test_inc = include_directories('../../external', '../../external/fff')

test_filebrowser = executable('test_filebrowser',
  'test_filebrowser.c',
  include_directories: filebrowser_test_inc,
  dependencies: [unity_dep, lib_filebrowser_dep],
)
test('filebrowser', test_filebrowser)

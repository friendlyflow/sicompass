webbrowser_test_inc = include_directories('../../external', '../../external/fff')

test_webbrowser = executable('test_webbrowser',
  'test_webbrowser.c',
  include_directories: webbrowser_test_inc,
  dependencies: [unity_dep, lib_webbrowser_dep],
)
test('webbrowser', test_webbrowser)

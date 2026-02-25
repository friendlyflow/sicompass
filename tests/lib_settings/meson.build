# lib_settings unit tests

settings_test_inc = include_directories('../../external', '../../external/fff')

test_settings = executable('test_settings',
  'test_settings.c',
  include_directories: settings_test_inc,
  dependencies: [unity_dep, lib_settings_dep],
)
test('settings', test_settings)

# lib_emailclient unit tests

emailclient_test_inc = include_directories('../../external', '../../external/fff')

test_emailclient = executable('test_emailclient',
  'test_emailclient.c',
  include_directories: emailclient_test_inc,
  dependencies: [unity_dep, lib_emailclient_dep],
)
test('emailclient', test_emailclient)

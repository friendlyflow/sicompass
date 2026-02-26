# lib_chatclient unit tests

chatclient_test_inc = include_directories('../../external', '../../external/fff')

test_chatclient = executable('test_chatclient',
  'test_chatclient.c',
  include_directories: chatclient_test_inc,
  dependencies: [unity_dep, lib_chatclient_dep],
)
test('chatclient', test_chatclient)

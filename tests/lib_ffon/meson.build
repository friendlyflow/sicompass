# lib_ffon unit tests

ffon_test_inc = include_directories('../../external', '../../external/fff')

test_ffon_element = executable('test_ffon_element',
  'test_ffon_element.c',
  include_directories: ffon_test_inc,
  dependencies: [unity_dep, lib_ffon_dep],
)
test('ffon_element', test_ffon_element)

test_ffon_object = executable('test_ffon_object',
  'test_ffon_object.c',
  include_directories: ffon_test_inc,
  dependencies: [unity_dep, lib_ffon_dep],
)
test('ffon_object', test_ffon_object)

test_ffon_idarray = executable('test_ffon_idarray',
  'test_ffon_idarray.c',
  include_directories: ffon_test_inc,
  dependencies: [unity_dep, lib_ffon_dep],
)
test('ffon_idarray', test_ffon_idarray)

test_ffon_navigation = executable('test_ffon_navigation',
  'test_ffon_navigation.c',
  include_directories: ffon_test_inc,
  dependencies: [unity_dep, lib_ffon_dep],
)
test('ffon_navigation', test_ffon_navigation)

test_ffon_serialization = executable('test_ffon_serialization',
  'test_ffon_serialization.c',
  include_directories: ffon_test_inc,
  dependencies: [unity_dep, lib_ffon_dep],
)
test('ffon_serialization', test_ffon_serialization)

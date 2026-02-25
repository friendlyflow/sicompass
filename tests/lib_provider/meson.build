# lib_provider unit tests

provider_test_inc = include_directories('../../external', '../../external/fff')

test_provider_tags = executable('test_provider_tags',
  'test_provider_tags.c',
  include_directories: provider_test_inc,
  dependencies: [unity_dep, lib_provider_dep],
)
test('provider_tags', test_provider_tags)

test_provider_interface = executable('test_provider_interface',
  'test_provider_interface.c',
  include_directories: provider_test_inc,
  dependencies: [unity_dep, lib_provider_dep],
)
test('provider_interface', test_provider_interface)

test_provider_platform = executable('test_provider_platform',
  'test_provider_platform.c',
  include_directories: provider_test_inc,
  dependencies: [unity_dep, lib_provider_dep],
)
test('provider_platform', test_provider_platform)

test_provider_config = executable('test_provider_config',
  'test_provider_config.c',
  include_directories: provider_test_inc,
  dependencies: [unity_dep, lib_provider_dep],
)
test('provider_config', test_provider_config)

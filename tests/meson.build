# Test configuration for unity + fff
# Note: Tests are disabled on Windows because the unity test framework
# uses macros that are incompatible with MSVC

if host_machine.system() != 'windows'

  # Unity test framework dependency
  unity_dep = dependency('unity', required: true)

  # Include directories for test headers (fff for mocking)
  test_inc = include_directories('../external', '../external/fff')

  # Example test demonstrating unity + fff
  test_example = executable('test_example',
    'test_example.c',
    include_directories: test_inc,
    dependencies: [unity_dep],
  )

  test('example', test_example)

  # Clipboard tests (handleCtrlX, handleCtrlC, handleCtrlV)
  test_clipboard = executable('test_clipboard',
    'test_clipboard.c',
    include_directories: test_inc,
    dependencies: [unity_dep],
  )

  test('clipboard', test_clipboard)

  # AccessKit accessibility tests
  test_accesskit = executable('test_accesskit',
    'test_accesskit.c',
    include_directories: test_inc,
    dependencies: [unity_dep],
  )

  test('accesskit', test_accesskit)

endif

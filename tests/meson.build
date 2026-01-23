# Test configuration for tau + fff
# Note: Tests are disabled on Windows because the tau test framework
# uses macros that are incompatible with MSVC

if host_machine.system() != 'windows'

  # Include directories for test headers
  test_inc = include_directories('../external', '../external/tau', '../external/fff')

  # Example test demonstrating tau + fff
  test_example = executable('test_example',
    'test_example.c',
    include_directories: test_inc,
  )

  test('example', test_example)

  # Clipboard tests (handleCtrlX, handleCtrlC, handleCtrlV)
  test_clipboard = executable('test_clipboard',
    'test_clipboard.c',
    include_directories: test_inc,
  )

  test('clipboard', test_clipboard)

endif

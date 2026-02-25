# Test configuration for unity + fff

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

# Library tests
subdir('lib_ffon')
subdir('lib_provider')
subdir('lib_filebrowser')
subdir('lib_settings')

# Sicompass tests
subdir('sicompass')

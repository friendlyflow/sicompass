# Test configuration for tau + fff

# Include directories for test headers
test_inc = include_directories('../external', '../external/tau', '../external/fff')

# Example test demonstrating tau + fff
test_example = executable('test_example',
  'test_example.c',
  include_directories: test_inc,
)

test('example', test_example)

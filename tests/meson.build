# Test configuration for unity + fff

# Unity test framework dependency
unity_dep = dependency('unity', required: true)

# Include directories for test headers (fff for mocking)
test_inc = include_directories('../external', '../external/fff')

# Library tests
subdir('lib_ffon')
subdir('lib_provider')
subdir('lib_filebrowser')
subdir('lib_chatclient')
subdir('lib_emailclient')
subdir('lib_settings')

# Sicompass tests
subdir('sicompass')

# Sicompass unit tests

# Include directories for test headers (fff for mocking)
sicompass_test_inc = include_directories('../../external', '../../external/fff')

# Clipboard tests (handleCtrlX, handleCtrlC, handleCtrlV)
test_clipboard = executable('test_clipboard',
  'test_clipboard.c',
  include_directories: sicompass_test_inc,
  dependencies: [unity_dep],
)

test('clipboard', test_clipboard)

# AccessKit accessibility tests
test_accesskit = executable('test_accesskit',
  'test_accesskit.c',
  include_directories: sicompass_test_inc,
  dependencies: [unity_dep],
)

test('accesskit', test_accesskit)

# State utility tests (coordinateToString, taskToString, isLineKey, setErrorMessage)
test_state = executable('test_state',
  'test_state.c',
  include_directories: sicompass_test_inc,
  dependencies: [unity_dep],
)

test('state', test_state)

# Caret tests (caretCreate, caretDestroy, caretUpdate, caretReset)
test_caret = executable('test_caret',
  'test_caret.c',
  include_directories: sicompass_test_inc,
  dependencies: [unity_dep],
)

test('caret', test_caret)

# Handler tests (selection helpers, UTF-8 cursor movement)
test_handlers = executable('test_handlers',
  'test_handlers.c',
  include_directories: sicompass_test_inc,
  dependencies: [unity_dep],
)

test('handlers', test_handlers)

# Unicode search tests (utf8_stristr, utf8_stristr_pos)
test_unicode_search = executable('test_unicode_search',
  'test_unicode_search.c',
  include_directories: sicompass_test_inc,
  dependencies: [unity_dep, utf8proc_dep],
)

test('unicode_search', test_unicode_search)

# Provider tests (registry, dispatch, navigation)
test_provider = executable('test_provider',
  'test_provider.c',
  include_directories: sicompass_test_inc,
  dependencies: [unity_dep],
)

test('provider', test_provider)

# List tests (clearListCurrentLayer, populateListCurrentLayer)
test_list = executable('test_list',
  'test_list.c',
  include_directories: sicompass_test_inc,
  dependencies: [unity_dep],
)

test('list', test_list)

# Update tests (updateHistory, updateFfon, updateIds)
test_update = executable('test_update',
  'test_update.c',
  include_directories: sicompass_test_inc,
  dependencies: [unity_dep],
)

test('update', test_update)

# Events tests (handleKeys dispatch, handleInput text insertion)
test_events = executable('test_events',
  'test_events.c',
  include_directories: sicompass_test_inc,
  dependencies: [unity_dep],
)

test('events', test_events)

# Text math tests (getTextScale, getWidthEM, getLineHeight)
test_text_math = executable('test_text_math',
  'test_text_math.c',
  include_directories: sicompass_test_inc,
  dependencies: [unity_dep],
)

test('text_math', test_text_math)

# Checkmark geometry tests (prepareCheckmark)
m_dep = meson.get_compiler('c').find_library('m', required: false)
test_checkmark = executable('test_checkmark',
  'test_checkmark.c',
  include_directories: sicompass_test_inc,
  dependencies: [unity_dep, m_dep],
)

test('checkmark', test_checkmark)

# Advanced handler tests (handleCtrlHome/End, handleColon, handleTab, handleEscape, handleCommand, handleCtrlF, handleUp/Down)
test_handlers_advanced = executable('test_handlers_advanced',
  'test_handlers_advanced.c',
  include_directories: sicompass_test_inc,
  dependencies: [unity_dep],
)

test('handlers_advanced', test_handlers_advanced)

project('sicompass', 'c',
  version : '0.1',
  default_options : ['warning_level=3', 'default_library=static'])

# Address sanitizer only on non-Windows debug builds (MSVC doesn't support it the same way)
if host_machine.system() != 'windows' and get_option('buildtype') == 'debug'
  add_project_arguments('-fsanitize=address', language: 'c')
  add_project_link_arguments('-fsanitize=address', language: 'c')
endif

vulkan_dep = dependency('vulkan')
sdl3_dep = dependency('sdl3')
stb_dep = dependency('stb')
webp_dep = dependency('libwebp')
hb_dep = dependency('harfbuzz')
jsonc_dep = dependency('json-c')
accesskit_dep = dependency('accesskit')
utf8proc_dep = dependency('libutf8proc')
cglm_dep = dependency('cglm')
if host_machine.system() == 'linux'
  xkbcommon_dep = dependency('xkbcommon')
  wlroots_dep = dependency('wlroots-0.19')
  wayland_server_dep = dependency('wayland-server')
  wayland_protocols_dep = dependency('wayland-protocols')
  pixman_dep = dependency('pixman-1')

  # Wayland protocol generation for desicompass
  wayland_scanner = find_program('wayland-scanner')
  wayland_protocols_dir = wayland_protocols_dep.get_variable('pkgdatadir')

  xdg_shell_xml = wayland_protocols_dir / 'stable/xdg-shell/xdg-shell.xml'
  xdg_shell_c = custom_target('xdg-shell-protocol.c',
    input: xdg_shell_xml,
    output: 'xdg-shell-protocol.c',
    command: [wayland_scanner, 'private-code', '@INPUT@', '@OUTPUT@'])
  xdg_shell_h = custom_target('xdg-shell-protocol.h',
    input: xdg_shell_xml,
    output: 'xdg-shell-protocol.h',
    command: [wayland_scanner, 'server-header', '@INPUT@', '@OUTPUT@'])
endif
cc = meson.get_compiler('c')
m_dep = cc.find_library('m', required : false)
dl_dep = cc.find_library('dl', required : false)
ft_dep = dependency('freetype2')

r = run_command('glslangValidator', '--quiet', '-V', 'shaders/image.vert', '-o', 'shaders/image_vert.spv', check: true)
output = r.stdout().strip()
errortxt = r.stderr().strip()

r = run_command('glslangValidator', '--quiet', '-V', 'shaders/image.frag', '-o', 'shaders/image_frag.spv', check: true)
output = r.stdout().strip()
errortxt = r.stderr().strip()

r = run_command('glslangValidator', '--quiet', '-V', 'shaders/text.vert', '-o', 'shaders/text_vert.spv', check: true)
output = r.stdout().strip()
errortxt = r.stderr().strip()

r = run_command('glslangValidator', '--quiet', '-V', 'shaders/text.frag', '-o', 'shaders/text_frag.spv', check: true)
output = r.stdout().strip()
errortxt = r.stderr().strip()

r = run_command('glslangValidator', '--quiet', '-V', 'shaders/rectangle.vert', '-o', 'shaders/rectangle_vert.spv', check: true)
output = r.stdout().strip()
errortxt = r.stderr().strip()

r = run_command('glslangValidator', '--quiet', '-V', 'shaders/rectangle.frag', '-o', 'shaders/rectangle_frag.spv', check: true)
output = r.stdout().strip()
errortxt = r.stderr().strip()

if host_machine.system() == 'linux'
  add_project_arguments('-DVK_USE_PLATFORM_WAYLAND_KHR', language: 'c')
elif host_machine.system() == 'darwin'
  add_project_arguments('-DVK_USE_PLATFORM_MACOS_MVK', language: 'c')
elif host_machine.system() == 'windows'
  add_project_arguments('-DVK_USE_PLATFORM_WIN32_KHR', '-DWIN32_LEAN_AND_MEAN', language: 'c')
  # Prepend debug DLL directory so debug builds use the debug CRT DLLs (prevents heap corruption)
  _win_test_env = environment()
  _win_test_env.prepend('PATH', 'C:/vcpkg/installed/x64-windows/debug/bin')
  add_test_setup('default', env: _win_test_env, is_default: true)
  # Copy runtime DLLs to the build directory so sicompass.exe can be run directly
  _copy_dlls = custom_target('copy_dlls',
    output: 'SDL3.dll',
    command: [find_program('python3'), files('scripts/copy_dlls.py'),
              meson.current_build_dir()],
    build_by_default: true,
  )
endif

subdir('lib/lib_ffon')
subdir('lib/lib_provider')
subdir('lib/lib_chatclient')
subdir('lib/lib_emailclient')
subdir('lib/lib_filebrowser')
subdir('lib/lib_friendlystore')
subdir('lib/lib_notebook')
subdir('lib/lib_webbrowser')
subdir('lib/lib_settings')
subdir('lib/lib_remote')

subdir('src')

tutorial_script_path = 'scripts/tutorial.ts'
sales_demo_script_path = 'scripts/sales_demo.ts'
remote_script_path = 'scripts/remote.ts'

if host_machine.system() == 'linux'
  exe_sc = executable('sicompass', [sources_sc], dependencies: [vulkan_dep, sdl3_dep, xkbcommon_dep, m_dep, dl_dep, ft_dep, hb_dep, stb_dep, webp_dep, jsonc_dep, accesskit_dep, utf8proc_dep, cglm_dep, lib_ffon_dep, lib_provider_dep, lib_filebrowser_dep, lib_chatclient_dep, lib_emailclient_dep, lib_webbrowser_dep, lib_settings_dep],
                      c_args: ['-DTUTORIAL_SCRIPT_PATH="' + tutorial_script_path + '"',
                               '-DSALES_DEMO_SCRIPT_PATH="' + sales_demo_script_path + '"',
                               '-DREMOTE_SCRIPT_PATH="' + remote_script_path + '"'],
                      include_directories: [], install : true)
else
  exe_sc = executable('sicompass', [sources_sc], dependencies: [vulkan_dep, sdl3_dep, m_dep, dl_dep, ft_dep, hb_dep, stb_dep, webp_dep, jsonc_dep, accesskit_dep, utf8proc_dep, cglm_dep, lib_ffon_dep, lib_provider_dep, lib_filebrowser_dep, lib_chatclient_dep, lib_emailclient_dep, lib_webbrowser_dep, lib_settings_dep],
                      c_args: ['-DTUTORIAL_SCRIPT_PATH="' + tutorial_script_path + '"',
                               '-DSALES_DEMO_SCRIPT_PATH="' + sales_demo_script_path + '"',
                               '-DREMOTE_SCRIPT_PATH="' + remote_script_path + '"'],
                      include_directories: [include_directories('external')], install : true)
endif

if host_machine.system() == 'linux'
  exe_dc = executable('desicompass', [sources_dc, xdg_shell_c, xdg_shell_h],
                      dependencies: [wlroots_dep, wayland_server_dep, xkbcommon_dep, pixman_dep, m_dep],
                      c_args: ['-DWLR_USE_UNSTABLE'],
                      include_directories: [], install : true)
endif

subdir('tests')

install_data(
  'lib/lib_tutorial/tutorial.ts',
  'lib/lib_sales_demo/sales_demo.ts',
  'lib/lib_remote/remote.ts',
  rename: ['tutorial.ts', 'sales_demo.ts', 'remote.ts'],
  install_dir: get_option('datadir') / 'sicompass' / 'scripts',
)

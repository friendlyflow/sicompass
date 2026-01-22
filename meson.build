project('sicompass', 'c',
  version : '0.1',
  default_options : ['warning_level=3', 'default_library=static'])

# Address sanitizer only on non-Windows (MSVC doesn't support it the same way)
if host_machine.system() != 'windows'
  add_project_arguments('-fsanitize=address', language: 'c')
  add_project_link_arguments('-fsanitize=address', language: 'c')
endif

vulkan_dep = dependency('vulkan')
sdl3_dep = dependency('sdl3')
stb_dep = dependency('stb')
hb_dep = dependency('harfbuzz')
jsonc_dep = dependency('json-c')
accesskit_dep = dependency('accesskit')
cglm_dep = dependency('cglm')
cc = meson.get_compiler('c')
m_dep = cc.find_library('m', required : false)
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
  add_project_arguments('-DVK_USE_PLATFORM_XCB_KHR', language: 'c')
elif host_machine.system() == 'darwin'
  add_project_arguments('-DVK_USE_PLATFORM_MACOS_MVK', language: 'c')
elif host_machine.system() == 'windows'
  add_project_arguments('-DVK_USE_PLATFORM_WIN32_KHR', language: 'c')
endif

subdir('lib/lib_ffon')
subdir('lib/lib_provider')
subdir('lib/lib_chatclient')
subdir('lib/lib_emailclient')
subdir('lib/lib_filebrowser')
subdir('lib/lib_friendlystore')
subdir('lib/lib_notebook')
subdir('lib/lib_webbrowser')

subdir('src')

exe_sc = executable('sicompass', [sources_sc], dependencies: [vulkan_dep, sdl3_dep, m_dep, ft_dep, hb_dep, stb_dep, jsonc_dep, accesskit_dep, cglm_dep, lib_ffon_dep, lib_provider_dep, lib_filebrowser_dep],
                    include_directories: [], install : true)

subdir('tests')



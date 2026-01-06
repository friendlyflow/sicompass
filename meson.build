project('sicompass', 'c',
  version : '0.1',
  default_options : ['warning_level=3'])

vulkan_dep = dependency('vulkan')
glfw_dep = dependency('glfw3')
stb_dep = dependency('stb')
hb_dep = dependency('harfbuzz')
cc = meson.get_compiler('c')
m_dep = cc.find_library('m', required : false)
ft_dep = cc.find_library('freetype', required : false)

r = run_command('glslangValidator', '--quiet', '-V', 'shaders/shader_depth.vert', '-o', 'shaders/vert.spv', check: true)
output = r.stdout().strip()
errortxt = r.stderr().strip()

r = run_command('glslangValidator', '--quiet', '-V', 'shaders/shader_depth.frag', '-o', 'shaders/frag.spv', check: true)
output = r.stdout().strip()
errortxt = r.stderr().strip()

r = run_command('glslangValidator', '--quiet', '-V', 'shaders/text.vert', '-o', 'shaders/text_vert.spv', check: true)
output = r.stdout().strip()
errortxt = r.stderr().strip()

r = run_command('glslangValidator', '--quiet', '-V', 'shaders/text.frag', '-o', 'shaders/text_frag.spv', check: true)
output = r.stdout().strip()
errortxt = r.stderr().strip()

subdir('src')

add_project_arguments('-DVK_USE_PLATFORM_XCB_KHR', language: 'c')

exe_sc = executable('sc', [sources_sc], dependencies: [vulkan_dep, glfw_dep, m_dep, ft_dep, stb_dep],
                    include_directories: [], install : false)

#test('basic', exe)


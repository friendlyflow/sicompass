lib_notebook_src = files('src/notebook.c')
lib_notebook_inc = include_directories('include')

lib_notebook = library('notebook',
  lib_notebook_src,
  include_directories: lib_notebook_inc,
  install: true,
)

lib_notebook_dep = declare_dependency(
  link_with: lib_notebook,
  include_directories: lib_notebook_inc,
)

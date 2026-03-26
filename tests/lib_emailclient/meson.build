# lib_emailclient unit tests

emailclient_test_inc = include_directories('../../external', '../../external/fff')

emailclient_link_args = []
if host_machine.system() == 'linux'
  emailclient_link_args = [
    '-Wl,--wrap=emailclientOAuth2Authorize',
    '-Wl,--wrap=emailclientOAuth2RefreshToken',
    '-Wl,--wrap=providerGetMainConfigPath',
    '-Wl,--wrap=emailclientListFolders',
    '-Wl,--wrap=emailclientFreeFolders',
    '-Wl,--wrap=emailclientListMessages',
    '-Wl,--wrap=emailclientFreeHeaders',
    '-Wl,--wrap=emailclientFetchMessage',
    '-Wl,--wrap=emailclientFreeMessage',
    '-Wl,--wrap=emailclientFetchMessageByMessageId',
    '-Wl,--wrap=emailclientIdleStart',
    '-Wl,--wrap=emailclientIdleStop',
    '-Wl,--wrap=emailclientGlobalInit',
    '-Wl,--wrap=emailclientGlobalCleanup',
    '-Wl,--wrap=emailclientSendMessage',
  ]
endif

test_emailclient = executable('test_emailclient',
  'test_emailclient.c',
  include_directories: emailclient_test_inc,
  dependencies: [unity_dep, lib_emailclient_dep],
  link_args: emailclient_link_args,
)
test('emailclient', test_emailclient)

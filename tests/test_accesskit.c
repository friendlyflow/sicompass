/*
 * Tests for AccessKit accessibility functions: accesskitInit, accesskitDestroy, accesskitSpeak
 */

#include <unity.h>
#include <fff/fff.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

DEFINE_FFF_GLOBALS

// AccessKit types (simplified for testing)
typedef uint64_t accesskit_node_id;
typedef int accesskit_role;
typedef int accesskit_live;

struct accesskit_tree_update;
struct accesskit_node;
struct accesskit_tree;
struct accesskit_action_request;

// Platform-specific adapter types (for the SDL adapter wrapper)
struct accesskit_unix_adapter;
struct accesskit_macos_subclassing_adapter;
struct accesskit_windows_subclassing_adapter;

// Mock SDL_Window
typedef struct SDL_Window SDL_Window;

// SDL adapter struct (matches accesskit_sdl.h)
struct accesskit_sdl_adapter {
#if defined(__APPLE__)
    struct accesskit_macos_subclassing_adapter *adapter;
#elif defined(_WIN32)
    struct accesskit_windows_subclassing_adapter *adapter;
#else
    struct accesskit_unix_adapter *adapter;
#endif
};

// AccessKit constants
#define ACCESSKIT_ROLE_WINDOW 0
#define ACCESSKIT_ROLE_LABEL 1
#define ACCESSKIT_LIVE_POLITE 1

// Callback types (matching accesskit_sdl.h)
typedef struct accesskit_tree_update* (*accesskit_activation_handler_callback)(void*);
typedef void (*accesskit_action_handler_callback)(struct accesskit_action_request*, void*);
typedef void (*accesskit_deactivation_handler_callback)(void*);
typedef struct accesskit_tree_update* (*accesskit_tree_update_factory)(void*);

// Mock AccessKit tree/node functions
FAKE_VALUE_FUNC(struct accesskit_tree_update*, accesskit_tree_update_with_capacity_and_focus, size_t, accesskit_node_id)
FAKE_VALUE_FUNC(struct accesskit_tree_update*, accesskit_tree_update_with_focus, accesskit_node_id)
FAKE_VALUE_FUNC(struct accesskit_node*, accesskit_node_new, accesskit_role)
FAKE_VOID_FUNC(accesskit_node_set_label, struct accesskit_node*, const char*)
FAKE_VOID_FUNC(accesskit_node_set_children, struct accesskit_node*, size_t, const accesskit_node_id*)
FAKE_VOID_FUNC(accesskit_node_set_live, struct accesskit_node*, accesskit_live)
FAKE_VOID_FUNC(accesskit_tree_update_push_node, struct accesskit_tree_update*, accesskit_node_id, struct accesskit_node*)
FAKE_VALUE_FUNC(struct accesskit_tree*, accesskit_tree_new, accesskit_node_id)
FAKE_VOID_FUNC(accesskit_tree_set_toolkit_name, struct accesskit_tree*, const char*)
FAKE_VOID_FUNC(accesskit_tree_set_toolkit_version, struct accesskit_tree*, const char*)
FAKE_VOID_FUNC(accesskit_tree_update_set_tree, struct accesskit_tree_update*, struct accesskit_tree*)
FAKE_VOID_FUNC(accesskit_action_request_free, struct accesskit_action_request*)

// SDL adapter mocks
FAKE_VOID_FUNC(accesskit_sdl_adapter_init, struct accesskit_sdl_adapter*, SDL_Window*,
    accesskit_activation_handler_callback, void*,
    accesskit_action_handler_callback, void*,
    accesskit_deactivation_handler_callback, void*)
FAKE_VOID_FUNC(accesskit_sdl_adapter_destroy, struct accesskit_sdl_adapter*)
FAKE_VOID_FUNC(accesskit_sdl_adapter_update_if_active, struct accesskit_sdl_adapter*,
    accesskit_tree_update_factory, void*)
FAKE_VOID_FUNC(accesskit_sdl_adapter_update_window_focus_state, struct accesskit_sdl_adapter*, bool)

// AccessKit node IDs (same as in render.c)
#define ACCESSKIT_ROOT_ID 1
#define ACCESSKIT_LIVE_REGION_ID 2

// Simplified structures for testing
#define MAX_ID_DEPTH 32
#define MAX_LINE_LENGTH 4096

typedef struct {
    int ids[MAX_ID_DEPTH];
    int depth;
} IdArray;

// Coordinate enum (matching view.h)
typedef enum {
    COORDINATE_OPERATOR_GENERAL,
    COORDINATE_OPERATOR_INSERT,
    COORDINATE_EDITOR_GENERAL,
    COORDINATE_EDITOR_INSERT,
    COORDINATE_EDITOR_NORMAL,
    COORDINATE_EDITOR_VISUAL,
    COORDINATE_SIMPLE_SEARCH,
    COORDINATE_EXTENDED_SEARCH,
    COORDINATE_COMMAND
} Coordinate;

// Forward declarations matching view.h
typedef struct AppRenderer AppRenderer;
typedef struct SiCompassApplication SiCompassApplication;

struct AppRenderer {
    struct accesskit_sdl_adapter accesskitAdapter;
    accesskit_node_id accesskitRootId;
    accesskit_node_id accesskitLiveRegionId;
    Coordinate currentCoordinate;
};

struct SiCompassApplication {
    AppRenderer* appRenderer;
    SDL_Window* window;
};

// Static variables to capture handlers for testing
static accesskit_activation_handler_callback captured_activation_handler = NULL;
static accesskit_action_handler_callback captured_action_handler = NULL;
static accesskit_deactivation_handler_callback captured_deactivation_handler = NULL;

// Custom fake for accesskit_sdl_adapter_init that captures handlers
static void fake_accesskit_sdl_adapter_init(
    struct accesskit_sdl_adapter *adapter, SDL_Window *window,
    accesskit_activation_handler_callback activation_handler, void* activation_userdata,
    accesskit_action_handler_callback action_handler, void* action_userdata,
    accesskit_deactivation_handler_callback deactivation_handler, void* deactivation_userdata) {
    (void)window;
    (void)activation_userdata;
    (void)action_userdata;
    (void)deactivation_userdata;
    captured_activation_handler = activation_handler;
    captured_action_handler = action_handler;
    captured_deactivation_handler = deactivation_handler;
    adapter->adapter = (void*)1; // Mark as initialized
}

// Custom fake for accesskit_sdl_adapter_destroy
static void fake_accesskit_sdl_adapter_destroy(struct accesskit_sdl_adapter* adapter) {
    adapter->adapter = NULL;
}

// Static variable to capture speak text
static const char* captured_speak_text = NULL;
static accesskit_tree_update_factory captured_update_factory = NULL;

// Custom fake for accesskit_sdl_adapter_update_if_active
static void fake_accesskit_sdl_adapter_update_if_active(
    struct accesskit_sdl_adapter* adapter,
    accesskit_tree_update_factory update_factory,
    void* userdata) {
    (void)adapter;
    captured_update_factory = update_factory;
    captured_speak_text = (const char*)userdata;
}

// Implementation of accesskitInit (matching render.c with SDL adapter)
void accesskitInit(SiCompassApplication *app) {
    app->appRenderer->accesskitRootId = ACCESSKIT_ROOT_ID;
    app->appRenderer->accesskitLiveRegionId = ACCESSKIT_LIVE_REGION_ID;

    accesskit_sdl_adapter_init(
        &app->appRenderer->accesskitAdapter,
        app->window,
        NULL, // activation handler placeholder
        NULL,
        NULL, // action handler placeholder
        NULL,
        NULL, // deactivation handler placeholder
        NULL
    );
}

// Implementation of accesskitDestroy (matching render.c with SDL adapter)
void accesskitDestroy(AppRenderer *appRenderer) {
    accesskit_sdl_adapter_destroy(&appRenderer->accesskitAdapter);
}

// Implementation of accesskitSpeak (matching render.c with SDL adapter)
void accesskitSpeak(AppRenderer *appRenderer, const char *text) {
    if (!text) {
        return;
    }

    accesskit_sdl_adapter_update_if_active(
        &appRenderer->accesskitAdapter,
        NULL, // update factory placeholder
        (void *)text
    );
}

// Implementation of coordinateToString (copied from state.c)
const char* coordinateToString(Coordinate coord) {
    switch (coord) {
        case COORDINATE_OPERATOR_GENERAL: return "operator mode";
        case COORDINATE_OPERATOR_INSERT: return "operator insert";
        case COORDINATE_EDITOR_GENERAL: return "editor mode";
        case COORDINATE_EDITOR_INSERT: return "editor insert";
        case COORDINATE_EDITOR_NORMAL: return "editor normal";
        case COORDINATE_EDITOR_VISUAL: return "editor visual";
        case COORDINATE_SIMPLE_SEARCH: return "search";
        case COORDINATE_COMMAND: return "run command";
        case COORDINATE_EXTENDED_SEARCH: return "ext search";
        default: return "unknown";
    }
}

// Implementation of accesskitSpeakModeChange (copied from state.c)
void accesskitSpeakModeChange(AppRenderer *appRenderer, const char *context) {
    char announcement[512];
    const char *modeName = coordinateToString(appRenderer->currentCoordinate);

    if (context && context[0] != '\0') {
        snprintf(announcement, sizeof(announcement), "%s - %s", modeName, context);
    } else {
        snprintf(announcement, sizeof(announcement), "%s", modeName);
    }

    accesskitSpeak(appRenderer, announcement);
}

// Helper functions
static SiCompassApplication* createTestApp(void) {
    SiCompassApplication *app = calloc(1, sizeof(SiCompassApplication));
    app->appRenderer = calloc(1, sizeof(AppRenderer));
    app->window = (SDL_Window*)0x12345678; // Mock window pointer
    return app;
}

static void destroyTestApp(SiCompassApplication *app) {
    if (app->appRenderer) {
        free(app->appRenderer);
    }
    free(app);
}

/* ============================================
 * Unity Test Setup/Teardown
 * ============================================ */

void setUp(void) {
    RESET_FAKE(accesskit_tree_update_with_capacity_and_focus);
    RESET_FAKE(accesskit_tree_update_with_focus);
    RESET_FAKE(accesskit_node_new);
    RESET_FAKE(accesskit_node_set_label);
    RESET_FAKE(accesskit_node_set_children);
    RESET_FAKE(accesskit_node_set_live);
    RESET_FAKE(accesskit_tree_update_push_node);
    RESET_FAKE(accesskit_tree_new);
    RESET_FAKE(accesskit_tree_set_toolkit_name);
    RESET_FAKE(accesskit_tree_set_toolkit_version);
    RESET_FAKE(accesskit_tree_update_set_tree);
    RESET_FAKE(accesskit_action_request_free);
    RESET_FAKE(accesskit_sdl_adapter_init);
    RESET_FAKE(accesskit_sdl_adapter_destroy);
    RESET_FAKE(accesskit_sdl_adapter_update_if_active);
    RESET_FAKE(accesskit_sdl_adapter_update_window_focus_state);
    FFF_RESET_HISTORY();

    // Set up custom fakes
    accesskit_sdl_adapter_init_fake.custom_fake = fake_accesskit_sdl_adapter_init;
    accesskit_sdl_adapter_destroy_fake.custom_fake = fake_accesskit_sdl_adapter_destroy;
    accesskit_sdl_adapter_update_if_active_fake.custom_fake = fake_accesskit_sdl_adapter_update_if_active;

    captured_activation_handler = NULL;
    captured_action_handler = NULL;
    captured_deactivation_handler = NULL;
    captured_speak_text = NULL;
    captured_update_factory = NULL;
}

void tearDown(void) {
}

/* ============================================
 * accesskitInit tests
 * ============================================ */

void test_accesskitInit_sets_root_id(void) {
    SiCompassApplication *app = createTestApp();

    accesskitInit(app);

    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_ROOT_ID, app->appRenderer->accesskitRootId);

    destroyTestApp(app);
}

void test_accesskitInit_sets_live_region_id(void) {
    SiCompassApplication *app = createTestApp();

    accesskitInit(app);

    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_LIVE_REGION_ID, app->appRenderer->accesskitLiveRegionId);

    destroyTestApp(app);
}

void test_accesskitInit_initializes_adapter(void) {
    SiCompassApplication *app = createTestApp();

    accesskitInit(app);

    TEST_ASSERT_EQUAL_INT(1, accesskit_sdl_adapter_init_fake.call_count);
    TEST_ASSERT_NOT_NULL(app->appRenderer->accesskitAdapter.adapter);

    destroyTestApp(app);
}

void test_accesskitInit_passes_window_to_adapter(void) {
    SiCompassApplication *app = createTestApp();

    accesskitInit(app);

    TEST_ASSERT_EQUAL_PTR(app->window, accesskit_sdl_adapter_init_fake.arg1_val);

    destroyTestApp(app);
}

/* ============================================
 * accesskitDestroy tests
 * ============================================ */

void test_accesskitDestroy_destroys_adapter(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    accesskitDestroy(app->appRenderer);

    TEST_ASSERT_EQUAL_INT(1, accesskit_sdl_adapter_destroy_fake.call_count);

    destroyTestApp(app);
}

void test_accesskitDestroy_clears_adapter_internal(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    TEST_ASSERT_NOT_NULL(app->appRenderer->accesskitAdapter.adapter);

    accesskitDestroy(app->appRenderer);

    TEST_ASSERT_NULL(app->appRenderer->accesskitAdapter.adapter);

    destroyTestApp(app);
}

/* ============================================
 * accesskitSpeak tests
 * ============================================ */

void test_accesskitSpeak_calls_update_if_active(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    accesskitSpeak(app->appRenderer, "Hello World");

    TEST_ASSERT_EQUAL_INT(1, accesskit_sdl_adapter_update_if_active_fake.call_count);

    destroyTestApp(app);
}

void test_accesskitSpeak_passes_text_to_update(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    const char *text = "Test announcement";
    accesskitSpeak(app->appRenderer, text);

    TEST_ASSERT_EQUAL_STRING(text, captured_speak_text);

    destroyTestApp(app);
}

void test_accesskitSpeak_does_nothing_with_null_text(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    accesskitSpeak(app->appRenderer, NULL);

    TEST_ASSERT_EQUAL_INT(0, accesskit_sdl_adapter_update_if_active_fake.call_count);

    destroyTestApp(app);
}

void test_accesskitSpeak_with_empty_string(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    // Empty string is not NULL, so it should still call the update
    accesskitSpeak(app->appRenderer, "");

    TEST_ASSERT_EQUAL_INT(1, accesskit_sdl_adapter_update_if_active_fake.call_count);
    TEST_ASSERT_EQUAL_STRING("", captured_speak_text);

    destroyTestApp(app);
}

void test_accesskitSpeak_multiple_announcements(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    accesskitSpeak(app->appRenderer, "First");
    TEST_ASSERT_EQUAL_STRING("First", captured_speak_text);

    accesskitSpeak(app->appRenderer, "Second");
    TEST_ASSERT_EQUAL_STRING("Second", captured_speak_text);

    accesskitSpeak(app->appRenderer, "Third");
    TEST_ASSERT_EQUAL_STRING("Third", captured_speak_text);

    TEST_ASSERT_EQUAL_INT(3, accesskit_sdl_adapter_update_if_active_fake.call_count);

    destroyTestApp(app);
}

/* ============================================
 * Integration tests
 * ============================================ */

void test_accesskit_lifecycle_init_speak_destroy(void) {
    SiCompassApplication *app = createTestApp();

    // Initialize
    accesskitInit(app);
    TEST_ASSERT_NOT_NULL(app->appRenderer->accesskitAdapter.adapter);
    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_ROOT_ID, app->appRenderer->accesskitRootId);
    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_LIVE_REGION_ID, app->appRenderer->accesskitLiveRegionId);

    // Speak
    accesskitSpeak(app->appRenderer, "Application started");
    TEST_ASSERT_EQUAL_STRING("Application started", captured_speak_text);

    // Destroy
    accesskitDestroy(app->appRenderer);
    TEST_ASSERT_NULL(app->appRenderer->accesskitAdapter.adapter);
    TEST_ASSERT_EQUAL_INT(1, accesskit_sdl_adapter_destroy_fake.call_count);

    destroyTestApp(app);
}

/* ============================================
 * accesskitSpeakModeChange tests
 * ============================================ */

void test_accesskitSpeakModeChange_announces_mode_name(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    app->appRenderer->currentCoordinate = COORDINATE_SIMPLE_SEARCH;
    accesskitSpeakModeChange(app->appRenderer, NULL);

    TEST_ASSERT_EQUAL_STRING("search", captured_speak_text);

    destroyTestApp(app);
}

void test_accesskitSpeakModeChange_announces_mode_with_context(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    app->appRenderer->currentCoordinate = COORDINATE_EDITOR_INSERT;
    accesskitSpeakModeChange(app->appRenderer, "filename.txt");

    TEST_ASSERT_EQUAL_STRING("editor insert - filename.txt", captured_speak_text);

    destroyTestApp(app);
}

void test_accesskitSpeakModeChange_handles_empty_context(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    app->appRenderer->currentCoordinate = COORDINATE_COMMAND;
    accesskitSpeakModeChange(app->appRenderer, "");

    // Empty context should result in mode name only
    TEST_ASSERT_EQUAL_STRING("run command", captured_speak_text);

    destroyTestApp(app);
}

void test_accesskitSpeakModeChange_operator_mode(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    app->appRenderer->currentCoordinate = COORDINATE_OPERATOR_GENERAL;
    accesskitSpeakModeChange(app->appRenderer, NULL);

    TEST_ASSERT_EQUAL_STRING("operator mode", captured_speak_text);

    destroyTestApp(app);
}

void test_accesskitSpeakModeChange_operator_insert_with_context(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    app->appRenderer->currentCoordinate = COORDINATE_OPERATOR_INSERT;
    accesskitSpeakModeChange(app->appRenderer, "Documents");

    TEST_ASSERT_EQUAL_STRING("operator insert - Documents", captured_speak_text);

    destroyTestApp(app);
}

void test_accesskitSpeakModeChange_editor_mode(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    app->appRenderer->currentCoordinate = COORDINATE_EDITOR_GENERAL;
    accesskitSpeakModeChange(app->appRenderer, NULL);

    TEST_ASSERT_EQUAL_STRING("editor mode", captured_speak_text);

    destroyTestApp(app);
}

void test_accesskitSpeakModeChange_extended_search(void) {
    SiCompassApplication *app = createTestApp();
    accesskitInit(app);

    app->appRenderer->currentCoordinate = COORDINATE_EXTENDED_SEARCH;
    accesskitSpeakModeChange(app->appRenderer, NULL);

    TEST_ASSERT_EQUAL_STRING("ext search", captured_speak_text);

    destroyTestApp(app);
}

/* ============================================
 * Main - Run all tests
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // accesskitInit tests
    RUN_TEST(test_accesskitInit_sets_root_id);
    RUN_TEST(test_accesskitInit_sets_live_region_id);
    RUN_TEST(test_accesskitInit_initializes_adapter);
    RUN_TEST(test_accesskitInit_passes_window_to_adapter);

    // accesskitDestroy tests
    RUN_TEST(test_accesskitDestroy_destroys_adapter);
    RUN_TEST(test_accesskitDestroy_clears_adapter_internal);

    // accesskitSpeak tests
    RUN_TEST(test_accesskitSpeak_calls_update_if_active);
    RUN_TEST(test_accesskitSpeak_passes_text_to_update);
    RUN_TEST(test_accesskitSpeak_does_nothing_with_null_text);
    RUN_TEST(test_accesskitSpeak_with_empty_string);
    RUN_TEST(test_accesskitSpeak_multiple_announcements);

    // Integration tests
    RUN_TEST(test_accesskit_lifecycle_init_speak_destroy);

    // accesskitSpeakModeChange tests
    RUN_TEST(test_accesskitSpeakModeChange_announces_mode_name);
    RUN_TEST(test_accesskitSpeakModeChange_announces_mode_with_context);
    RUN_TEST(test_accesskitSpeakModeChange_handles_empty_context);
    RUN_TEST(test_accesskitSpeakModeChange_operator_mode);
    RUN_TEST(test_accesskitSpeakModeChange_operator_insert_with_context);
    RUN_TEST(test_accesskitSpeakModeChange_editor_mode);
    RUN_TEST(test_accesskitSpeakModeChange_extended_search);

    return UNITY_END();
}

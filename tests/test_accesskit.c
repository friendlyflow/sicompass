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
struct accesskit_unix_adapter;
struct accesskit_macos_adapter;
struct accesskit_windows_adapter;

// AccessKit constants
#define ACCESSKIT_ROLE_WINDOW 0
#define ACCESSKIT_ROLE_LABEL 1
#define ACCESSKIT_LIVE_POLITE 1

// Mock AccessKit functions
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

// Unix adapter mocks
typedef struct accesskit_tree_update* (*accesskit_activation_handler)(void*);
typedef void (*accesskit_action_handler)(struct accesskit_action_request*, void*);
typedef void (*accesskit_deactivation_handler)(void*);
typedef struct accesskit_tree_update* (*accesskit_update_factory)(void*);

FAKE_VALUE_FUNC(struct accesskit_unix_adapter*, accesskit_unix_adapter_new,
    accesskit_activation_handler, void*,
    accesskit_action_handler, void*,
    accesskit_deactivation_handler, void*)
FAKE_VOID_FUNC(accesskit_unix_adapter_free, struct accesskit_unix_adapter*)
FAKE_VOID_FUNC(accesskit_unix_adapter_update_if_active, struct accesskit_unix_adapter*,
    accesskit_update_factory, void*)

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

// Forward declarations matching view.h
typedef struct AppRenderer AppRenderer;
typedef struct SiCompassApplication SiCompassApplication;

struct AppRenderer {
    struct accesskit_unix_adapter *accesskitAdapter;
    accesskit_node_id accesskitRootId;
    accesskit_node_id accesskitLiveRegionId;
};

struct SiCompassApplication {
    AppRenderer* appRenderer;
};

// Static variable to capture the activation handler for testing
static accesskit_activation_handler captured_activation_handler = NULL;
static accesskit_action_handler captured_action_handler = NULL;
static accesskit_deactivation_handler captured_deactivation_handler = NULL;

// Custom fake for accesskit_unix_adapter_new that captures handlers
static struct accesskit_unix_adapter* fake_accesskit_unix_adapter_new(
    accesskit_activation_handler activation_handler, void* activation_userdata,
    accesskit_action_handler action_handler, void* action_userdata,
    accesskit_deactivation_handler deactivation_handler, void* deactivation_userdata) {
    (void)activation_userdata;
    (void)action_userdata;
    (void)deactivation_userdata;
    captured_activation_handler = activation_handler;
    captured_action_handler = action_handler;
    captured_deactivation_handler = deactivation_handler;
    return (struct accesskit_unix_adapter*)malloc(sizeof(void*));
}

// Custom fake for accesskit_unix_adapter_free that actually frees memory
static void fake_accesskit_unix_adapter_free(struct accesskit_unix_adapter* adapter) {
    free(adapter);
}

// Static variable to capture speak text
static const char* captured_speak_text = NULL;
static struct accesskit_tree_update* (*captured_update_factory)(void*) = NULL;

// Custom fake for accesskit_unix_adapter_update_if_active
static void fake_accesskit_unix_adapter_update_if_active(
    struct accesskit_unix_adapter* adapter,
    struct accesskit_tree_update* (*update_factory)(void*),
    void* userdata) {
    (void)adapter;
    captured_update_factory = update_factory;
    captured_speak_text = (const char*)userdata;
}

// Custom fake that also invokes the update factory (for tree population tests)
static void fake_accesskit_unix_adapter_update_if_active_invoke(
    struct accesskit_unix_adapter* adapter,
    struct accesskit_tree_update* (*update_factory)(void*),
    void* userdata) {
    (void)adapter;
    captured_update_factory = update_factory;
    captured_speak_text = (const char*)userdata;
    // Actually invoke the factory to test tree population
    if (update_factory) {
        update_factory(userdata);
    }
}

// Callback for AccessKit activation - returns initial tree (copied from render.c)
static struct accesskit_tree_update* accesskitActivationHandler(void *userdata) {
    (void)userdata;
    // Create initial tree with root window and live region
    struct accesskit_tree_update *update = accesskit_tree_update_with_capacity_and_focus(2, ACCESSKIT_ROOT_ID);

    // Create root node (window)
    struct accesskit_node *root = accesskit_node_new(ACCESSKIT_ROLE_WINDOW);
    accesskit_node_set_label(root, "Silicon's Compass");
    accesskit_node_id children[] = {ACCESSKIT_LIVE_REGION_ID};
    accesskit_node_set_children(root, 1, children);
    accesskit_tree_update_push_node(update, ACCESSKIT_ROOT_ID, root);

    // Create live region for announcements
    struct accesskit_node *liveRegion = accesskit_node_new(ACCESSKIT_ROLE_LABEL);
    accesskit_node_set_live(liveRegion, ACCESSKIT_LIVE_POLITE);
    accesskit_node_set_label(liveRegion, "");
    accesskit_tree_update_push_node(update, ACCESSKIT_LIVE_REGION_ID, liveRegion);

    // Set tree info
    struct accesskit_tree *tree = accesskit_tree_new(ACCESSKIT_ROOT_ID);
    accesskit_tree_set_toolkit_name(tree, "sicompass");
    accesskit_tree_set_toolkit_version(tree, "0.1");
    accesskit_tree_update_set_tree(update, tree);

    return update;
}

// Callback for AccessKit action requests (copied from render.c)
static void accesskitActionHandler(struct accesskit_action_request *request, void *userdata) {
    (void)userdata;
    // Handle accessibility actions (click, focus, etc.)
    // For now, we just free the request
    accesskit_action_request_free(request);
}

// Callback for AccessKit deactivation (copied from render.c)
static void accesskitDeactivationHandler(void *userdata) {
    (void)userdata;
    // Called when assistive technology disconnects
    // Nothing to do here for now
}

// Factory function for tree updates when speaking (copied from render.c)
static struct accesskit_tree_update* accesskitSpeakUpdateFactory(void *userdata) {
    const char *text = (const char *)userdata;

    struct accesskit_tree_update *update = accesskit_tree_update_with_focus(ACCESSKIT_ROOT_ID);

    // Update live region with new text
    struct accesskit_node *liveRegion = accesskit_node_new(ACCESSKIT_ROLE_LABEL);
    accesskit_node_set_live(liveRegion, ACCESSKIT_LIVE_POLITE);
    accesskit_node_set_label(liveRegion, text);
    accesskit_tree_update_push_node(update, ACCESSKIT_LIVE_REGION_ID, liveRegion);

    return update;
}

// Implementation of accesskitInit (copied from render.c, simplified for Linux)
void accesskitInit(SiCompassApplication *app) {
    app->appRenderer->accesskitRootId = ACCESSKIT_ROOT_ID;
    app->appRenderer->accesskitLiveRegionId = ACCESSKIT_LIVE_REGION_ID;

    app->appRenderer->accesskitAdapter = accesskit_unix_adapter_new(
        accesskitActivationHandler,
        NULL,
        accesskitActionHandler,
        NULL,
        accesskitDeactivationHandler,
        NULL
    );
}

// Implementation of accesskitDestroy (copied from render.c, simplified for Linux)
void accesskitDestroy(AppRenderer *appRenderer) {
    if (appRenderer->accesskitAdapter) {
        accesskit_unix_adapter_free(appRenderer->accesskitAdapter);
        appRenderer->accesskitAdapter = NULL;
    }
}

// Implementation of accesskitSpeak (copied from render.c, simplified for Linux)
void accesskitSpeak(AppRenderer *appRenderer, const char *text) {
    if (!appRenderer->accesskitAdapter || !text) {
        return;
    }

    accesskit_unix_adapter_update_if_active(
        appRenderer->accesskitAdapter,
        accesskitSpeakUpdateFactory,
        (void *)text
    );
}

// Helper functions
static SiCompassApplication* createTestApp(void) {
    SiCompassApplication *app = calloc(1, sizeof(SiCompassApplication));
    app->appRenderer = calloc(1, sizeof(AppRenderer));
    return app;
}

static void destroyTestApp(SiCompassApplication *app) {
    if (app->appRenderer) {
        if (app->appRenderer->accesskitAdapter) {
            free(app->appRenderer->accesskitAdapter);
        }
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
    RESET_FAKE(accesskit_unix_adapter_new);
    RESET_FAKE(accesskit_unix_adapter_free);
    RESET_FAKE(accesskit_unix_adapter_update_if_active);
    FFF_RESET_HISTORY();

    // Set up custom fakes that properly handle memory
    accesskit_unix_adapter_free_fake.custom_fake = fake_accesskit_unix_adapter_free;

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
    accesskit_unix_adapter_new_fake.custom_fake = fake_accesskit_unix_adapter_new;

    accesskitInit(app);

    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_ROOT_ID, app->appRenderer->accesskitRootId);

    destroyTestApp(app);
}

void test_accesskitInit_sets_live_region_id(void) {
    SiCompassApplication *app = createTestApp();
    accesskit_unix_adapter_new_fake.custom_fake = fake_accesskit_unix_adapter_new;

    accesskitInit(app);

    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_LIVE_REGION_ID, app->appRenderer->accesskitLiveRegionId);

    destroyTestApp(app);
}

void test_accesskitInit_creates_adapter(void) {
    SiCompassApplication *app = createTestApp();
    accesskit_unix_adapter_new_fake.custom_fake = fake_accesskit_unix_adapter_new;

    accesskitInit(app);

    TEST_ASSERT_NOT_NULL(app->appRenderer->accesskitAdapter);
    TEST_ASSERT_EQUAL_INT(1, accesskit_unix_adapter_new_fake.call_count);

    destroyTestApp(app);
}

void test_accesskitInit_adapter_is_null_when_creation_fails(void) {
    SiCompassApplication *app = createTestApp();
    accesskit_unix_adapter_new_fake.return_val = NULL;

    accesskitInit(app);

    TEST_ASSERT_NULL(app->appRenderer->accesskitAdapter);

    destroyTestApp(app);
}

/* ============================================
 * accesskitDestroy tests
 * ============================================ */

void test_accesskitDestroy_frees_adapter(void) {
    SiCompassApplication *app = createTestApp();
    accesskit_unix_adapter_new_fake.custom_fake = fake_accesskit_unix_adapter_new;
    accesskitInit(app);

    accesskitDestroy(app->appRenderer);

    TEST_ASSERT_EQUAL_INT(1, accesskit_unix_adapter_free_fake.call_count);
    TEST_ASSERT_NULL(app->appRenderer->accesskitAdapter);

    // Clean up without double-free
    free(app->appRenderer);
    free(app);
}

void test_accesskitDestroy_handles_null_adapter(void) {
    SiCompassApplication *app = createTestApp();
    app->appRenderer->accesskitAdapter = NULL;

    // Should not crash
    accesskitDestroy(app->appRenderer);

    TEST_ASSERT_EQUAL_INT(0, accesskit_unix_adapter_free_fake.call_count);

    destroyTestApp(app);
}

void test_accesskitDestroy_sets_adapter_to_null(void) {
    SiCompassApplication *app = createTestApp();
    accesskit_unix_adapter_new_fake.custom_fake = fake_accesskit_unix_adapter_new;
    accesskitInit(app);

    TEST_ASSERT_NOT_NULL(app->appRenderer->accesskitAdapter);

    accesskitDestroy(app->appRenderer);

    TEST_ASSERT_NULL(app->appRenderer->accesskitAdapter);

    free(app->appRenderer);
    free(app);
}

/* ============================================
 * accesskitSpeak tests
 * ============================================ */

void test_accesskitSpeak_calls_update_if_active(void) {
    SiCompassApplication *app = createTestApp();
    accesskit_unix_adapter_new_fake.custom_fake = fake_accesskit_unix_adapter_new;
    accesskit_unix_adapter_update_if_active_fake.custom_fake = fake_accesskit_unix_adapter_update_if_active;
    accesskitInit(app);

    accesskitSpeak(app->appRenderer, "Hello World");

    TEST_ASSERT_EQUAL_INT(1, accesskit_unix_adapter_update_if_active_fake.call_count);

    destroyTestApp(app);
}

void test_accesskitSpeak_passes_text_to_update(void) {
    SiCompassApplication *app = createTestApp();
    accesskit_unix_adapter_new_fake.custom_fake = fake_accesskit_unix_adapter_new;
    accesskit_unix_adapter_update_if_active_fake.custom_fake = fake_accesskit_unix_adapter_update_if_active;
    accesskitInit(app);

    const char *text = "Test announcement";
    accesskitSpeak(app->appRenderer, text);

    TEST_ASSERT_EQUAL_STRING(text, captured_speak_text);

    destroyTestApp(app);
}

void test_accesskitSpeak_does_nothing_with_null_adapter(void) {
    SiCompassApplication *app = createTestApp();
    app->appRenderer->accesskitAdapter = NULL;

    accesskitSpeak(app->appRenderer, "Hello");

    TEST_ASSERT_EQUAL_INT(0, accesskit_unix_adapter_update_if_active_fake.call_count);

    destroyTestApp(app);
}

void test_accesskitSpeak_does_nothing_with_null_text(void) {
    SiCompassApplication *app = createTestApp();
    accesskit_unix_adapter_new_fake.custom_fake = fake_accesskit_unix_adapter_new;
    accesskitInit(app);

    accesskitSpeak(app->appRenderer, NULL);

    TEST_ASSERT_EQUAL_INT(0, accesskit_unix_adapter_update_if_active_fake.call_count);

    destroyTestApp(app);
}

void test_accesskitSpeak_with_empty_string(void) {
    SiCompassApplication *app = createTestApp();
    accesskit_unix_adapter_new_fake.custom_fake = fake_accesskit_unix_adapter_new;
    accesskit_unix_adapter_update_if_active_fake.custom_fake = fake_accesskit_unix_adapter_update_if_active;
    accesskitInit(app);

    // Empty string is not NULL, so it should still call the update
    accesskitSpeak(app->appRenderer, "");

    TEST_ASSERT_EQUAL_INT(1, accesskit_unix_adapter_update_if_active_fake.call_count);
    TEST_ASSERT_EQUAL_STRING("", captured_speak_text);

    destroyTestApp(app);
}

void test_accesskitSpeak_multiple_announcements(void) {
    SiCompassApplication *app = createTestApp();
    accesskit_unix_adapter_new_fake.custom_fake = fake_accesskit_unix_adapter_new;
    accesskit_unix_adapter_update_if_active_fake.custom_fake = fake_accesskit_unix_adapter_update_if_active;
    accesskitInit(app);

    accesskitSpeak(app->appRenderer, "First");
    TEST_ASSERT_EQUAL_STRING("First", captured_speak_text);

    accesskitSpeak(app->appRenderer, "Second");
    TEST_ASSERT_EQUAL_STRING("Second", captured_speak_text);

    accesskitSpeak(app->appRenderer, "Third");
    TEST_ASSERT_EQUAL_STRING("Third", captured_speak_text);

    TEST_ASSERT_EQUAL_INT(3, accesskit_unix_adapter_update_if_active_fake.call_count);

    destroyTestApp(app);
}

/* ============================================
 * Integration tests
 * ============================================ */

void test_accesskit_lifecycle_init_speak_destroy(void) {
    SiCompassApplication *app = createTestApp();
    accesskit_unix_adapter_new_fake.custom_fake = fake_accesskit_unix_adapter_new;
    accesskit_unix_adapter_update_if_active_fake.custom_fake = fake_accesskit_unix_adapter_update_if_active;

    // Initialize
    accesskitInit(app);
    TEST_ASSERT_NOT_NULL(app->appRenderer->accesskitAdapter);
    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_ROOT_ID, app->appRenderer->accesskitRootId);
    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_LIVE_REGION_ID, app->appRenderer->accesskitLiveRegionId);

    // Speak
    accesskitSpeak(app->appRenderer, "Application started");
    TEST_ASSERT_EQUAL_STRING("Application started", captured_speak_text);

    // Destroy
    accesskitDestroy(app->appRenderer);
    TEST_ASSERT_NULL(app->appRenderer->accesskitAdapter);
    TEST_ASSERT_EQUAL_INT(1, accesskit_unix_adapter_free_fake.call_count);

    free(app->appRenderer);
    free(app);
}

void test_accesskit_speak_after_destroy_does_nothing(void) {
    SiCompassApplication *app = createTestApp();
    accesskit_unix_adapter_new_fake.custom_fake = fake_accesskit_unix_adapter_new;
    accesskit_unix_adapter_update_if_active_fake.custom_fake = fake_accesskit_unix_adapter_update_if_active;

    accesskitInit(app);
    accesskitDestroy(app->appRenderer);

    int call_count_before = accesskit_unix_adapter_update_if_active_fake.call_count;

    // Should not crash or call update after destroy
    accesskitSpeak(app->appRenderer, "This should not be spoken");

    TEST_ASSERT_EQUAL_INT(call_count_before, accesskit_unix_adapter_update_if_active_fake.call_count);

    free(app->appRenderer);
    free(app);
}

/* ============================================
 * Tree population tests - accesskitActivationHandler
 * ============================================ */

void test_activation_handler_creates_tree_update_with_capacity(void) {
    accesskitActivationHandler(NULL);

    TEST_ASSERT_EQUAL_INT(1, accesskit_tree_update_with_capacity_and_focus_fake.call_count);
    TEST_ASSERT_EQUAL_UINT64(2, accesskit_tree_update_with_capacity_and_focus_fake.arg0_val);
    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_ROOT_ID, accesskit_tree_update_with_capacity_and_focus_fake.arg1_val);
}

void test_activation_handler_creates_root_node_with_window_role(void) {
    accesskitActivationHandler(NULL);

    // First call to accesskit_node_new should be for root (WINDOW role)
    TEST_ASSERT_GREATER_OR_EQUAL_INT(1, accesskit_node_new_fake.call_count);
    TEST_ASSERT_EQUAL_INT(ACCESSKIT_ROLE_WINDOW, accesskit_node_new_fake.arg0_history[0]);
}

void test_activation_handler_sets_root_label(void) {
    accesskitActivationHandler(NULL);

    // First call to set_label should be for root
    TEST_ASSERT_GREATER_OR_EQUAL_INT(1, accesskit_node_set_label_fake.call_count);
    TEST_ASSERT_EQUAL_STRING("Silicon's Compass", accesskit_node_set_label_fake.arg1_history[0]);
}

void test_activation_handler_sets_root_children(void) {
    accesskitActivationHandler(NULL);

    TEST_ASSERT_EQUAL_INT(1, accesskit_node_set_children_fake.call_count);
    TEST_ASSERT_EQUAL_UINT64(1, accesskit_node_set_children_fake.arg1_val);
}

void test_activation_handler_creates_live_region_with_label_role(void) {
    accesskitActivationHandler(NULL);

    // Second call to accesskit_node_new should be for live region (LABEL role)
    TEST_ASSERT_GREATER_OR_EQUAL_INT(2, accesskit_node_new_fake.call_count);
    TEST_ASSERT_EQUAL_INT(ACCESSKIT_ROLE_LABEL, accesskit_node_new_fake.arg0_history[1]);
}

void test_activation_handler_sets_live_region_live_polite(void) {
    accesskitActivationHandler(NULL);

    TEST_ASSERT_EQUAL_INT(1, accesskit_node_set_live_fake.call_count);
    TEST_ASSERT_EQUAL_INT(ACCESSKIT_LIVE_POLITE, accesskit_node_set_live_fake.arg1_val);
}

void test_activation_handler_sets_live_region_empty_label(void) {
    accesskitActivationHandler(NULL);

    // Second call to set_label should be empty string for live region
    TEST_ASSERT_GREATER_OR_EQUAL_INT(2, accesskit_node_set_label_fake.call_count);
    TEST_ASSERT_EQUAL_STRING("", accesskit_node_set_label_fake.arg1_history[1]);
}

void test_activation_handler_pushes_two_nodes(void) {
    accesskitActivationHandler(NULL);

    TEST_ASSERT_EQUAL_INT(2, accesskit_tree_update_push_node_fake.call_count);
    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_ROOT_ID, accesskit_tree_update_push_node_fake.arg1_history[0]);
    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_LIVE_REGION_ID, accesskit_tree_update_push_node_fake.arg1_history[1]);
}

void test_activation_handler_creates_tree_with_root_id(void) {
    accesskitActivationHandler(NULL);

    TEST_ASSERT_EQUAL_INT(1, accesskit_tree_new_fake.call_count);
    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_ROOT_ID, accesskit_tree_new_fake.arg0_val);
}

void test_activation_handler_sets_toolkit_name(void) {
    accesskitActivationHandler(NULL);

    TEST_ASSERT_EQUAL_INT(1, accesskit_tree_set_toolkit_name_fake.call_count);
    TEST_ASSERT_EQUAL_STRING("sicompass", accesskit_tree_set_toolkit_name_fake.arg1_val);
}

void test_activation_handler_sets_toolkit_version(void) {
    accesskitActivationHandler(NULL);

    TEST_ASSERT_EQUAL_INT(1, accesskit_tree_set_toolkit_version_fake.call_count);
    TEST_ASSERT_EQUAL_STRING("0.1", accesskit_tree_set_toolkit_version_fake.arg1_val);
}

void test_activation_handler_sets_tree_on_update(void) {
    accesskitActivationHandler(NULL);

    TEST_ASSERT_EQUAL_INT(1, accesskit_tree_update_set_tree_fake.call_count);
}

/* ============================================
 * Tree population tests - accesskitSpeakUpdateFactory
 * ============================================ */

void test_speak_factory_creates_tree_update_with_focus(void) {
    accesskitSpeakUpdateFactory("Test text");

    TEST_ASSERT_EQUAL_INT(1, accesskit_tree_update_with_focus_fake.call_count);
    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_ROOT_ID, accesskit_tree_update_with_focus_fake.arg0_val);
}

void test_speak_factory_creates_live_region_node(void) {
    accesskitSpeakUpdateFactory("Test text");

    TEST_ASSERT_EQUAL_INT(1, accesskit_node_new_fake.call_count);
    TEST_ASSERT_EQUAL_INT(ACCESSKIT_ROLE_LABEL, accesskit_node_new_fake.arg0_val);
}

void test_speak_factory_sets_live_polite(void) {
    accesskitSpeakUpdateFactory("Test text");

    TEST_ASSERT_EQUAL_INT(1, accesskit_node_set_live_fake.call_count);
    TEST_ASSERT_EQUAL_INT(ACCESSKIT_LIVE_POLITE, accesskit_node_set_live_fake.arg1_val);
}

void test_speak_factory_sets_label_to_text(void) {
    const char *text = "Hello accessibility";
    accesskitSpeakUpdateFactory((void*)text);

    TEST_ASSERT_EQUAL_INT(1, accesskit_node_set_label_fake.call_count);
    TEST_ASSERT_EQUAL_STRING(text, accesskit_node_set_label_fake.arg1_val);
}

void test_speak_factory_pushes_live_region_node(void) {
    accesskitSpeakUpdateFactory("Test text");

    TEST_ASSERT_EQUAL_INT(1, accesskit_tree_update_push_node_fake.call_count);
    TEST_ASSERT_EQUAL_UINT64(ACCESSKIT_LIVE_REGION_ID, accesskit_tree_update_push_node_fake.arg1_val);
}

void test_accesskitSpeak_invokes_factory_with_correct_tree(void) {
    SiCompassApplication *app = createTestApp();
    accesskit_unix_adapter_new_fake.custom_fake = fake_accesskit_unix_adapter_new;
    accesskit_unix_adapter_update_if_active_fake.custom_fake = fake_accesskit_unix_adapter_update_if_active_invoke;
    accesskitInit(app);

    // Reset mocks after init to only capture speak calls
    RESET_FAKE(accesskit_tree_update_with_focus);
    RESET_FAKE(accesskit_node_new);
    RESET_FAKE(accesskit_node_set_live);
    RESET_FAKE(accesskit_node_set_label);
    RESET_FAKE(accesskit_tree_update_push_node);

    accesskitSpeak(app->appRenderer, "Announcement");

    // Verify the speak update factory was invoked and built the tree
    TEST_ASSERT_EQUAL_INT(1, accesskit_tree_update_with_focus_fake.call_count);
    TEST_ASSERT_EQUAL_INT(1, accesskit_node_new_fake.call_count);
    TEST_ASSERT_EQUAL_INT(ACCESSKIT_ROLE_LABEL, accesskit_node_new_fake.arg0_val);
    TEST_ASSERT_EQUAL_STRING("Announcement", accesskit_node_set_label_fake.arg1_val);

    destroyTestApp(app);
}

/* ============================================
 * Tree population tests - accesskitActionHandler
 * ============================================ */

void test_action_handler_frees_request(void) {
    struct accesskit_action_request *request = (struct accesskit_action_request*)0x12345678;

    accesskitActionHandler(request, NULL);

    TEST_ASSERT_EQUAL_INT(1, accesskit_action_request_free_fake.call_count);
    TEST_ASSERT_EQUAL_PTR(request, accesskit_action_request_free_fake.arg0_val);
}

/* ============================================
 * Main - Run all tests
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // accesskitInit tests
    RUN_TEST(test_accesskitInit_sets_root_id);
    RUN_TEST(test_accesskitInit_sets_live_region_id);
    RUN_TEST(test_accesskitInit_creates_adapter);
    RUN_TEST(test_accesskitInit_adapter_is_null_when_creation_fails);

    // accesskitDestroy tests
    RUN_TEST(test_accesskitDestroy_frees_adapter);
    RUN_TEST(test_accesskitDestroy_handles_null_adapter);
    RUN_TEST(test_accesskitDestroy_sets_adapter_to_null);

    // accesskitSpeak tests
    RUN_TEST(test_accesskitSpeak_calls_update_if_active);
    RUN_TEST(test_accesskitSpeak_passes_text_to_update);
    RUN_TEST(test_accesskitSpeak_does_nothing_with_null_adapter);
    RUN_TEST(test_accesskitSpeak_does_nothing_with_null_text);
    RUN_TEST(test_accesskitSpeak_with_empty_string);
    RUN_TEST(test_accesskitSpeak_multiple_announcements);

    // Integration tests
    RUN_TEST(test_accesskit_lifecycle_init_speak_destroy);
    RUN_TEST(test_accesskit_speak_after_destroy_does_nothing);

    // Tree population tests - accesskitActivationHandler
    RUN_TEST(test_activation_handler_creates_tree_update_with_capacity);
    RUN_TEST(test_activation_handler_creates_root_node_with_window_role);
    RUN_TEST(test_activation_handler_sets_root_label);
    RUN_TEST(test_activation_handler_sets_root_children);
    RUN_TEST(test_activation_handler_creates_live_region_with_label_role);
    RUN_TEST(test_activation_handler_sets_live_region_live_polite);
    RUN_TEST(test_activation_handler_sets_live_region_empty_label);
    RUN_TEST(test_activation_handler_pushes_two_nodes);
    RUN_TEST(test_activation_handler_creates_tree_with_root_id);
    RUN_TEST(test_activation_handler_sets_toolkit_name);
    RUN_TEST(test_activation_handler_sets_toolkit_version);
    RUN_TEST(test_activation_handler_sets_tree_on_update);

    // Tree population tests - accesskitSpeakUpdateFactory
    RUN_TEST(test_speak_factory_creates_tree_update_with_focus);
    RUN_TEST(test_speak_factory_creates_live_region_node);
    RUN_TEST(test_speak_factory_sets_live_polite);
    RUN_TEST(test_speak_factory_sets_label_to_text);
    RUN_TEST(test_speak_factory_pushes_live_region_node);
    RUN_TEST(test_accesskitSpeak_invokes_factory_with_correct_tree);

    // Tree population tests - accesskitActionHandler
    RUN_TEST(test_action_handler_frees_request);

    return UNITY_END();
}

/*
 * Tests for caret.c functions:
 * - caretCreate
 * - caretDestroy
 * - caretUpdate
 * - caretReset
 */

#include <unity.h>
#include <fff/fff.h>
#include <stdlib.h>
#include <stdbool.h>
#include <stdint.h>

DEFINE_FFF_GLOBALS;

// Mock SDL_GetTicks
FAKE_VALUE_FUNC(uint64_t, SDL_GetTicks);

#define DEFAULT_BLINK_INTERVAL 800

// CaretState definition (private struct from caret.c)
typedef struct CaretState {
    bool visible;
    uint64_t lastBlinkTime;
    uint32_t blinkInterval;
} CaretState;

/* ============================================
 * Functions under test (from caret.c)
 * ============================================ */

CaretState* caretCreate(void) {
    CaretState* caret = calloc(1, sizeof(CaretState));
    if (!caret) return NULL;

    caret->visible = true;
    caret->lastBlinkTime = SDL_GetTicks();
    caret->blinkInterval = DEFAULT_BLINK_INTERVAL;

    return caret;
}

void caretDestroy(CaretState* caret) {
    if (caret) {
        free(caret);
    }
}

void caretUpdate(CaretState* caret, uint64_t currentTime) {
    if (currentTime - caret->lastBlinkTime >= caret->blinkInterval) {
        caret->visible = !caret->visible;
        caret->lastBlinkTime = currentTime;
    }
}

void caretReset(CaretState* caret, uint64_t currentTime) {
    caret->visible = true;
    caret->lastBlinkTime = currentTime;
}

/* ============================================
 * Unity Setup/Teardown
 * ============================================ */

void setUp(void) {
    RESET_FAKE(SDL_GetTicks);
    FFF_RESET_HISTORY();
}

void tearDown(void) {}

/* ============================================
 * caretCreate tests
 * ============================================ */

void test_caretCreate_returns_non_null(void) {
    SDL_GetTicks_fake.return_val = 1000;
    CaretState *caret = caretCreate();
    TEST_ASSERT_NOT_NULL(caret);
    caretDestroy(caret);
}

void test_caretCreate_initial_visible(void) {
    SDL_GetTicks_fake.return_val = 1000;
    CaretState *caret = caretCreate();
    TEST_ASSERT_TRUE(caret->visible);
    caretDestroy(caret);
}

void test_caretCreate_stores_initial_time(void) {
    SDL_GetTicks_fake.return_val = 5000;
    CaretState *caret = caretCreate();
    TEST_ASSERT_EQUAL_UINT64(5000, caret->lastBlinkTime);
    caretDestroy(caret);
}

void test_caretCreate_sets_blink_interval(void) {
    SDL_GetTicks_fake.return_val = 0;
    CaretState *caret = caretCreate();
    TEST_ASSERT_EQUAL_UINT32(DEFAULT_BLINK_INTERVAL, caret->blinkInterval);
    caretDestroy(caret);
}

void test_caretCreate_calls_SDL_GetTicks(void) {
    SDL_GetTicks_fake.return_val = 0;
    CaretState *caret = caretCreate();
    TEST_ASSERT_EQUAL_INT(1, SDL_GetTicks_fake.call_count);
    caretDestroy(caret);
}

/* ============================================
 * caretDestroy tests
 * ============================================ */

void test_caretDestroy_null_safe(void) {
    caretDestroy(NULL); // Should not crash
}

void test_caretDestroy_frees_caret(void) {
    SDL_GetTicks_fake.return_val = 0;
    CaretState *caret = caretCreate();
    TEST_ASSERT_NOT_NULL(caret);
    caretDestroy(caret); // Should not crash or leak
}

/* ============================================
 * caretUpdate tests
 * ============================================ */

void test_caretUpdate_no_blink_before_interval(void) {
    SDL_GetTicks_fake.return_val = 0;
    CaretState *caret = caretCreate();

    // Update before interval elapsed
    caretUpdate(caret, 500);
    TEST_ASSERT_TRUE(caret->visible);

    caretDestroy(caret);
}

void test_caretUpdate_blinks_at_interval(void) {
    SDL_GetTicks_fake.return_val = 0;
    CaretState *caret = caretCreate();

    // Update exactly at interval
    caretUpdate(caret, DEFAULT_BLINK_INTERVAL);
    TEST_ASSERT_FALSE(caret->visible);

    caretDestroy(caret);
}

void test_caretUpdate_blinks_after_interval(void) {
    SDL_GetTicks_fake.return_val = 0;
    CaretState *caret = caretCreate();

    // Update past interval
    caretUpdate(caret, DEFAULT_BLINK_INTERVAL + 100);
    TEST_ASSERT_FALSE(caret->visible);

    caretDestroy(caret);
}

void test_caretUpdate_toggles_visibility(void) {
    SDL_GetTicks_fake.return_val = 0;
    CaretState *caret = caretCreate();

    // First blink: visible -> invisible
    caretUpdate(caret, DEFAULT_BLINK_INTERVAL);
    TEST_ASSERT_FALSE(caret->visible);

    // Second blink: invisible -> visible
    caretUpdate(caret, DEFAULT_BLINK_INTERVAL * 2);
    TEST_ASSERT_TRUE(caret->visible);

    // Third blink: visible -> invisible
    caretUpdate(caret, DEFAULT_BLINK_INTERVAL * 3);
    TEST_ASSERT_FALSE(caret->visible);

    caretDestroy(caret);
}

void test_caretUpdate_updates_lastBlinkTime(void) {
    SDL_GetTicks_fake.return_val = 1000;
    CaretState *caret = caretCreate();

    caretUpdate(caret, 1000 + DEFAULT_BLINK_INTERVAL);
    TEST_ASSERT_EQUAL_UINT64(1000 + DEFAULT_BLINK_INTERVAL, caret->lastBlinkTime);

    caretDestroy(caret);
}

void test_caretUpdate_no_update_before_interval_keeps_time(void) {
    SDL_GetTicks_fake.return_val = 1000;
    CaretState *caret = caretCreate();

    caretUpdate(caret, 1500);
    TEST_ASSERT_EQUAL_UINT64(1000, caret->lastBlinkTime);

    caretDestroy(caret);
}

/* ============================================
 * caretReset tests
 * ============================================ */

void test_caretReset_sets_visible(void) {
    SDL_GetTicks_fake.return_val = 0;
    CaretState *caret = caretCreate();

    // Blink to invisible
    caretUpdate(caret, DEFAULT_BLINK_INTERVAL);
    TEST_ASSERT_FALSE(caret->visible);

    // Reset
    caretReset(caret, DEFAULT_BLINK_INTERVAL + 100);
    TEST_ASSERT_TRUE(caret->visible);

    caretDestroy(caret);
}

void test_caretReset_updates_lastBlinkTime(void) {
    SDL_GetTicks_fake.return_val = 0;
    CaretState *caret = caretCreate();

    caretReset(caret, 5000);
    TEST_ASSERT_EQUAL_UINT64(5000, caret->lastBlinkTime);

    caretDestroy(caret);
}

void test_caretReset_already_visible(void) {
    SDL_GetTicks_fake.return_val = 0;
    CaretState *caret = caretCreate();

    // Already visible, reset should keep visible
    caretReset(caret, 100);
    TEST_ASSERT_TRUE(caret->visible);

    caretDestroy(caret);
}

void test_caretReset_restarts_blink_cycle(void) {
    SDL_GetTicks_fake.return_val = 0;
    CaretState *caret = caretCreate();

    // Blink to invisible
    caretUpdate(caret, DEFAULT_BLINK_INTERVAL);
    TEST_ASSERT_FALSE(caret->visible);

    // Reset at 1000
    caretReset(caret, 1000);
    TEST_ASSERT_TRUE(caret->visible);

    // Should not blink again until 1000 + interval
    caretUpdate(caret, 1000 + DEFAULT_BLINK_INTERVAL - 1);
    TEST_ASSERT_TRUE(caret->visible);

    // Now should blink
    caretUpdate(caret, 1000 + DEFAULT_BLINK_INTERVAL);
    TEST_ASSERT_FALSE(caret->visible);

    caretDestroy(caret);
}

/* ============================================
 * Main
 * ============================================ */

int main(void) {
    UNITY_BEGIN();

    // caretCreate
    RUN_TEST(test_caretCreate_returns_non_null);
    RUN_TEST(test_caretCreate_initial_visible);
    RUN_TEST(test_caretCreate_stores_initial_time);
    RUN_TEST(test_caretCreate_sets_blink_interval);
    RUN_TEST(test_caretCreate_calls_SDL_GetTicks);

    // caretDestroy
    RUN_TEST(test_caretDestroy_null_safe);
    RUN_TEST(test_caretDestroy_frees_caret);

    // caretUpdate
    RUN_TEST(test_caretUpdate_no_blink_before_interval);
    RUN_TEST(test_caretUpdate_blinks_at_interval);
    RUN_TEST(test_caretUpdate_blinks_after_interval);
    RUN_TEST(test_caretUpdate_toggles_visibility);
    RUN_TEST(test_caretUpdate_updates_lastBlinkTime);
    RUN_TEST(test_caretUpdate_no_update_before_interval_keeps_time);

    // caretReset
    RUN_TEST(test_caretReset_sets_visible);
    RUN_TEST(test_caretReset_updates_lastBlinkTime);
    RUN_TEST(test_caretReset_already_visible);
    RUN_TEST(test_caretReset_restarts_blink_cycle);

    return UNITY_END();
}

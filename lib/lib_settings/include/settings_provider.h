#pragma once

#include <provider_interface.h>

/**
 * Callback invoked when a setting value changes (on load and on user interaction).
 * key: setting name (e.g. "colorScheme")
 * value: new value (e.g. "dark" or "light")
 */
typedef void (*SettingsApplyFn)(const char *key, const char *value, void *userdata);

/**
 * Create the settings provider.
 *
 * The provider exposes a hierarchical settings tree:
 *   settings
 *   └── sicompass
 *       └── <radio>color scheme
 *           ├── dark
 *           └── light
 *
 * On init it reads ~/.config/sicompass/providers/settings.json and calls
 * applyCallback for each stored setting. On radio selection changes it saves
 * the config and calls applyCallback immediately.
 */
Provider* settingsProviderCreate(SettingsApplyFn applyCallback, void *userdata);

/**
 * Register an additional section in the settings tree.
 * Must be called before providerInitAll() / providerGetInitialElement().
 * The section appears as a child of the settings root with a "no settings" placeholder.
 */
void settingsAddSection(Provider *provider, const char *sectionName);

/**
 * Remove a section and all its entries (radio, text, checkbox) from the settings tree.
 * Used when hot-disabling a program to clean up its settings.
 */
void settingsRemoveSection(Provider *provider, const char *sectionName);

/**
 * Register a radio group as a direct child of a named section in the settings tree.
 * The section is created automatically (no separate settingsAddSection call needed).
 * radioKey becomes the display label of the radio group (e.g. "global sorting").
 * configKey is used for JSON persistence and as the applyCallback key (e.g. "sortOrder").
 * options/optionCount list the selectable values; defaultValue is used when no config exists.
 * Must be called before providerInitAll() / providerGetInitialElement().
 */
void settingsAddSectionRadio(Provider *provider,
                             const char *sectionName,
                             const char *radioKey,
                             const char *configKey,
                             const char **options,
                             int optionCount,
                             const char *defaultValue);

/**
 * Register a text input field as a direct child of a named section in the settings tree.
 * The section is created automatically (no separate settingsAddSection call needed).
 * label becomes the display label of the containing object (e.g. "save folder").
 * configKey is used for JSON persistence and as the applyCallback key (e.g. "saveFolder").
 * defaultValue is the initial text shown in the input field.
 * Must be called before providerInitAll() / providerGetInitialElement().
 */
void settingsAddSectionText(Provider *provider,
                            const char *sectionName,
                            const char *label,
                            const char *configKey,
                            const char *defaultValue);

/**
 * Mark a section as "priority" so it renders before the sicompass section.
 * The section is also registered automatically (no separate settingsAddSection call needed).
 * Must be called before providerInitAll() / providerGetInitialElement().
 */
void settingsAddPrioritySection(Provider *provider, const char *sectionName);

/**
 * Register a checkbox entry as a direct child of a named section in the settings tree.
 * The section is created automatically (no separate settingsAddSection call needed).
 * label becomes the display text of the checkbox (e.g. "tutorial").
 * configKey is used for JSON persistence and as the applyCallback key (e.g. "enable_tutorial").
 * defaultChecked is the initial state when no config exists.
 * Must be called before providerInitAll() / providerGetInitialElement().
 */
void settingsAddSectionCheckbox(Provider *provider,
                                const char *sectionName,
                                const char *label,
                                const char *configKey,
                                bool defaultChecked);

/* SPDX-License-Identifier: GPL-3.0-or-later */
#include "debug_ui.h"

#include <aurora/overlay.h>
#include <SDL3/SDL_events.h>
#include <SDL3/SDL_gamepad.h>
#include <SDL3/SDL_timer.h>
#include <SDL3/SDL_video.h>
#include <stdint.h>
#include <stdlib.h>

typedef struct DebugUi DebugUi;

typedef enum {
    DEBUG_UI_POINTER_MOVED,
    DEBUG_UI_POINTER_BUTTON,
    DEBUG_UI_POINTER_GONE,
    DEBUG_UI_SCROLL,
    DEBUG_UI_NAV,
} DebugUiEventKind;

typedef enum {
    DEBUG_UI_NAV_UP,
    DEBUG_UI_NAV_DOWN,
    DEBUG_UI_NAV_LEFT,
    DEBUG_UI_NAV_RIGHT,
    DEBUG_UI_NAV_ACTIVATE,
    DEBUG_UI_NAV_BACK,
} DebugUiNav;

#define STICK_NAV_THRESHOLD 18000
#define STICK_NAV_REPEAT_MS 200

typedef struct {
    uint32_t kind;
    float x;
    float y;
    uint32_t button;
    bool pressed;
} DebugUiEvent;

DebugUi* debug_ui_create(void);
void debug_ui_toggle(const DebugUi* ui);
void debug_ui_toggle_overlays(const DebugUi* ui);
void debug_ui_toggle_pad_capture(const DebugUi* ui);
bool debug_ui_captures_pad(const DebugUi* ui);
void debug_ui_event(const DebugUi* ui, const DebugUiEvent* event);
void debug_ui_run(const DebugUi* ui, float width_points, float height_points, float pixels_per_point);
void debug_ui_paint(const AuroraOverlayFrame* frame, void* user);

static DebugUi* s_ui;
static SDL_Window* s_window;

void pc_debug_ui_init(SDL_Window* window) {
    s_window = window;
    s_ui = debug_ui_create();
    aurora_set_overlay_callback(debug_ui_paint, s_ui);
    if (getenv("MELEE_DEBUG_UI_OPEN") != NULL) {
        debug_ui_toggle(s_ui);
    }
    if (getenv("MELEE_DEBUG_OVERLAYS") != NULL) {
        debug_ui_toggle_overlays(s_ui);
    }
}

static bool nav_from_button(SDL_GamepadButton button, DebugUiNav* nav) {
    switch (button) {
    case SDL_GAMEPAD_BUTTON_DPAD_UP:
        *nav = DEBUG_UI_NAV_UP;
        return true;
    case SDL_GAMEPAD_BUTTON_DPAD_DOWN:
        *nav = DEBUG_UI_NAV_DOWN;
        return true;
    case SDL_GAMEPAD_BUTTON_DPAD_LEFT:
        *nav = DEBUG_UI_NAV_LEFT;
        return true;
    case SDL_GAMEPAD_BUTTON_DPAD_RIGHT:
        *nav = DEBUG_UI_NAV_RIGHT;
        return true;
    case SDL_GAMEPAD_BUTTON_SOUTH:
        *nav = DEBUG_UI_NAV_ACTIVATE;
        return true;
    case SDL_GAMEPAD_BUTTON_EAST:
        *nav = DEBUG_UI_NAV_BACK;
        return true;
    default:
        return false;
    }
}

static bool nav_from_axis(const SDL_GamepadAxisEvent* axis, DebugUiNav* nav) {
    static Uint64 last_x;
    static Uint64 last_y;
    const Uint64 now = SDL_GetTicks();
    if (axis->value > -STICK_NAV_THRESHOLD && axis->value < STICK_NAV_THRESHOLD) {
        return false;
    }
    if (axis->axis == SDL_GAMEPAD_AXIS_LEFTY && now - last_y > STICK_NAV_REPEAT_MS) {
        last_y = now;
        *nav = axis->value > 0 ? DEBUG_UI_NAV_DOWN : DEBUG_UI_NAV_UP;
        return true;
    }
    if (axis->axis == SDL_GAMEPAD_AXIS_LEFTX && now - last_x > STICK_NAV_REPEAT_MS) {
        last_x = now;
        *nav = axis->value > 0 ? DEBUG_UI_NAV_RIGHT : DEBUG_UI_NAV_LEFT;
        return true;
    }
    return false;
}

void pc_debug_ui_event(const SDL_Event* e) {
    DebugUiEvent event = { 0 };
    DebugUiNav nav;
    switch (e->type) {
    case SDL_EVENT_GAMEPAD_BUTTON_DOWN:
        if (e->gbutton.button == SDL_GAMEPAD_BUTTON_RIGHT_STICK) {
            debug_ui_toggle_pad_capture(s_ui);
            return;
        }
        if (!nav_from_button(e->gbutton.button, &nav)) {
            return;
        }
        event.kind = DEBUG_UI_NAV;
        event.button = nav;
        break;
    case SDL_EVENT_GAMEPAD_AXIS_MOTION:
        if (!nav_from_axis(&e->gaxis, &nav)) {
            return;
        }
        event.kind = DEBUG_UI_NAV;
        event.button = nav;
        break;
    case SDL_EVENT_KEY_DOWN:
        if (e->key.scancode == SDL_SCANCODE_F2 && !e->key.repeat) {
            debug_ui_toggle(s_ui);
        }
        if (e->key.scancode == SDL_SCANCODE_F3 && !e->key.repeat) {
            debug_ui_toggle_overlays(s_ui);
        }
        return;
    case SDL_EVENT_MOUSE_MOTION:
        event.kind = DEBUG_UI_POINTER_MOVED;
        event.x = e->motion.x;
        event.y = e->motion.y;
        break;
    case SDL_EVENT_MOUSE_BUTTON_DOWN:
    case SDL_EVENT_MOUSE_BUTTON_UP:
        event.kind = DEBUG_UI_POINTER_BUTTON;
        event.x = e->button.x;
        event.y = e->button.y;
        event.button = e->button.button;
        event.pressed = e->button.down;
        break;
    case SDL_EVENT_MOUSE_WHEEL:
        event.kind = DEBUG_UI_SCROLL;
        event.x = e->wheel.x;
        event.y = e->wheel.y;
        break;
    case SDL_EVENT_WINDOW_MOUSE_LEAVE:
        event.kind = DEBUG_UI_POINTER_GONE;
        break;
    default:
        return;
    }
    debug_ui_event(s_ui, &event);
}

void pc_debug_ui_update(void) {
    int width = 0;
    int height = 0;
    int pixel_width = 0;
    if (!SDL_GetWindowSize(s_window, &width, &height) ||
        !SDL_GetWindowSizeInPixels(s_window, &pixel_width, NULL) || width <= 0) {
        return;
    }
    debug_ui_run(s_ui, (float)width, (float)height, (float)pixel_width / (float)width);
}

bool pc_debug_ui_captures_pad(void) {
    return debug_ui_captures_pad(s_ui);
}

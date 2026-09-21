/* SPDX-License-Identifier: GPL-3.0-or-later */
#include <aurora/aurora.h>
#include <aurora/event.h>
#include <aurora/main.h>
#include <dolphin/gx.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <SDL3/SDL_stdinc.h>

#include "pc/debug_ui.h"

typedef struct {
    float x, y, z;
} Vec3;

static int s_stocks[6] = { 4, 3, 0, 0, 0, 0 };
static uint32_t s_sim_hz = 60;

int Player_GetPlayerSlotType(int32_t slot) {
    return slot == 0 ? 0 : slot == 1 ? 1 : 3;
}
typedef struct {
    uint32_t buttons;
    float stick_x, stick_y, cstick_x, cstick_y, trigger_l, trigger_r;
    bool connected;
} PcDebugPad;

static int s_frame;

void pc_debug_pad(int port, PcDebugPad* out) {
    const float t = (float)s_frame / 30.0f;
    *out = (PcDebugPad){ 0 };
    out->connected = port < 2;
    if (port == 0) {
        out->buttons = (s_frame / 20 % 2 ? 0x0100 : 0) | (s_frame / 45 % 2 ? 0x0800 : 0) | (s_frame / 70 % 2 ? 0x0010 : 0);
        out->stick_x = SDL_cosf(t);
        out->stick_y = SDL_sinf(t);
        out->trigger_r = (SDL_sinf(t * 0.7f) + 1.0f) * 0.5f;
    } else if (port == 1) {
        out->buttons = (s_frame / 33 % 2 ? 0x0200 : 0) | (s_frame / 50 % 2 ? 0x0008 : 0) | (out->trigger_l > 0.9f ? 0x0040 : 0);
        out->cstick_x = SDL_sinf(t * 1.3f);
        out->cstick_y = -0.4f;
        out->trigger_l = 1.0f;
        out->buttons |= 0x0040;
    }
}

bool pc_debug_fighter_alive(int slot) {
    return slot < 2;
}
int32_t Player_GetKOsByPlayerIndex(int slot, int idx) {
    if (slot == 0 && idx == 1) {
        return 1 + s_frame / 150;
    }
    return slot == 1 && idx == 1 ? s_frame / 260 : 0;
}
int Player_GetPlayerCharacter(int slot) {
    return slot == 0 ? 0x02 : 0x09;
}
int32_t Player_GetStocks(int slot) {
    return s_stocks[slot];
}
void Player_SetStocks(int slot, int stocks) {
    s_stocks[slot] = stocks;
}
int32_t Player_GetDamage(int32_t slot) {
    return 42 * (slot + 1) + (slot == 1 ? s_frame / 40 * 7 : 0);
}
void Player_LoadPlayerCoords(int32_t slot, Vec3* out) {
    out->x = -30.0f + 60.0f * (float)slot;
    out->y = 12.5f;
    out->z = 0.0f;
}
uint8_t gm_GetCurrentGameMode(void) {
    return 2;
}
int32_t gm_80180AE4(void) {
    return 0;
}
uint8_t gm_GetCurrentSceneIndex(void) {
    return 2;
}
uint32_t pc_get_sim_hz(void) {
    return s_sim_hz;
}
void pc_set_sim_hz(uint32_t hz) {
    s_sim_hz = hz;
}

static void press(SDL_GamepadButton button) {
    const SDL_Event e = { .gbutton = { .type = SDL_EVENT_GAMEPAD_BUTTON_DOWN, .button = button } };
    pc_debug_ui_event(&e);
}

static void pad_script(int frame) {
    static const SDL_GamepadButton script[] = {
        SDL_GAMEPAD_BUTTON_RIGHT_STICK, SDL_GAMEPAD_BUTTON_DPAD_DOWN,  SDL_GAMEPAD_BUTTON_DPAD_DOWN,
        SDL_GAMEPAD_BUTTON_DPAD_RIGHT,  SDL_GAMEPAD_BUTTON_DPAD_RIGHT, SDL_GAMEPAD_BUTTON_DPAD_RIGHT,
        SDL_GAMEPAD_BUTTON_EAST,
    };
    const int step = frame / 10 - 1;
    if (frame % 10 == 0 && step >= 0 && step < (int)(sizeof(script) / sizeof(script[0]))) {
        press(script[step]);
    }
}

int main(int argc, char* argv[]) {
    const int frames = argc > 1 ? atoi(argv[1]) : 0;
    const bool scripted_pad = argc > 2 && strcmp(argv[2], "pad") == 0;
    const AuroraConfig config = {
        .appName = "debug-ui-smoke",
        .windowWidth = 960,
        .windowHeight = 720,
        .vsync = getenv("MELEE_CAPTURE") == NULL,
        .captureReadback = getenv("MELEE_CAPTURE") != NULL,
    };
    const AuroraInfo info = aurora_initialize(argc, argv, &config);
    pc_debug_ui_init(info.window);

    const bool overlays = argc > 2 && strcmp(argv[2], "overlays") == 0;
    const SDL_Event open = {
        .key = { .type = SDL_EVENT_KEY_DOWN, .scancode = overlays ? SDL_SCANCODE_F3 : SDL_SCANCODE_F2 }
    };
    pc_debug_ui_event(&open);

    for (int frame = 0; frames == 0 || frame < frames; frame++) {
        const AuroraEvent* event = aurora_update();
        for (; event != NULL && event->type != AURORA_NONE; ++event) {
            if (event->type == AURORA_EXIT) {
                pc_debug_ui_capture_finish();
                aurora_shutdown();
                return 0;
            }
            if (event->type == AURORA_SDL_EVENT) {
                pc_debug_ui_event(&event->sdl);
            }
        }
        s_frame = frame;
        if (scripted_pad) {
            pad_script(frame);
        }
        pc_debug_ui_update();
        if (!aurora_begin_frame()) {
            continue;
        }
        GXSetCopyClear((GXColor){ 40, 60, 110, 255 }, GX_MAX_Z24);
        GXCopyDisp(NULL, GX_TRUE);
        aurora_end_frame();
    }
    printf("debug-ui-smoke: rendered %d frames, sim_hz=%u, captures_pad=%d\n", frames, s_sim_hz,
           pc_debug_ui_captures_pad());
    pc_debug_ui_capture_finish();
    aurora_shutdown();
    return 0;
}

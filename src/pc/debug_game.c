/* SPDX-License-Identifier: GPL-3.0-or-later */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include <melee/ft/forward.h>
#include <melee/pl/player.h>
#include <sysdolphin/baselib/controller.h>
#include <sysdolphin/baselib/gobj.h>

typedef struct {
    uint32_t buttons;
    float stick_x;
    float stick_y;
    float cstick_x;
    float cstick_y;
    float trigger_l;
    float trigger_r;
    bool connected;
} PcDebugPad;

bool pc_debug_fighter_alive(int slot) {
    const HSD_GObj* entity = Player_GetEntity(slot);
    if (entity == NULL || HSD_GObjPLinkHead == NULL) {
        return false;
    }
    for (const HSD_GObj* cur = HSD_GObjPLinkHead[HSD_GOBJ_PLINK_FIGHTER]; cur != NULL; cur = cur->next) {
        if (cur == entity) {
            return true;
        }
    }
    return false;
}

void pc_debug_pad(int port, PcDebugPad* out) {
    const HSD_PadStatus* pad = &HSD_PadCopyStatus[port];
    out->buttons = pad->button;
    out->stick_x = pad->nml_stickX;
    out->stick_y = pad->nml_stickY;
    out->cstick_x = pad->nml_subStickX;
    out->cstick_y = pad->nml_subStickY;
    out->trigger_l = pad->nml_analogL;
    out->trigger_r = pad->nml_analogR;
    out->connected = pad->err == 0;
}

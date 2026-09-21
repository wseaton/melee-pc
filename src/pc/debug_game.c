/* SPDX-License-Identifier: GPL-3.0-or-later */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include <melee/cm/camera.h>
#include <melee/ft/forward.h>
#include <melee/lb/lbvector.h>
#include <melee/pl/player.h>
#include <sysdolphin/baselib/cobj.h>
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

bool pc_debug_tag_anchor(int slot, float* x, float* y) {
    const HSD_GObj* camera = Camera_80030A50();
    if (camera == NULL || camera->hsd_obj == NULL || !pc_debug_fighter_alive(slot)) {
        return false;
    }
    HSD_CObj* cobj = camera->hsd_obj;
    const float width = cobj->viewport.xmax - cobj->viewport.xmin;
    const float height = cobj->viewport.ymax - cobj->viewport.ymin;
    if (width <= 0.0f || height <= 0.0f) {
        return false;
    }
    Vec3 world;
    Vec3 screen;
    Player_LoadPlayerCoords(slot, &world);
    world.y += Player_800360D8(slot) - 2.5f;
    if (lbVector_WorldToScreen(cobj, &world, &screen, 0) == NULL) {
        return false;
    }
    *x = (screen.x - cobj->viewport.xmin) / width;
    *y = (screen.y - cobj->viewport.ymin) / height;
    return true;
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

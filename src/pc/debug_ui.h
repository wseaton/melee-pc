/* SPDX-License-Identifier: GPL-3.0-or-later */
#ifndef PC_DEBUG_UI_H
#define PC_DEBUG_UI_H

#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

struct SDL_Window;
union SDL_Event;

void pc_debug_ui_init(struct SDL_Window* window);
void pc_debug_ui_event(const union SDL_Event* e);
void pc_debug_ui_update(void);
bool pc_debug_ui_captures_pad(void);

#ifdef __cplusplus
}
#endif

#endif /* PC_DEBUG_UI_H */

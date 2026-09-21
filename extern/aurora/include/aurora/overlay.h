#ifndef AURORA_OVERLAY_H
#define AURORA_OVERLAY_H

#include <webgpu/webgpu.h>

#ifdef __cplusplus
#include <cstdint>

extern "C" {
#else
#include "stdint.h"
#endif

typedef struct {
  WGPUDevice device;
  WGPUQueue queue;
  WGPURenderPassEncoder pass;
  WGPUTextureFormat format;
  uint32_t width;
  uint32_t height;
} AuroraOverlayFrame;

/** Runs on the thread that records the present pass, after ImGui has drawn. */
typedef void (*AuroraOverlayCallback)(const AuroraOverlayFrame* frame, void* user);

void aurora_set_overlay_callback(AuroraOverlayCallback callback, void* user);

#ifdef __cplusplus
}
#endif

#endif

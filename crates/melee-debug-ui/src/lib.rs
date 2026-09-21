mod capture;
mod events;
mod game;
mod gpu;
mod input;
mod menu;
mod overlay;
mod painter;
mod shipper;
mod trace;

use std::ffi::c_void;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use egui::{RawInput, Rect, pos2};
use melee_events::{Envelope, Event};

use crate::capture::{Capture, CaptureFrame};
use crate::events::Tracker;
use crate::game::{Player, Slot};
use crate::gpu::{WGPUDevice, WGPUQueue, WGPURenderPassEncoder, WGPUTextureFormat};
use crate::input::{InputEvent, NavAction, RawEvent, key_tap};
use crate::overlay::{Hud, View};
use crate::painter::{Frame, Painter, Target};
use crate::shipper::Shipper;
use crate::trace::Trace;

#[repr(C)]
pub struct OverlayFrame {
    device: WGPUDevice,
    queue: WGPUQueue,
    pass: WGPURenderPassEncoder,
    format: WGPUTextureFormat,
    width: u32,
    height: u32,
}

#[derive(Default)]
struct GameSide {
    visible: bool,
    overlays: bool,
    published: bool,
    pad_captured: bool,
    events: Vec<egui::Event>,
    frame_count: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum Publish {
    Draw,
    Clear,
    Nothing,
}

impl GameSide {
    fn publish_action(&mut self, drawable: bool) -> Publish {
        let wanted = drawable && (self.visible || self.overlays);
        let action = match (wanted, self.published) {
            (true, _) => Publish::Draw,
            (false, true) => Publish::Clear,
            (false, false) => Publish::Nothing,
        };
        self.published = wanted;
        action
    }

    fn toggle_visible(&mut self) {
        self.visible = !self.visible;
        self.pad_captured = false;
        self.events.clear();
    }

    fn toggle_pad_capture(&mut self) {
        self.pad_captured = !self.pad_captured;
        self.visible |= self.pad_captured;
    }

    fn push(&mut self, event: InputEvent, has_focus: bool) {
        match event {
            InputEvent::Nav(_) if !self.pad_captured => {}
            InputEvent::Nav(NavAction::Back) => self.pad_captured = false,
            InputEvent::Nav(action) => self
                .events
                .extend(action.key(has_focus).into_iter().flat_map(key_tap)),
            pointer if self.visible => self.events.extend(pointer.into_egui()),
            _ => {}
        }
    }
}

#[derive(Default)]
struct RenderSide {
    painter: Option<Painter>,
    failed: bool,
}

#[derive(Default)]
struct Monitor {
    tracker: Tracker,
    hud: Hud,
    trace: Option<Trace>,
}

struct Telemetry {
    shipper: Shipper,
    seq: u64,
}

impl Telemetry {
    fn connect(addr: String) -> Option<Self> {
        match Shipper::spawn(addr) {
            Ok(shipper) => Some(Self { shipper, seq: 0 }),
            Err(error) => {
                eprintln!("events: cannot start shipper thread: {error}");
                None
            }
        }
    }

    fn ship(&mut self, frame: u64, events: Vec<Event>) {
        let time_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            });
        for event in events {
            let envelope = Envelope {
                seq: self.seq,
                frame,
                time_ms,
                dropped: self.shipper.dropped(),
                event,
            };
            self.seq += 1;
            match serde_json::to_string(&envelope) {
                Ok(line) => self.shipper.send(line),
                Err(error) => eprintln!("events: cannot encode event: {error}"),
            }
        }
    }
}

#[derive(Default)]
pub struct DebugUi {
    telemetry: Mutex<Option<Telemetry>>,
    monitor: Mutex<Monitor>,
    ctx: egui::Context,
    game: Mutex<GameSide>,
    pending: Mutex<Option<Frame>>,
    render: Mutex<RenderSide>,
    capture: Capture,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl DebugUi {
    fn push_event(&self, event: InputEvent) {
        let has_focus = self.ctx.memory(|memory| memory.focused().is_some());
        lock(&self.game).push(event, has_focus);
    }

    fn toggle(&self) {
        lock(&self.game).toggle_visible();
    }

    fn toggle_overlays(&self) {
        let mut game = lock(&self.game);
        game.overlays = !game.overlays;
    }

    fn toggle_pad_capture(&self) {
        lock(&self.game).toggle_pad_capture();
    }

    fn captures_pad(&self) -> bool {
        lock(&self.game).pad_captured
    }

    fn run(&self, width_points: f32, height_points: f32, pixels_per_point: f32) {
        let mut game = lock(&self.game);
        game.frame_count += 1;
        let frame_count = game.frame_count;
        let mut monitor = lock(&self.monitor);
        let events = monitor.tracker.update(game::snapshot());
        for event in &events {
            monitor.hud.ingest(frame_count, event);
        }
        monitor.hud.expire(frame_count);
        if let Some(trace) = monitor.trace.as_mut() {
            let players: Vec<Player> = Slot::all().filter_map(game::player).collect();
            if let Err(error) = trace.record(frame_count, &players) {
                eprintln!("trace: {error}, disabling the trace");
                monitor.trace = None;
            }
        }
        if let Some(telemetry) = lock(&self.telemetry).as_mut() {
            telemetry.ship(frame_count, events);
            for command in telemetry.shipper.commands() {
                monitor.hud.command(frame_count, command);
            }
        }
        let drawable = width_points > 0.0 && height_points > 0.0 && pixels_per_point > 0.0;
        match game.publish_action(drawable) {
            Publish::Draw => {}
            Publish::Clear => {
                self.publish(egui::TexturesDelta::default(), Vec::new(), 1.0);
                return;
            }
            Publish::Nothing => return,
        }
        let mut viewports = egui::ViewportIdMap::default();
        viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                native_pixels_per_point: Some(pixels_per_point),
                ..Default::default()
            },
        );
        let raw_input = RawInput {
            screen_rect: Some(Rect::from_min_max(
                pos2(0.0, 0.0),
                pos2(width_points, height_points),
            )),
            viewports,
            events: std::mem::take(&mut game.events),
            ..Default::default()
        };
        let (menu_visible, overlays) = (game.visible, game.overlays);
        let view = View {
            aspect: game::presentation_aspect(),
            bag: monitor
                .hud
                .wants_bag()
                .then(|| game::tag_anchor(Slot::SANDBAG))
                .flatten(),
        };
        let output = self.ctx.run_ui(raw_input, |ui| {
            if overlays {
                monitor.hud.draw(ui.ctx(), frame_count, &game::pads(), view);
            }
            if menu_visible {
                menu::show(ui.ctx(), frame_count);
            }
        });

        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        self.publish(output.textures_delta, primitives, output.pixels_per_point);
    }

    fn publish(
        &self,
        mut textures: egui::TexturesDelta,
        primitives: Vec<egui::ClippedPrimitive>,
        pixels_per_point: f32,
    ) {
        let mut pending = lock(&self.pending);
        if let Some(mut unpainted) = pending.take() {
            let mut merged = std::mem::take(&mut unpainted.textures);
            merged.append(textures);
            textures = merged;
        }
        *pending = Some(Frame {
            textures,
            primitives,
            pixels_per_point,
        });
    }

    fn paint(&self, target: &Target) {
        let newer = lock(&self.pending).take();
        let mut render = lock(&self.render);
        if render.failed {
            return;
        }
        let result = match &mut render.painter {
            Some(painter) => painter.paint(target, newer),
            None if newer.is_none() => return,
            None => Painter::new(target.device)
                .and_then(|painter| render.painter.insert(painter).paint(target, newer)),
        };
        if let Err(error) = result {
            eprintln!("debug ui: {error}, disabling overlay");
            render.failed = true;
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn debug_ui_create() -> *mut DebugUi {
    let telemetry = std::env::var("MELEE_EVENTS_ADDR")
        .ok()
        .and_then(Telemetry::connect);
    let trace = std::env::var_os("MELEE_TRACE").and_then(|path| {
        Trace::create(Path::new(&path))
            .inspect_err(|error| eprintln!("trace: cannot create {}: {error}", path.display()))
            .ok()
    });
    Box::into_raw(Box::new(DebugUi {
        telemetry: Mutex::new(telemetry),
        monitor: Mutex::new(Monitor {
            trace,
            ..Monitor::default()
        }),
        capture: Capture::from_env(),
        ..DebugUi::default()
    }))
}

/// # Safety
/// `ui` must be null or come from `debug_ui_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_toggle(ui: *const DebugUi) {
    if let Some(ui) = unsafe { ui.as_ref() } {
        ui.toggle();
    }
}

/// # Safety
/// `ui` must be null or come from `debug_ui_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_toggle_overlays(ui: *const DebugUi) {
    if let Some(ui) = unsafe { ui.as_ref() } {
        ui.toggle_overlays();
    }
}

/// # Safety
/// `ui` must be null or come from `debug_ui_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_toggle_pad_capture(ui: *const DebugUi) {
    if let Some(ui) = unsafe { ui.as_ref() } {
        ui.toggle_pad_capture();
    }
}

/// # Safety
/// `ui` must be null or come from `debug_ui_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_captures_pad(ui: *const DebugUi) -> bool {
    unsafe { ui.as_ref() }.is_some_and(DebugUi::captures_pad)
}

/// # Safety
/// `ui` must be null or come from `debug_ui_create`; `event` must be null or point at a valid event.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_event(ui: *const DebugUi, event: *const RawEvent) {
    let (Some(ui), Some(event)) = (unsafe { ui.as_ref() }, unsafe { event.as_ref() }) else {
        return;
    };
    if let Some(event) = InputEvent::from_raw(event) {
        ui.push_event(event);
    }
}

/// # Safety
/// `ui` must be null or come from `debug_ui_create`. Call from the game thread, once per frame.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_run(
    ui: *const DebugUi,
    width_points: f32,
    height_points: f32,
    pixels_per_point: f32,
) {
    if let Some(ui) = unsafe { ui.as_ref() } {
        ui.run(width_points, height_points, pixels_per_point);
    }
}

/// # Safety
/// Matches `AuroraOverlayCallback`; `user` must be null or come from `debug_ui_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_paint(frame: *const OverlayFrame, user: *mut c_void) {
    let (Some(frame), Some(ui)) = (unsafe { frame.as_ref() }, unsafe {
        user.cast::<DebugUi>().as_ref()
    }) else {
        return;
    };
    ui.paint(&Target {
        device: frame.device,
        queue: frame.queue,
        pass: frame.pass,
        format: frame.format,
        width: frame.width,
        height: frame.height,
    });
}

/// # Safety
/// Matches `AuroraCaptureCallback`; `user` must be null or come from `debug_ui_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_capture_record(frame: *const CaptureFrame, user: *mut c_void) {
    let (Some(frame), Some(ui)) = (unsafe { frame.as_ref() }, unsafe {
        user.cast::<DebugUi>().as_ref()
    }) else {
        return;
    };
    unsafe { ui.capture.record(frame) };
}

/// # Safety
/// Matches `AuroraCaptureSubmittedCallback`; `user` must be null or come from `debug_ui_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_capture_submitted(user: *mut c_void) {
    if let Some(ui) = unsafe { user.cast::<DebugUi>().as_ref() } {
        ui.capture.submitted();
    }
}

/// # Safety
/// `ui` must be null or come from `debug_ui_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_capture_should_wait(ui: *const DebugUi) -> bool {
    unsafe { ui.as_ref() }.is_some_and(|ui| ui.capture.should_wait())
}

/// # Safety
/// `ui` must be null or come from `debug_ui_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_capture_draining(ui: *const DebugUi) -> bool {
    unsafe { ui.as_ref() }.is_some_and(|ui| ui.capture.draining())
}

/// # Safety
/// `ui` must be null or come from `debug_ui_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_capture_active(ui: *const DebugUi) -> bool {
    unsafe { ui.as_ref() }.is_some_and(|ui| ui.capture.active())
}

/// # Safety
/// `ui` must be null or come from `debug_ui_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_capture_started(ui: *const DebugUi) -> bool {
    unsafe { ui.as_ref() }.is_some_and(|ui| ui.capture.has_started())
}

/// # Safety
/// `ui` must be null or come from `debug_ui_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_capture_stalled(ui: *const DebugUi, waited_ms: u32) {
    if let Some(ui) = unsafe { ui.as_ref() } {
        ui.capture.stalled(waited_ms);
    }
}

/// # Safety
/// `ui` must be null or come from `debug_ui_create`. Call once, before the device is torn down.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn debug_ui_capture_finish(ui: *const DebugUi) {
    if let Some(ui) = unsafe { ui.as_ref() } {
        ui.capture.finish();
    }
}

#[cfg(test)]
mod tests {
    use egui::{Context, RawInput, pos2};

    use crate::input::{InputEvent, NavAction};
    use crate::{GameSide, Publish};

    #[test]
    fn nav_is_ignored_until_the_pad_is_captured() {
        let mut game = GameSide::default();
        game.toggle_visible();
        game.push(InputEvent::Nav(NavAction::Down), true);
        assert!(game.events.is_empty());
    }

    #[test]
    fn capturing_the_pad_opens_the_menu() {
        let mut game = GameSide::default();
        game.toggle_pad_capture();
        assert!(game.visible && game.pad_captured);
        game.toggle_pad_capture();
        assert!(game.visible && !game.pad_captured);
    }

    #[test]
    fn back_releases_the_pad_and_keeps_the_menu() {
        let mut game = GameSide::default();
        game.toggle_pad_capture();
        game.push(InputEvent::Nav(NavAction::Back), true);
        assert!(game.visible && !game.pad_captured);
        assert!(game.events.is_empty());
    }

    #[test]
    fn hiding_the_menu_releases_the_pad_and_drops_events() {
        let mut game = GameSide::default();
        game.toggle_pad_capture();
        game.push(InputEvent::Nav(NavAction::Down), true);
        assert_eq!(game.events.len(), 2);
        game.toggle_visible();
        assert!(!game.visible && !game.pad_captured);
        assert!(game.events.is_empty());
    }

    #[test]
    fn nothing_is_published_while_hidden() {
        let mut game = GameSide::default();
        assert_eq!(game.publish_action(true), Publish::Nothing);
        assert_eq!(game.publish_action(true), Publish::Nothing);
    }

    #[test]
    fn menu_or_overlays_draw_every_frame() {
        let mut game = GameSide::default();
        game.toggle_visible();
        assert_eq!(game.publish_action(true), Publish::Draw);
        assert_eq!(game.publish_action(true), Publish::Draw);

        let mut game = GameSide {
            overlays: true,
            ..GameSide::default()
        };
        assert_eq!(game.publish_action(true), Publish::Draw);
    }

    #[test]
    fn hiding_clears_the_retained_frame_exactly_once() {
        let mut game = GameSide {
            overlays: true,
            ..GameSide::default()
        };
        assert_eq!(game.publish_action(true), Publish::Draw);
        game.overlays = false;
        assert_eq!(game.publish_action(true), Publish::Clear);
        assert_eq!(game.publish_action(true), Publish::Nothing);
        game.overlays = true;
        assert_eq!(game.publish_action(true), Publish::Draw);
    }

    #[test]
    fn an_undrawable_window_clears_like_hiding() {
        let mut game = GameSide {
            overlays: true,
            ..GameSide::default()
        };
        assert_eq!(game.publish_action(true), Publish::Draw);
        assert_eq!(game.publish_action(false), Publish::Clear);
        assert_eq!(game.publish_action(false), Publish::Nothing);
        assert_eq!(game.publish_action(true), Publish::Draw);
    }

    #[test]
    fn pointer_events_need_a_visible_menu() {
        let mut game = GameSide::default();
        game.push(InputEvent::PointerMoved(pos2(1.0, 1.0)), false);
        assert!(game.events.is_empty());
        game.toggle_visible();
        game.push(InputEvent::PointerMoved(pos2(1.0, 1.0)), false);
        assert_eq!(game.events.len(), 1);
    }

    struct Clicks {
        first: u32,
        second: u32,
    }

    fn frame(ctx: &Context, game: &mut GameSide, clicks: &mut Clicks) {
        let input = RawInput {
            events: std::mem::take(&mut game.events),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            if ui.button("first").clicked() {
                clicks.first += 1;
            }
            if ui.button("second").clicked() {
                clicks.second += 1;
            }
        });
        output.textures_delta.clear();
    }

    fn nav(ctx: &Context, game: &mut GameSide, clicks: &mut Clicks, action: NavAction) {
        let has_focus = ctx.memory(|memory| memory.focused().is_some());
        game.push(InputEvent::Nav(action), has_focus);
        frame(ctx, game, clicks);
        frame(ctx, game, clicks);
    }

    #[test]
    fn pad_walks_focus_and_activates_widgets() {
        let ctx = Context::default();
        let mut game = GameSide::default();
        let mut clicks = Clicks {
            first: 0,
            second: 0,
        };
        game.toggle_pad_capture();
        frame(&ctx, &mut game, &mut clicks);

        nav(&ctx, &mut game, &mut clicks, NavAction::Down);
        assert!(ctx.memory(|memory| memory.focused().is_some()));
        nav(&ctx, &mut game, &mut clicks, NavAction::Activate);
        assert_eq!((clicks.first, clicks.second), (1, 0));

        nav(&ctx, &mut game, &mut clicks, NavAction::Down);
        nav(&ctx, &mut game, &mut clicks, NavAction::Activate);
        assert_eq!((clicks.first, clicks.second), (1, 1));

        nav(&ctx, &mut game, &mut clicks, NavAction::Up);
        nav(&ctx, &mut game, &mut clicks, NavAction::Activate);
        assert_eq!((clicks.first, clicks.second), (2, 1));
    }
}

use std::collections::VecDeque;

use egui::epaint::{CornerRadius, PathShape};
use egui::{
    Align2, Color32, Context, FontId, Id, LayerId, Order, Painter, Pos2, Rect, Shape, Stroke,
    StrokeKind, Vec2, pos2, vec2,
};

use melee_events::{Centimeters, Event, GameMode};

use crate::game::{PLAYER_SLOTS, PadButton, PadView};

const FEED_CAPACITY: usize = 7;
const FEED_LIFETIME: u64 = 300;
const FEED_FADE_IN: u64 = 8;
const FEED_FADE_OUT: u64 = 45;
const COMBO_WINDOW: u64 = 90;

const MARGIN: f32 = 16.0;
const FEED_TOP: f32 = MARGIN + 46.0;
const HRC_FEED_TOP: f32 = 225.0;
const HOME_RUN_GOLD: Color32 = Color32::from_rgb(255, 196, 64);
const PANEL_RADIUS: u8 = 10;
const PANEL_FILL: Color32 = Color32::from_rgba_premultiplied(9, 11, 16, 200);
const PANEL_EDGE: Color32 = Color32::from_rgba_premultiplied(40, 44, 54, 120);
const TEXT: Color32 = Color32::from_rgb(232, 236, 244);
const TEXT_DIM: Color32 = Color32::from_rgb(138, 146, 162);
const IDLE: Color32 = Color32::from_rgb(52, 58, 72);
const KO_RED: Color32 = Color32::from_rgb(255, 92, 92);

const PORT_COLORS: [Color32; PLAYER_SLOTS] = [
    Color32::from_rgb(241, 89, 89),
    Color32::from_rgb(101, 118, 254),
    Color32::from_rgb(254, 190, 63),
    Color32::from_rgb(76, 214, 110),
    Color32::from_rgb(170, 176, 190),
    Color32::from_rgb(170, 176, 190),
];

fn slot_index(player: u8) -> Option<usize> {
    player
        .checked_sub(1)
        .map(usize::from)
        .filter(|&slot| slot < PLAYER_SLOTS)
}

fn port_color(player: u8) -> Color32 {
    slot_index(player).map_or(TEXT_DIM, |slot| PORT_COLORS[slot])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedKind {
    Banner(&'static str),
    HomeRun {
        distance: Centimeters,
    },
    Ko {
        killer: u8,
        victim: u8,
    },
    SelfDestruct {
        player: u8,
    },
    Damage {
        player: u8,
        dealt: i32,
        total: i32,
        hits: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedEntry {
    pub born: u64,
    pub kind: FeedKind,
}

pub fn feed_alpha(age: u64) -> f32 {
    if age >= FEED_LIFETIME {
        0.0
    } else if age < FEED_FADE_IN {
        (age + 1) as f32 / FEED_FADE_IN as f32
    } else if age > FEED_LIFETIME - FEED_FADE_OUT {
        (FEED_LIFETIME - age) as f32 / FEED_FADE_OUT as f32
    } else {
        1.0
    }
}

pub fn home_run_label(distance: Centimeters) -> String {
    format!("HOME RUN  {} FT", distance.feet_as_displayed())
}

pub fn match_clock(frames: u64) -> String {
    let centis = frames % 60 * 100 / 60;
    let seconds = frames / 60;
    format!("{:02}:{:02}.{:02}", seconds / 60, seconds % 60, centis)
}

#[derive(Default)]
pub struct Hud {
    feed: VecDeque<FeedEntry>,
    mode: GameMode,
    match_started: Option<u64>,
    match_ended: Option<u64>,
    kos: [u32; PLAYER_SLOTS],
    falls: [u32; PLAYER_SLOTS],
}

impl Hud {
    pub fn ingest(&mut self, frame: u64, event: &Event) {
        let kind = match *event {
            Event::ModeChange { to, .. } => {
                self.mode = to;
                return;
            }
            Event::SceneChange { .. } | Event::StockLost { .. } => return,
            Event::MatchStart { .. } => {
                self.match_started = Some(frame);
                self.match_ended = None;
                self.kos = [0; PLAYER_SLOTS];
                self.falls = [0; PLAYER_SLOTS];
                FeedKind::Banner("MATCH START")
            }
            Event::HomeRunResult { distance } => FeedKind::HomeRun { distance },
            Event::MatchEnd => {
                self.match_ended = Some(frame);
                FeedKind::Banner("MATCH END")
            }
            Event::Ko { killer, victim } => {
                bump(&mut self.kos, killer);
                bump(&mut self.falls, victim);
                FeedKind::Ko { killer, victim }
            }
            Event::SelfDestruct { player } => {
                bump(&mut self.falls, player);
                FeedKind::SelfDestruct { player }
            }
            Event::Damage { player, from, to } => {
                if to <= from {
                    return;
                }
                let combo = self
                    .feed
                    .iter_mut()
                    .rev()
                    .find_map(|entry| match &mut entry.kind {
                        FeedKind::Damage {
                            player: hit,
                            dealt,
                            total,
                            hits,
                        } if *hit == player && frame.saturating_sub(entry.born) <= COMBO_WINDOW => {
                            Some((&mut entry.born, dealt, total, hits))
                        }
                        _ => None,
                    });
                if let Some((born, dealt, total, hits)) = combo {
                    *born = frame;
                    *dealt += to - from;
                    *total = to;
                    *hits += 1;
                    return;
                }
                FeedKind::Damage {
                    player,
                    dealt: to - from,
                    total: to,
                    hits: 1,
                }
            }
        };
        self.feed.push_back(FeedEntry { born: frame, kind });
        while self.feed.len() > FEED_CAPACITY {
            self.feed.pop_front();
        }
    }

    pub fn expire(&mut self, frame: u64) {
        self.feed
            .retain(|entry| frame.saturating_sub(entry.born) < FEED_LIFETIME);
    }

    pub fn match_frames(&self, frame: u64) -> Option<u64> {
        let started = self.match_started?;
        Some(self.match_ended.unwrap_or(frame).saturating_sub(started))
    }

    pub fn draw(&self, ctx: &Context, frame: u64, pads: &[Option<PadView>]) {
        let painter = ctx.layer_painter(LayerId::new(Order::Background, Id::new("hud")));
        let screen = ctx.content_rect();
        self.draw_match_bar(&painter, screen, frame);
        self.draw_feed(&painter, screen, frame);
        draw_pads(&painter, screen, pads);
    }

    fn draw_match_bar(&self, painter: &Painter, screen: Rect, frame: u64) {
        let scorers: Vec<usize> = (0..PLAYER_SLOTS)
            .filter(|&slot| self.kos[slot] > 0 || self.falls[slot] > 0)
            .collect();
        let mode = self.mode.name().to_uppercase().replace('_', " ");
        let clock = self
            .match_frames(frame)
            .map_or_else(|| "--:--.--".to_owned(), match_clock);
        let frame_label = format!("F{frame}");
        let measure =
            |text: &str, font: FontId| painter.layout_no_wrap(text.to_owned(), font, TEXT).size().x;
        let mode_width = measure(&mode, FontId::proportional(12.0));
        let clock_width = measure(&clock, FontId::monospace(15.0));
        let frame_width = measure(&frame_label, FontId::monospace(10.0));
        let width = 30.0
            + mode_width
            + 16.0
            + clock_width
            + 10.0
            + frame_width
            + 14.0
            + scorers.len() as f32 * 52.0;
        let rect = Rect::from_min_size(
            pos2(
                (screen.center().x - width / 2.0).round(),
                screen.top() + MARGIN,
            ),
            vec2(width, 34.0),
        );
        panel(painter, rect, 1.0);

        let mid = rect.center().y;
        let live = self.match_started.is_some() && self.match_ended.is_none();
        painter.circle_filled(
            pos2(rect.left() + 16.0, mid),
            4.0,
            if live { KO_RED } else { IDLE },
        );
        let mut x = rect.left() + 30.0;
        painter.text(
            pos2(x, mid),
            Align2::LEFT_CENTER,
            mode,
            FontId::proportional(12.0),
            TEXT_DIM,
        );
        x += mode_width + 16.0;
        painter.text(
            pos2(x, mid),
            Align2::LEFT_CENTER,
            clock,
            FontId::monospace(15.0),
            TEXT,
        );
        x += clock_width + 10.0;
        painter.text(
            pos2(x, mid + 1.5),
            Align2::LEFT_CENTER,
            frame_label,
            FontId::monospace(10.0),
            TEXT_DIM,
        );
        x += frame_width + 14.0;
        for &slot in &scorers {
            chip(
                painter,
                pos2(x, mid),
                &format!("P{}", slot + 1),
                PORT_COLORS[slot],
                1.0,
            );
            painter.text(
                pos2(x + 29.0, mid),
                Align2::LEFT_CENTER,
                self.kos[slot].to_string(),
                FontId::monospace(14.0),
                TEXT,
            );
            x += 52.0;
        }
    }

    fn feed_top(&self) -> f32 {
        if self.mode == GameMode::HomeRunContest {
            HRC_FEED_TOP
        } else {
            FEED_TOP
        }
    }

    fn draw_feed(&self, painter: &Painter, screen: Rect, frame: u64) {
        let width = 236.0;
        let row = 30.0;
        let mut y = screen.top() + self.feed_top();
        for entry in &self.feed {
            let age = frame.saturating_sub(entry.born);
            let alpha = feed_alpha(age);
            if alpha <= 0.0 {
                continue;
            }
            let slide = (1.0 - (age as f32 / FEED_FADE_IN as f32).min(1.0)) * 24.0;
            let rect = Rect::from_min_size(
                pos2(screen.right() - MARGIN - width + slide, y),
                vec2(width, row - 4.0),
            );
            panel(painter, rect, alpha);
            let mid = rect.center().y;
            let left = rect.left() + 10.0;
            match entry.kind {
                FeedKind::Banner(text) => {
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        text,
                        FontId::proportional(12.0),
                        fade(TEXT, alpha),
                    );
                }
                FeedKind::HomeRun { distance } => {
                    accent(painter, rect, HOME_RUN_GOLD, alpha);
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        home_run_label(distance),
                        FontId::proportional(13.0),
                        fade(HOME_RUN_GOLD, alpha),
                    );
                }
                FeedKind::Ko { killer, victim } => {
                    accent(painter, rect, KO_RED, alpha);
                    chip(
                        painter,
                        pos2(left + 4.0, mid),
                        &format!("P{killer}"),
                        port_color(killer),
                        alpha,
                    );
                    arrow(painter, pos2(left + 44.0, mid), fade(KO_RED, alpha));
                    painter.text(
                        pos2(left + 62.0, mid),
                        Align2::LEFT_CENTER,
                        "KO",
                        FontId::proportional(13.0),
                        fade(KO_RED, alpha),
                    );
                    arrow(painter, pos2(left + 92.0, mid), fade(KO_RED, alpha));
                    chip(
                        painter,
                        pos2(left + 108.0, mid),
                        &format!("P{victim}"),
                        port_color(victim),
                        alpha,
                    );
                }
                FeedKind::SelfDestruct { player } => {
                    accent(painter, rect, KO_RED, alpha);
                    chip(
                        painter,
                        pos2(left + 4.0, mid),
                        &format!("P{player}"),
                        port_color(player),
                        alpha,
                    );
                    painter.text(
                        pos2(left + 40.0, mid),
                        Align2::LEFT_CENTER,
                        "SELF DESTRUCT",
                        FontId::proportional(12.0),
                        fade(KO_RED, alpha),
                    );
                }
                FeedKind::Damage {
                    player,
                    dealt,
                    total,
                    hits,
                } => {
                    accent(painter, rect, port_color(player), alpha);
                    chip(
                        painter,
                        pos2(left + 4.0, mid),
                        &format!("P{player}"),
                        port_color(player),
                        alpha,
                    );
                    painter.text(
                        pos2(left + 40.0, mid),
                        Align2::LEFT_CENTER,
                        if hits > 1 {
                            format!("+{dealt}%  x{hits}")
                        } else {
                            format!("+{dealt}%")
                        },
                        FontId::monospace(13.0),
                        fade(TEXT, alpha),
                    );
                    painter.text(
                        pos2(rect.right() - 10.0, mid),
                        Align2::RIGHT_CENTER,
                        format!("{total}%"),
                        FontId::monospace(13.0),
                        fade(damage_color(total), alpha),
                    );
                }
            }
            y += row;
        }
    }
}

fn bump(counts: &mut [u32; PLAYER_SLOTS], player: u8) {
    if let Some(slot) = slot_index(player) {
        counts[slot] += 1;
    }
}

pub fn damage_color(percent: i32) -> Color32 {
    let t = (percent.clamp(0, 150) as f32) / 150.0;
    let lerp = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgb(lerp(232, 255), lerp(236, 70), lerp(244, 60))
}

fn fade(color: Color32, alpha: f32) -> Color32 {
    color.gamma_multiply(alpha.clamp(0.0, 1.0))
}

fn panel(painter: &Painter, rect: Rect, alpha: f32) {
    let radius = CornerRadius::same(PANEL_RADIUS);
    painter.rect_filled(rect, radius, fade(PANEL_FILL, alpha));
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, fade(PANEL_EDGE, alpha)),
        StrokeKind::Inside,
    );
}

fn accent(painter: &Painter, rect: Rect, color: Color32, alpha: f32) {
    let bar = Rect::from_min_size(
        rect.left_top() + vec2(0.0, 6.0),
        vec2(3.0, rect.height() - 12.0),
    );
    painter.rect_filled(bar, CornerRadius::same(2), fade(color, alpha));
}

fn chip(painter: &Painter, left_center: Pos2, label: &str, color: Color32, alpha: f32) {
    let rect = Rect::from_min_size(left_center - vec2(0.0, 8.0), vec2(24.0, 16.0));
    painter.rect_filled(rect, CornerRadius::same(5), fade(color, alpha));
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(10.5),
        fade(Color32::from_rgb(12, 14, 20), alpha),
    );
}

fn arrow(painter: &Painter, center: Pos2, color: Color32) {
    let points = vec![
        center + vec2(-4.0, -5.0),
        center + vec2(5.0, 0.0),
        center + vec2(-4.0, 5.0),
    ];
    painter.add(Shape::Path(PathShape::convex_polygon(
        points,
        color,
        Stroke::NONE,
    )));
}

fn draw_pads(painter: &Painter, screen: Rect, pads: &[Option<PadView>]) {
    let size = vec2(214.0, 96.0);
    let mut x = screen.left() + MARGIN;
    for (port, pad) in pads.iter().enumerate() {
        let Some(pad) = pad else {
            continue;
        };
        let rect = Rect::from_min_size(pos2(x, screen.bottom() - MARGIN - size.y), size);
        draw_pad(painter, rect, port, pad);
        x += size.x + 10.0;
    }
}

fn draw_pad(painter: &Painter, rect: Rect, port: usize, pad: &PadView) {
    const GREEN: Color32 = Color32::from_rgb(47, 191, 113);
    const RED: Color32 = Color32::from_rgb(229, 72, 77);
    const GREY: Color32 = Color32::from_rgb(201, 205, 212);
    const PURPLE: Color32 = Color32::from_rgb(124, 108, 242);
    const YELLOW: Color32 = Color32::from_rgb(242, 201, 76);

    let origin = rect.left_top();
    let color = PORT_COLORS.get(port).copied().unwrap_or(TEXT_DIM);
    panel(painter, rect, 1.0);
    accent(painter, rect, color, 1.0);
    painter.text(
        origin + vec2(12.0, 8.0),
        Align2::LEFT_TOP,
        format!("P{}", port + 1),
        FontId::proportional(11.0),
        color,
    );

    trigger(
        painter,
        origin + vec2(40.0, 11.0),
        "L",
        pad.trigger_l,
        pad.pressed(PadButton::L),
    );
    trigger(
        painter,
        origin + vec2(108.0, 11.0),
        "R",
        pad.trigger_r,
        pad.pressed(PadButton::R),
    );
    let z = Rect::from_min_size(origin + vec2(170.0, 8.0), vec2(32.0, 13.0));
    let z_pressed = pad.pressed(PadButton::Z);
    painter.rect_filled(z, CornerRadius::same(6), lit(PURPLE, z_pressed));
    label(painter, z.center(), "Z", 8.5, z_pressed);

    stick(painter, origin + vec2(40.0, 60.0), 24.0, pad.stick, TEXT);
    dpad(painter, origin + vec2(88.0, 76.0), pad);
    stick(
        painter,
        origin + vec2(128.0, 68.0),
        15.0,
        pad.cstick,
        YELLOW,
    );
    painter.circle_filled(
        origin + vec2(88.0, 46.0),
        4.0,
        lit(GREY, pad.pressed(PadButton::Start)),
    );

    let a = origin + vec2(176.0, 62.0);
    face_button(
        painter,
        a,
        11.5,
        "A",
        10.0,
        GREEN,
        pad.pressed(PadButton::A),
    );
    face_button(
        painter,
        a + vec2(-18.0, 12.0),
        7.5,
        "B",
        8.0,
        RED,
        pad.pressed(PadButton::B),
    );
    face_button(
        painter,
        a + vec2(20.0, -5.0),
        7.5,
        "X",
        8.0,
        GREY,
        pad.pressed(PadButton::X),
    );
    face_button(
        painter,
        a + vec2(-6.0, -20.0),
        7.5,
        "Y",
        8.0,
        GREY,
        pad.pressed(PadButton::Y),
    );
}

fn face_button(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    name: &str,
    size: f32,
    color: Color32,
    pressed: bool,
) {
    painter.circle_filled(center, radius, lit(color, pressed));
    if !pressed {
        painter.circle_stroke(center, radius, Stroke::new(1.0, color.gamma_multiply(0.45)));
    }
    label(painter, center, name, size, pressed);
}

fn label(painter: &Painter, center: Pos2, text: &str, size: f32, pressed: bool) {
    let color = if pressed {
        Color32::from_rgb(12, 14, 20)
    } else {
        TEXT_DIM
    };
    painter.text(
        center,
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(size),
        color,
    );
}

fn lit(color: Color32, pressed: bool) -> Color32 {
    if pressed {
        color
    } else {
        color.gamma_multiply(0.22)
    }
}

fn trigger(painter: &Painter, min: Pos2, name: &str, amount: f32, pressed: bool) {
    painter.text(
        min + vec2(0.0, 3.5),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(8.5),
        TEXT_DIM,
    );
    let track = Rect::from_min_size(min + vec2(10.0, 0.0), vec2(44.0, 7.0));
    painter.rect_filled(track, CornerRadius::same(3), IDLE);
    let amount = if pressed { 1.0 } else { amount.clamp(0.0, 1.0) };
    if amount > 0.01 {
        let fill = Rect::from_min_size(track.min, vec2(track.width() * amount, track.height()));
        let color = if pressed { TEXT } else { TEXT_DIM };
        painter.rect_filled(fill, CornerRadius::same(3), color);
    }
}

fn stick(painter: &Painter, center: Pos2, radius: f32, axis: Vec2, color: Color32) {
    let gate: Vec<Pos2> = (0..8)
        .map(|i| {
            let angle = std::f32::consts::FRAC_PI_4 * i as f32 + std::f32::consts::FRAC_PI_8;
            center + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    painter.add(Shape::Path(PathShape::convex_polygon(
        gate,
        Color32::from_rgba_premultiplied(20, 23, 31, 160),
        Stroke::new(1.5, IDLE),
    )));
    let offset = vec2(axis.x.clamp(-1.0, 1.0), -axis.y.clamp(-1.0, 1.0)) * (radius - 5.0);
    painter.line_segment(
        [center, center + offset],
        Stroke::new(2.0, color.gamma_multiply(0.45)),
    );
    painter.circle_filled(center + offset, radius * 0.3, color);
}

fn dpad(painter: &Painter, center: Pos2, pad: &PadView) {
    let arm = |dx: f32, dy: f32, button: PadButton| {
        let rect = Rect::from_center_size(center + vec2(dx, dy) * 7.0, vec2(7.0, 7.0));
        painter.rect_filled(rect, CornerRadius::same(2), lit(TEXT, pad.pressed(button)));
    };
    arm(0.0, -1.0, PadButton::Up);
    arm(0.0, 1.0, PadButton::Down);
    arm(-1.0, 0.0, PadButton::Left);
    arm(1.0, 0.0, PadButton::Right);
    painter.rect_filled(
        Rect::from_center_size(center, vec2(7.0, 7.0)),
        CornerRadius::ZERO,
        IDLE,
    );
}

#[cfg(test)]
mod tests {
    use melee_events::{Centimeters, Event, GameMode};

    use crate::overlay::{
        FEED_CAPACITY, FEED_LIFETIME, FEED_TOP, FeedKind, HRC_FEED_TOP, Hud, PORT_COLORS, TEXT_DIM,
        damage_color, feed_alpha, home_run_label, match_clock, port_color, slot_index,
    };

    #[test]
    fn alpha_fades_in_holds_and_fades_out() {
        assert!(feed_alpha(0) > 0.0 && feed_alpha(0) < 0.2);
        assert_eq!(feed_alpha(7), 1.0);
        assert_eq!(feed_alpha(100), 1.0);
        let late = feed_alpha(FEED_LIFETIME - 10);
        assert!(late > 0.0 && late < 1.0);
        assert_eq!(feed_alpha(FEED_LIFETIME), 0.0);
        assert_eq!(feed_alpha(u64::MAX), 0.0);
    }

    #[test]
    fn alpha_never_increases_during_fade_out() {
        let mut previous = 1.0;
        for age in FEED_LIFETIME - 60..=FEED_LIFETIME {
            let alpha = feed_alpha(age);
            assert!(alpha <= previous, "age {age}");
            previous = alpha;
        }
    }

    #[test]
    fn clock_formats_frames_at_60hz() {
        assert_eq!(match_clock(0), "00:00.00");
        assert_eq!(match_clock(30), "00:00.50");
        assert_eq!(match_clock(59), "00:00.98");
        assert_eq!(match_clock(60), "00:01.00");
        assert_eq!(match_clock(60 * 61 + 15), "01:01.25");
        assert_eq!(match_clock(60 * 60 * 10), "10:00.00");
    }

    #[test]
    fn ko_updates_feed_and_tallies() {
        let mut hud = Hud::default();
        hud.ingest(
            10,
            &Event::Ko {
                killer: 2,
                victim: 1,
            },
        );
        hud.ingest(
            20,
            &Event::Ko {
                killer: 2,
                victim: 1,
            },
        );
        hud.ingest(30, &Event::SelfDestruct { player: 2 });
        assert_eq!(hud.kos, [0, 2, 0, 0, 0, 0]);
        assert_eq!(hud.falls, [2, 1, 0, 0, 0, 0]);
        let kinds: Vec<&FeedKind> = hud.feed.iter().map(|entry| &entry.kind).collect();
        assert_eq!(
            kinds,
            [
                &FeedKind::Ko {
                    killer: 2,
                    victim: 1
                },
                &FeedKind::Ko {
                    killer: 2,
                    victim: 1
                },
                &FeedKind::SelfDestruct { player: 2 },
            ]
        );
    }

    #[test]
    fn out_of_range_players_do_not_panic_or_count() {
        let mut hud = Hud::default();
        hud.ingest(
            1,
            &Event::Ko {
                killer: 0,
                victim: 9,
            },
        );
        assert_eq!(hud.kos, [0; 6]);
        assert_eq!(hud.falls, [0; 6]);
    }

    #[test]
    fn player_numbers_are_one_based() {
        assert_eq!(slot_index(0), None);
        assert_eq!(slot_index(1), Some(0));
        assert_eq!(slot_index(6), Some(5));
        assert_eq!(slot_index(7), None);
        assert_eq!(slot_index(u8::MAX), None);
        assert_eq!(port_color(1), PORT_COLORS[0]);
        assert_eq!(port_color(4), PORT_COLORS[3]);
        assert_eq!(port_color(0), TEXT_DIM);
        assert_eq!(port_color(9), TEXT_DIM);
    }

    #[test]
    fn rapid_hits_on_one_player_merge_into_one_row() {
        let mut hud = Hud::default();
        hud.ingest(
            100,
            &Event::Damage {
                player: 1,
                from: 0,
                to: 4,
            },
        );
        hud.ingest(
            108,
            &Event::Damage {
                player: 1,
                from: 4,
                to: 9,
            },
        );
        hud.ingest(
            120,
            &Event::Damage {
                player: 1,
                from: 9,
                to: 21,
            },
        );
        assert_eq!(hud.feed.len(), 1);
        assert_eq!(
            hud.feed[0].kind,
            FeedKind::Damage {
                player: 1,
                dealt: 21,
                total: 21,
                hits: 3,
            }
        );
        assert_eq!(hud.feed[0].born, 120);
    }

    #[test]
    fn hits_on_another_player_or_after_the_window_start_a_new_row() {
        let mut hud = Hud::default();
        hud.ingest(
            100,
            &Event::Damage {
                player: 1,
                from: 0,
                to: 4,
            },
        );
        hud.ingest(
            105,
            &Event::Damage {
                player: 2,
                from: 0,
                to: 6,
            },
        );
        hud.ingest(
            200,
            &Event::Damage {
                player: 2,
                from: 6,
                to: 8,
            },
        );
        assert_eq!(hud.feed.len(), 3);
    }

    #[test]
    fn a_combo_keeps_merging_across_another_players_row() {
        let mut hud = Hud::default();
        hud.ingest(
            100,
            &Event::Damage {
                player: 1,
                from: 0,
                to: 5,
            },
        );
        hud.ingest(
            110,
            &Event::Damage {
                player: 2,
                from: 0,
                to: 3,
            },
        );
        hud.ingest(
            150,
            &Event::Damage {
                player: 1,
                from: 5,
                to: 12,
            },
        );
        assert_eq!(hud.feed.len(), 2);
        assert_eq!(
            hud.feed[0].kind,
            FeedKind::Damage {
                player: 1,
                dealt: 12,
                total: 12,
                hits: 2
            }
        );
        assert_eq!(hud.feed[0].born, 150);
        assert_eq!(
            hud.feed[1].kind,
            FeedKind::Damage {
                player: 2,
                dealt: 3,
                total: 3,
                hits: 1
            }
        );
    }

    #[test]
    fn a_hit_after_the_combo_window_starts_a_new_row() {
        let mut hud = Hud::default();
        hud.ingest(
            100,
            &Event::Damage {
                player: 1,
                from: 0,
                to: 5,
            },
        );
        hud.ingest(
            190,
            &Event::Damage {
                player: 1,
                from: 5,
                to: 9,
            },
        );
        hud.ingest(
            281,
            &Event::Damage {
                player: 1,
                from: 9,
                to: 20,
            },
        );
        assert_eq!(hud.feed.len(), 2);
        assert_eq!(
            hud.feed[0].kind,
            FeedKind::Damage {
                player: 1,
                dealt: 9,
                total: 9,
                hits: 2
            }
        );
        assert_eq!(
            hud.feed[1].kind,
            FeedKind::Damage {
                player: 1,
                dealt: 11,
                total: 20,
                hits: 1
            }
        );
    }

    #[test]
    fn healing_and_respawn_resets_are_not_shown() {
        let mut hud = Hud::default();
        hud.ingest(
            5,
            &Event::Damage {
                player: 1,
                from: 80,
                to: 0,
            },
        );
        hud.ingest(
            6,
            &Event::Damage {
                player: 1,
                from: 30,
                to: 30,
            },
        );
        assert!(hud.feed.is_empty());
    }

    #[test]
    fn feed_is_capped_and_drops_the_oldest() {
        let mut hud = Hud::default();
        for player in 0..(FEED_CAPACITY as u8 + 3) {
            hud.ingest(
                u64::from(player),
                &Event::SelfDestruct {
                    player: player % 4 + 1,
                },
            );
        }
        assert_eq!(hud.feed.len(), FEED_CAPACITY);
        assert_eq!(hud.feed[0].born, 3);
    }

    #[test]
    fn expire_removes_entries_past_their_lifetime() {
        let mut hud = Hud::default();
        hud.ingest(0, &Event::MatchEnd);
        hud.ingest(200, &Event::MatchEnd);
        hud.expire(FEED_LIFETIME - 1);
        assert_eq!(hud.feed.len(), 2);
        hud.expire(FEED_LIFETIME);
        assert_eq!(hud.feed.len(), 1);
        hud.expire(200 + FEED_LIFETIME);
        assert!(hud.feed.is_empty());
    }

    #[test]
    fn match_clock_runs_from_start_and_freezes_at_end() {
        let mut hud = Hud::default();
        assert_eq!(hud.match_frames(500), None);
        hud.ingest(
            10,
            &Event::Ko {
                killer: 1,
                victim: 2,
            },
        );
        hud.ingest(
            808,
            &Event::MatchStart {
                players: Vec::new(),
            },
        );
        assert_eq!(hud.kos, [0; 6], "a new match resets the tally");
        assert_eq!(hud.match_frames(808), Some(0));
        assert_eq!(hud.match_frames(1408), Some(600));
        hud.ingest(7795, &Event::MatchEnd);
        assert_eq!(hud.match_frames(9000), Some(6987));
    }

    #[test]
    fn a_home_run_gets_a_feed_row_with_the_distance_in_feet() {
        let mut hud = Hud::default();
        hud.ingest(
            742,
            &Event::HomeRunResult {
                distance: Centimeters(4720),
            },
        );
        assert_eq!(hud.feed.len(), 1);
        assert_eq!(
            hud.feed[0].kind,
            FeedKind::HomeRun {
                distance: Centimeters(4720)
            }
        );
        assert_eq!(hud.feed[0].born, 742);
        assert_eq!(home_run_label(Centimeters(4720)), "HOME RUN  154.8 FT");
    }

    #[test]
    fn the_feed_sits_below_the_distance_meter_in_home_run_contest() {
        let mut hud = Hud::default();
        assert_eq!(hud.feed_top(), FEED_TOP);
        hud.ingest(
            3,
            &Event::ModeChange {
                from: GameMode::Title,
                to: GameMode::HomeRunContest,
            },
        );
        assert_eq!(hud.feed_top(), HRC_FEED_TOP);
        hud.ingest(
            742,
            &Event::ModeChange {
                from: GameMode::HomeRunContest,
                to: GameMode::Menu,
            },
        );
        assert_eq!(hud.feed_top(), FEED_TOP);
    }

    #[test]
    fn mode_is_tracked_without_a_feed_row() {
        let mut hud = Hud::default();
        hud.ingest(
            3,
            &Event::ModeChange {
                from: GameMode::Title,
                to: GameMode::DebugVs,
            },
        );
        hud.ingest(3, &Event::SceneChange { from: 0, to: 1 });
        hud.ingest(
            3,
            &Event::StockLost {
                player: 1,
                stocks: 3,
            },
        );
        assert_eq!(hud.mode, GameMode::DebugVs);
        assert!(hud.feed.is_empty());
    }

    #[test]
    fn damage_color_runs_from_white_to_red() {
        assert_eq!(damage_color(0), egui::Color32::from_rgb(232, 236, 244));
        assert_eq!(damage_color(150), egui::Color32::from_rgb(255, 70, 60));
        assert_eq!(damage_color(999), damage_color(150));
        assert_eq!(damage_color(-5), damage_color(0));
    }
}

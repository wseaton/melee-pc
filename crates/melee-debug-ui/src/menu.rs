use egui::{Context, Grid, Slider, Window};

use crate::game::{self, DEFAULT_SIM_HZ, Player, Slot};

const MIN_SIM_HZ: u32 = 5;
const MAX_SIM_HZ: u32 = 240;

pub fn show(ctx: &Context, frame_count: u64) {
    Window::new("Melee debug (Rust + egui)")
        .default_pos([16.0, 16.0])
        .show(ctx, |ui| {
            ui.label(format!(
                "frame {frame_count}   scene {}",
                game::scene_index()
            ));

            ui.horizontal(|ui| {
                let mut hz = game::sim_hz();
                let slider = Slider::new(&mut hz, MIN_SIM_HZ..=MAX_SIM_HZ).text("sim Hz");
                if ui.add(slider).changed() {
                    game::set_sim_hz(hz);
                }
                if ui.button("reset").clicked() {
                    game::set_sim_hz(DEFAULT_SIM_HZ);
                }
            });

            ui.separator();
            let players: Vec<Player> = Slot::all().filter_map(game::player).collect();
            if players.is_empty() {
                ui.label("no players loaded");
                return;
            }
            Grid::new("players").striped(true).show(ui, |ui| {
                for heading in ["slot", "kind", "character", "damage", "x", "y", "stocks"] {
                    ui.strong(heading);
                }
                ui.end_row();
                for player in &players {
                    player_row(ui, player);
                    ui.end_row();
                }
            });
        });
}

fn player_row(ui: &mut egui::Ui, player: &Player) {
    ui.label(format!("P{}", player.slot.number()));
    ui.label(player.kind.name());
    ui.label(player.character.name());
    ui.label(format!("{}%", player.damage));
    ui.label(format!("{:.1}", player.position.x));
    ui.label(format!("{:.1}", player.position.y));
    ui.horizontal(|ui| {
        if ui.small_button("-").clicked() {
            game::set_stocks(player.slot, (player.stocks - 1).max(0));
        }
        ui.label(player.stocks.to_string());
        if ui.small_button("+").clicked() {
            game::set_stocks(player.slot, (player.stocks + 1).min(99));
        }
    });
}

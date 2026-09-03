//! Farben und Maße.
//!
//! Dunkel, weil man in einem Club sitzt und nicht in einem Büro. Die Deckfarben
//! unterscheiden sich deutlich voneinander — im Halbdunkel muss auf einen Blick
//! klar sein, welche Hälfte gerade läuft.

use egui::Color32;

pub const BG: Color32 = Color32::from_rgb(16, 17, 21);
pub const PANEL: Color32 = Color32::from_rgb(24, 26, 32);
pub const PANEL_HELL: Color32 = Color32::from_rgb(34, 37, 45);
pub const RAHMEN: Color32 = Color32::from_rgb(52, 56, 68);

pub const TEXT: Color32 = Color32::from_rgb(226, 229, 236);
pub const TEXT_LEISE: Color32 = Color32::from_rgb(138, 145, 160);

pub const DECK_A: Color32 = Color32::from_rgb(94, 190, 255);
pub const DECK_B: Color32 = Color32::from_rgb(255, 158, 84);
pub const AUX: Color32 = Color32::from_rgb(150, 230, 160);

pub const GESPIELT: Color32 = Color32::from_rgb(78, 84, 98);
pub const BEAT: Color32 = Color32::from_rgb(70, 76, 92);
pub const TAKT: Color32 = Color32::from_rgb(120, 128, 150);
pub const CUE: Color32 = Color32::from_rgb(255, 214, 92);
pub const PLAYHEAD: Color32 = Color32::from_rgb(255, 96, 96);

pub const AKTIV: Color32 = Color32::from_rgb(96, 220, 140);
pub const WARNUNG: Color32 = Color32::from_rgb(255, 128, 96);

/// Farbe eines Decks nach Index.
pub fn deck_farbe(index: usize) -> Color32 {
    match index {
        0 => DECK_A,
        1 => DECK_B,
        _ => AUX,
    }
}

pub fn stil(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = Color32::from_rgb(12, 13, 16);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.inactive.bg_fill = PANEL_HELL;
    visuals.widgets.hovered.bg_fill = RAHMEN;
    visuals.override_text_color = Some(TEXT);
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 5.0);
        style.spacing.slider_width = 130.0;
    });
}

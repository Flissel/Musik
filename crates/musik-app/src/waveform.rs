//! Wellenform-Darstellung.
//!
//! Zwei Ansichten, wie bei jedem DJ-Programm: eine Übersicht über den ganzen
//! Track und ein Ausschnitt um die Abspielposition. Die Übersicht dient dem
//! Springen, der Ausschnitt dem Mixen.
//!
//! Im Zeichenpfad wird **nicht gerechnet**. Die Spitzen liegen aus der Analyse
//! vorberechnet in mehreren Auflösungen vor; hier wird nur noch ausgewählt und
//! gemalt. Über die Rohdaten zu laufen hieße dreizehn Millionen Werte pro Deck
//! und Bild — bei 60 Bildern pro Sekunde aussichtslos.

use analysis::peaks::PeakLevel;
use audio_core::Beatgrid;
use egui::{Color32, Painter, Pos2, Rect, Stroke};

use crate::theme;

/// Wählt die Auflösung, die zur Zoomstufe passt.
///
/// Zu fein heißt: hunderte Spitzen je Bildspalte zusammenfassen, also Arbeit
/// ohne Nutzen. Zu grob heißt: Treppenstufen.
pub fn passende_stufe(levels: &[PeakLevel], sichtbare_frames: f64, breite_px: f32) -> &PeakLevel {
    let gewuenscht = (sichtbare_frames / breite_px.max(1.0) as f64).max(1.0);

    levels
        .iter()
        .filter(|l| !l.is_empty())
        .min_by_key(|l| {
            let verhaeltnis = l.samples_per_peak as f64 / gewuenscht;
            // Logarithmischer Abstand, damit „doppelt so fein" und „halb so
            // fein" gleich schlecht zählen.
            (verhaeltnis.log2().abs() * 1_000.0) as i64
        })
        .unwrap_or(&levels[0])
}

/// Welche Spitzen zu einer Bildspalte gehören.
///
/// `None`, wenn die Spalte ganz vor dem Anfang oder hinter dem Ende des Tracks
/// liegt. Das ist der Normalfall in der Zoom-Ansicht: sie ist um die
/// Abspielposition zentriert, also ragt sie in den ersten Sekunden links über
/// den Track hinaus. Ohne die Prüfung wird dort die erste Spitze für jede
/// Spalte wiederholt — ein Balken, den es nicht gibt.
fn spitzen_bereich(
    von_frame: f64,
    bis_frame: f64,
    samples_je_spitze: u32,
    anzahl: usize,
) -> Option<(usize, usize)> {
    if bis_frame <= 0.0 || anzahl == 0 || samples_je_spitze == 0 {
        return None;
    }

    let je = samples_je_spitze as f64;
    let i0 = (von_frame / je).floor().max(0.0) as usize;
    if i0 >= anzahl {
        return None;
    }

    let i1 = ((bis_frame / je).ceil().max(0.0) as usize).clamp(i0 + 1, anzahl);
    Some((i0, i1))
}

/// Zeichnet einen Frame-Bereich als Wellenform.
#[allow(clippy::too_many_arguments)]
pub fn zeichne(
    painter: &Painter,
    rect: Rect,
    level: &PeakLevel,
    von_frame: f64,
    bis_frame: f64,
    farbe: Color32,
    gespielt_bis: Option<f64>,
) {
    painter.rect_filled(rect, 2.0, Color32::from_rgb(12, 13, 16));

    if level.is_empty() || bis_frame <= von_frame {
        return;
    }

    let mitte = rect.center().y;
    let halbe_hoehe = rect.height() * 0.5 - 1.0;
    let spalten = rect.width().max(1.0) as usize;
    let frames_je_spalte = (bis_frame - von_frame) / spalten as f64;

    for spalte in 0..spalten {
        let x = rect.left() + spalte as f32 + 0.5;
        let f0 = von_frame + spalte as f64 * frames_je_spalte;
        let f1 = f0 + frames_je_spalte;

        let Some((i0, i1)) = spitzen_bereich(f0, f1, level.samples_per_peak, level.len()) else {
            continue;
        };

        let mut lo = i8::MAX;
        let mut hi = i8::MIN;
        for i in i0..i1 {
            lo = lo.min(level.min[i]);
            hi = hi.max(level.max[i]);
        }
        if lo > hi {
            continue;
        }

        let y0 = mitte - hi as f32 / 127.0 * halbe_hoehe;
        let y1 = mitte - lo as f32 / 127.0 * halbe_hoehe;

        let bereits_gespielt = gespielt_bis.is_some_and(|p| f1 <= p);
        let farbe = if bereits_gespielt {
            theme::GESPIELT
        } else {
            farbe
        };

        painter.line_segment(
            [Pos2::new(x, y0), Pos2::new(x, y1)],
            Stroke::new(1.0, farbe),
        );
    }
}

/// Legt Beat- und Taktlinien über einen Bereich.
///
/// Takte kräftiger als Beats: Beim Mixen zählt man Vierertakte, nicht einzelne
/// Schläge.
#[allow(clippy::too_many_arguments)]
pub fn zeichne_grid(
    painter: &Painter,
    rect: Rect,
    grid: &Beatgrid,
    sample_rate: u32,
    von_frame: f64,
    bis_frame: f64,
    laenge_frames: f64,
) {
    let frames_je_beat = grid.frames_per_beat(sample_rate);
    if frames_je_beat <= 0.0 || bis_frame <= von_frame {
        return;
    }

    let sichtbar = bis_frame - von_frame;
    // Bei zu vielen Linien wird das Bild zum Zaun — dann nur noch Takte.
    let nur_takte = sichtbar / frames_je_beat > 64.0;

    let erster = grid.beat_at(von_frame, sample_rate).floor() as i64;
    let letzter = grid.beat_at(bis_frame, sample_rate).ceil() as i64;

    for beat in erster..=letzter {
        let taktanfang = beat.rem_euclid(4) == 0;
        if nur_takte && !taktanfang {
            continue;
        }

        let frame = grid.frame_of_beat(beat as f64, sample_rate);
        // Vor dem ersten und hinter dem letzten Sample gibt es keine Beats.
        // Die Zoom-Ansicht ist um die Abspielposition zentriert und ragt am
        // Anfang eines Tracks über ihn hinaus — dort ein Raster zu zeigen,
        // behauptete Musik, die noch gar nicht anfängt.
        if frame < 0.0 || frame > laenge_frames {
            continue;
        }

        let t = (frame - von_frame) / sichtbar;
        if !(0.0..=1.0).contains(&t) {
            continue;
        }

        let x = rect.left() + t as f32 * rect.width();
        let (farbe, breite) = if taktanfang {
            (theme::TAKT, 1.4)
        } else {
            (theme::BEAT, 0.8)
        };

        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(breite, farbe),
        );
    }
}

/// Markiert Cue-Punkte.
pub fn zeichne_cues(
    painter: &Painter,
    rect: Rect,
    cues: &[(usize, u64)],
    von_frame: f64,
    bis_frame: f64,
) {
    if bis_frame <= von_frame {
        return;
    }

    for (index, frame) in cues {
        let t = (*frame as f64 - von_frame) / (bis_frame - von_frame);
        if !(0.0..=1.0).contains(&t) {
            continue;
        }

        let x = rect.left() + t as f32 * rect.width();
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.5, theme::CUE),
        );
        painter.text(
            Pos2::new(x + 2.0, rect.top() + 1.0),
            egui::Align2::LEFT_TOP,
            format!("{}", index + 1),
            egui::FontId::monospace(9.0),
            theme::CUE,
        );
    }
}

pub fn zeichne_playhead(painter: &Painter, rect: Rect, x: f32) {
    painter.line_segment(
        [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
        Stroke::new(1.6, theme::PLAYHEAD),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stufe(samples_per_peak: u32, len: usize) -> PeakLevel {
        PeakLevel {
            samples_per_peak,
            min: vec![-100; len],
            max: vec![100; len],
        }
    }

    #[test]
    fn die_stufe_passt_sich_dem_zoom_an() {
        let levels = vec![stufe(256, 10_000), stufe(2_048, 1_250), stufe(16_384, 160)];

        // Ganzer Track auf 800 px: grob genügt.
        let weit = passende_stufe(&levels, 10_000_000.0, 800.0);
        assert_eq!(weit.samples_per_peak, 16_384);

        // Wenige Sekunden auf 800 px: fein.
        let nah = passende_stufe(&levels, 200_000.0, 800.0);
        assert_eq!(nah.samples_per_peak, 256);
    }

    #[test]
    fn vor_dem_anfang_und_hinter_dem_ende_wird_nichts_gezeichnet() {
        // Zoom-Ansicht in der ersten Sekunde: die linke Hälfte liegt vor dem
        // Track. Dort darf keine Spitze wiederholt werden.
        assert_eq!(spitzen_bereich(-8_000.0, -4_000.0, 256, 100), None);
        assert_eq!(spitzen_bereich(30_000.0, 34_000.0, 256, 100), None);

        // Der Übergang gehört noch dazu, aber nur mit dem vorhandenen Teil.
        assert_eq!(spitzen_bereich(-500.0, 500.0, 256, 100), Some((0, 2)));
        assert_eq!(
            spitzen_bereich(25_000.0, 26_000.0, 256, 100),
            Some((97, 100))
        );
    }

    #[test]
    fn leere_stufen_werden_uebersprungen() {
        let levels = vec![
            PeakLevel {
                samples_per_peak: 256,
                min: vec![],
                max: vec![],
            },
            stufe(2_048, 100),
        ];

        assert_eq!(
            passende_stufe(&levels, 100_000.0, 800.0).samples_per_peak,
            2_048
        );
    }
}

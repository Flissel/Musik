//! Tempo und Beatgrid aus der Onset-Hüllkurve.
//!
//! Drei Stufen mit bewusst unterschiedlichen Werkzeugen:
//!
//! 1. **Grob** per Autokorrelation. Sie vergleicht das Signal mit *einer*
//!    Verschiebung und ist damit unempfindlich gegen kleine Periodenfehler.
//! 2. **Grundperiode** per Autokorrelation an der halben Verschiebung.
//! 3. **Fein** per Kammfilter, der zugleich die Phase liefert.
//!
//! Die Reihenfolge ist nicht beliebig. Ein Kammfilter summiert über hunderte
//! Pulse, und ein Periodenfehler von einem halben Frame verschiebt den
//! fünfzigsten Puls bereits um fünfundzwanzig — als Grobsuche ist er deshalb
//! unbrauchbar, als Feinsuche in einem engen Fenster dagegen ideal.
//!
//! Die Phase ist der Teil, den man leicht vergisst: ohne sie hat man ein Tempo,
//! aber kein Grid, und Sync klingt trotz gleicher BPM falsch.

use audio_core::track::CHANNELS;

use crate::onset::OnsetEnvelope;

/// Untergrenze der Tempo-Erkennung.
///
/// # Was darunter passiert — gemessen, nicht vermutet
///
/// **Eine harte Untergrenze weist unter sich nicht ab. Sie meldet falsch.**
/// Gebautes Material mit 66 BPM bekommt ein Grid mit **71,51 BPM**, eines mit
/// 68 eines mit 69,88 — und zwar mit hoher Deutlichkeit, denn innerhalb des
/// abgeschnittenen Suchfensters ragt die beste Verschiebung sauber heraus. Sie
/// sitzt nur am Rand. Ein Grid, das um 8 % danebenliegt, ist schlimmer als
/// keines: Jeder Beat driftet, Sync zieht das andere Deck mit, und nichts sagt
/// es.
///
/// Das Verfahren selbst trägt tiefer. Mit geöffnetem Fenster findet die
/// Autokorrelation zwischen 46 und 58 BPM die richtige Periode, deutlich und
/// stabil — zwei unabhängige Gegenproben (Median der Hüllkurven-Abstände,
/// Grobsuche mit tieferer Grenze) stimmen dort auf ein halbes BPM überein.
/// Die Grenze liegt also an dieser Konstante, nicht am Verfahren.
///
/// # Warum sie trotzdem steht
///
/// Zwei naheliegende Auswege wurden ausprobiert und sind beide falsch:
///
/// 1. **Das Fenster weiter aufmachen** lädt den Halbtempo-Fehler ein, und zwar
///    messbar: Bei Material mit Snare auf zwei und vier greift die Grobsuche
///    auf den Zweitakt-Zyklus. Der Demo-Track mit 124 BPM bekam so gar kein
///    Grid mehr. Einen Backbeat hat fast jede Musik.
/// 2. **Den Rand erkennen** geht nicht lokal. Ein echtes Stück mit 71 BPM sitzt
///    mit 0,989 · Fensterrand **näher** am Rand als das falsche 66 mit 0,975,
///    und „die Korrelation steigt am Rand noch" trifft 92 und 128 BPM genauso.
///    Die nötige Auskunft — wo die eigentliche Spitze liegt — steht per
///    Konstruktion außerhalb des Fensters.
///
/// Der Weg dahin führt über die Oktavwahl: Erst wenn die Entscheidung zwischen
/// Periode und halber Periode ohne enges Fenster trägt, darf das Fenster
/// aufgehen. Das ist eine eigene Aufgabe und keine Schwellenkorrektur.
///
/// **Bis dahin gilt: Material unter 70 BPM bekommt ein Grid, dem nicht zu
/// trauen ist** — nicht „kein Grid", sondern ein falsches. Der Messweg dorthin
/// steht in `docs/FAHRPLAN.md` unter N1.
const MIN_BPM: f64 = 70.0;
const MAX_BPM: f64 = 200.0;

/// Sanfter Erwartungswert für die Oktavwahl, nur für die Grobsuche.
const PRIOR_CENTER_BPM: f64 = 120.0;
const PRIOR_WIDTH: f64 = 1.2;

/// Ab welcher Güte die halbe Periode als die eigentliche gilt. Siehe [`fundamental`].
const FUNDAMENTAL_RATIO: f32 = 0.8;

/// Wie weit die Autokorrelationsspitze aus dem Feld der übrigen
/// Verschiebungen herausragen muss, damit das Ergebnis als Aussage durchgeht
/// statt als Rauschen. Gemessen in Standardabweichungen — siehe
/// [`coarse_period`], warum nicht als Verhältnis zum Mittelwert.
///
/// Gemessen: Klick-Tracks 4,2–6,0; ein dichter Loop aus Kick, Bass und Hi-Hats
/// 3,5; ein reiner Dauerton 1,7. Der Wert liegt bewusst näher am unteren Ende
/// — lieber ein zweifelhaftes Grid, das der Nutzer sieht und korrigieren kann,
/// als gar keins. Siehe `schwellen_trennen_perkussiv_von_dauerton`.
///
/// **An fünf echten Aufnahmen nachgemessen** (August 2026): Sie liegen bei 3,0
/// bis 4,3 — also über der Schwelle, aber deutlich unter dem, was gebautes
/// Material erreicht. Kein Tempo wurde dabei fälschlich abgewiesen. Der Abstand
/// nach unten ist allerdings dünn, und fünf Aufnahmen sind keine Sammlung.
///
/// **Wichtiger noch: Die daraus abgeleitete Konfidenz sagt nichts über die
/// Richtigkeit.** Bei denselben fünf Aufnahmen war das Tempo mit 0,12 genauso
/// richtig wie mit 0,46 — nachgeprüft über die Oktavlage und über eine
/// Feinsuche, die in allen fünf Fällen bis auf 0,09 BPM denselben Wert fand,
/// stabil über alle Drittel des Stücks. Wer `bpm_confidence` als Gütesiegel
/// liest, liest etwas hinein, das nicht drinsteht: Sie misst, wie deutlich die
/// Spitze aus dem Feld ragt, nicht ob sie an der richtigen Stelle sitzt.
const MIN_SALIENCE: f32 = 2.5;

/// Glättung der Hüllkurve für die Grobstufe, in Frames. Macht die
/// Autokorrelation an ganzzahligen Verschiebungen tolerant gegen den halben
/// Frame, um den ein Onset danebenliegen kann.
const SMOOTH: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct Beatgrid {
    pub bpm: f32,
    /// Erster erkannter Beat, in Sample-Frames.
    pub anchor_frames: u64,
    /// 0..1, abgeleitet aus der Schärfe der Autokorrelationsspitze.
    pub confidence: f32,
}

/// Ermittelt Tempo und ersten Beat.
///
/// `None`, wenn das Material zu kurz ist oder keine verwertbare Periodizität
/// zeigt — eine geratene Zahl wäre schlechter als keine.
pub fn detect(env: &OnsetEnvelope) -> Option<Beatgrid> {
    let v = &env.values;
    let p_short = env.rate * 60.0 / MAX_BPM;
    let p_long = env.rate * 60.0 / MIN_BPM;

    if v.len() < (p_long * 4.0) as usize {
        return None;
    }
    if v.iter().all(|x| *x <= 0.0) {
        return None;
    }

    let smoothed = smooth(v, SMOOTH);
    let (coarse, salience) = coarse_period(&smoothed, env.rate, p_short, p_long)?;

    if salience < MIN_SALIENCE {
        return None;
    }

    let period = fundamental(&smoothed, coarse, p_short);
    let (period, phase) = refine(v, period, p_short);

    Some(Beatgrid {
        bpm: (env.rate * 60.0 / period) as f32,
        anchor_frames: env.to_sample_frames(phase).round().max(0.0) as u64,
        confidence: ((salience - MIN_SALIENCE) / 4.0).clamp(0.0, 1.0),
    })
}

/// Beste Verschiebung nach Autokorrelation, gewichtet mit dem Oktav-Prior.
///
/// Zweiter Rückgabewert ist die Deutlichkeit: wie viele Standardabweichungen
/// die Spitze über dem Mittel aller Verschiebungen liegt.
///
/// Naheliegender wäre Spitze durch Mittelwert gewesen, und genau so stand es
/// hier auch — bis dichtes Material es widerlegt hat. Ein Klick-Track ist
/// zwischen den Klicks still, also korreliert er bei fast jeder Verschiebung
/// mit nahezu null und das Verhältnis wird riesig (14–27). Echte Musik hat
/// dagegen durchgehend Energie: die Korrelation hat überall einen hohen
/// Sockel, und das Verhältnis rutscht gegen 1, auch wenn der Beat schnurgerade
/// durchläuft. Ein dichter Loop aus Kick, Bass und Hi-Hats kam so auf 2,06 —
/// und wäre an einer Schwelle von 3,0 hängengeblieben, obwohl das Tempo längst
/// richtig erkannt war.
///
/// Der z-Wert misst stattdessen, ob **eine** Verschiebung aus dem Feld
/// heraussticht. Ein konstanter Sockel verschiebt Spitze und Mittel gleich
/// weit und kürzt sich heraus. Auf demselben Material trennt er doppelt so
/// deutlich: 3,46 gegen 1,71 statt 2,06 gegen 1,65.
fn coarse_period(v: &[f32], rate: f64, p_short: f64, p_long: f64) -> Option<(f64, f32)> {
    let lo = p_short.floor().max(2.0) as usize;
    let hi = p_long.ceil() as usize;
    if hi <= lo {
        return None;
    }

    let mut best = 0.0f64;
    let mut best_weighted = f32::NEG_INFINITY;
    let mut peak = 0.0f32;
    let mut korrelationen = Vec::with_capacity(hi - lo + 1);

    for lag in lo..=hi {
        let r = autocorrelation(v, lag as f64);
        korrelationen.push(r);
        peak = peak.max(r);

        let weighted = r * octave_prior(rate * 60.0 / lag as f64) as f32;
        if weighted > best_weighted {
            best_weighted = weighted;
            best = lag as f64;
        }
    }

    if best == 0.0 || korrelationen.is_empty() {
        return None;
    }

    let salience = z_wert(peak, &korrelationen);
    Some((best, salience))
}

/// Abstand eines Wertes vom Mittel, in Standardabweichungen.
///
/// Null, wenn alle Werte gleich sind — dann sticht nichts heraus, und genau
/// das soll die Zahl aussagen.
fn z_wert(wert: f32, werte: &[f32]) -> f32 {
    if werte.is_empty() {
        return 0.0;
    }

    let n = werte.len() as f64;
    let mittel = werte.iter().map(|r| *r as f64).sum::<f64>() / n;
    let varianz = werte
        .iter()
        .map(|r| {
            let d = *r as f64 - mittel;
            d * d
        })
        .sum::<f64>()
        / n;
    let sd = varianz.sqrt();

    if sd > 0.0 {
        ((wert as f64 - mittel) / sd) as f32
    } else {
        0.0
    }
}

/// Steigt von einer Verschiebung zu ihrer Grundperiode ab.
///
/// Der Kern des Oktavproblems: Die Autokorrelation hat Spitzen bei *jedem*
/// Vielfachen der Beat-Periode. Bei 174 BPM liegt eine bei 174 und eine
/// genauso hohe bei 87 — der Prior allein wählt die falsche, weil 87 näher an
/// 120 liegt.
///
/// Umgekehrt gilt es nicht: Bei der *halben* Periode liegt ein Tal, nicht eine
/// Spitze. Deshalb wird halbiert, solange die Korrelation dabei erhalten
/// bleibt, und man landet bei der kleinsten Periode der Reihe.
///
/// Es bleibt eine echte Mehrdeutigkeit: 87-BPM-HipHop mit Hi-Hats auf Achteln
/// wird als 174 erkannt. Genau dafür haben DJ-Programme den
/// Halbieren/Verdoppeln-Knopf, und den brauchen wir auch.
fn fundamental(v: &[f32], start: f64, p_short: f64) -> f64 {
    let mut period = start;
    let mut score = autocorrelation(v, period);

    loop {
        let half = period / 2.0;
        if half < p_short {
            return period;
        }
        let half_score = autocorrelation(v, half);
        if half_score < FUNDAMENTAL_RATIO * score {
            return period;
        }
        period = half;
        score = half_score;
    }
}

/// Feinsuche ±1 Frame per Kammfilter, auf der ungeglätteten Hüllkurve.
///
/// Hier kann die Oktave nicht mehr kippen, dafür wird die Periode auf zwei
/// Nachkommastellen genau — nötig, damit der Kamm auch am Ende eines langen
/// Tracks noch auf den Beats sitzt.
fn refine(v: &[f32], around: f64, p_short: f64) -> (f64, f64) {
    let mut best = (around, 0.0, f32::NEG_INFINITY);

    let mut p = around - 1.0;
    while p <= around + 1.0 {
        if p >= p_short {
            let (score, phase) = comb(v, p);
            if score > best.2 {
                best = (p, phase, score);
            }
        }
        p += 0.02;
    }

    (best.0, best.1)
}

/// Blockgröße der Zeitbereichs-Nachkorrektur. 64 Samples ≈ 1,5 ms bei 44,1 kHz.
const FINE_HOP: usize = 64;

/// Zieht den Anker im Zeitbereich nach.
///
/// Der spektrale Fluss schlägt aus, sobald ein Transient ins Analysefenster
/// *eintritt* — also bis zu einer Fensterlänge, bevor er tatsächlich erklingt.
/// Über die ganze Kette bleibt davon ein systematischer Vorlauf von gut zehn
/// Millisekunden übrig. Bei 128 BPM sind das vier Prozent eines Beats, und ein
/// so verschobenes Grid macht Sync unbrauchbar.
///
/// Die Korrektur sucht deshalb auf einer feinen Energie-Anstiegskurve nach,
/// und zwar nur innerhalb einer halben Periode um den vorhandenen Anker. Damit
/// kann sie die Phase schärfen, aber nicht auf einen anderen Beat springen.
pub fn refine_anchor(samples: &[f32], sample_rate: u32, grid: Beatgrid) -> Beatgrid {
    let period = 60.0 / grid.bpm as f64 * sample_rate as f64;
    if !period.is_finite() || period < (FINE_HOP * 4) as f64 {
        return grid;
    }

    let mono: Vec<f32> = samples
        .chunks_exact(CHANNELS)
        .map(|f| 0.5 * (f[0] + f[1]))
        .collect();

    let blocks = mono.len() / FINE_HOP;
    if blocks < 8 {
        return grid;
    }

    let mut rise = Vec::with_capacity(blocks);
    let mut prev = 0.0f32;
    for b in 0..blocks {
        let energy: f32 = mono[b * FINE_HOP..(b + 1) * FINE_HOP]
            .iter()
            .map(|v| v * v)
            .sum();
        rise.push((energy - prev).max(0.0));
        prev = energy;
    }

    let period_blocks = period / FINE_HOP as f64;
    let start = grid.anchor_frames as f64 / FINE_HOP as f64;

    let mut best = (f32::NEG_INFINITY, start);
    let mut offset = -period_blocks / 2.0;
    while offset <= period_blocks / 2.0 {
        let phase = start + offset;
        offset += 1.0;
        if phase < 0.0 {
            continue;
        }

        let mut sum = 0.0f64;
        let mut n = 0usize;
        let mut t = phase;
        while t < (blocks - 1) as f64 {
            sum += interpolate(&rise, t) as f64;
            n += 1;
            t += period_blocks;
        }

        if n > 0 {
            let score = (sum / n as f64) as f32;
            if score > best.0 {
                best = (score, phase);
            }
        }
    }

    Beatgrid {
        anchor_frames: (best.1 * FINE_HOP as f64).max(0.0).round() as u64,
        ..grid
    }
}

fn smooth(v: &[f32], win: usize) -> Vec<f32> {
    if win <= 1 {
        return v.to_vec();
    }
    let half = win / 2;
    (0..v.len())
        .map(|i| {
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(v.len());
            v[lo..hi].iter().sum::<f32>() / (hi - lo) as f32
        })
        .collect()
}

fn autocorrelation(v: &[f32], lag: f64) -> f32 {
    let count = v.len() as f64 - lag - 1.0;
    if count <= 1.0 {
        return 0.0;
    }
    let count = count as usize;
    let mut sum = 0.0f64;
    for (i, x) in v.iter().enumerate().take(count) {
        sum += (*x * interpolate(v, i as f64 + lag)) as f64;
    }
    (sum / count as f64) as f32
}

/// Bevorzugt Tempi in der Nähe von [`PRIOR_CENTER_BPM`] auf einer Log-Skala.
fn octave_prior(bpm: f64) -> f64 {
    let d = (bpm / PRIOR_CENTER_BPM).log2() / PRIOR_WIDTH;
    (-0.5 * d * d).exp()
}

/// Legt einen Pulskamm der Periode `period` über die Hüllkurve und sucht die
/// Phase mit der größten Ausbeute.
///
/// Rückgabe: (mittlerer Wert an den Pulsstellen, Phase in Frames).
fn comb(v: &[f32], period: f64) -> (f32, f64) {
    let steps = (period * 2.0).round().max(1.0) as usize;
    let mut best = (f32::NEG_INFINITY, 0.0);

    for s in 0..steps {
        let phase = s as f64 * period / steps as f64;
        let mut sum = 0.0f64;
        let mut n = 0usize;
        let mut t = phase;

        while t < (v.len() - 1) as f64 {
            sum += interpolate(v, t) as f64;
            n += 1;
            t += period;
        }

        if n > 0 {
            let score = (sum / n as f64) as f32;
            if score > best.0 {
                best = (score, phase);
            }
        }
    }

    best
}

fn interpolate(v: &[f32], t: f64) -> f32 {
    let i = t.floor() as usize;
    if i + 1 >= v.len() {
        return v[v.len() - 1];
    }
    let frac = (t - i as f64) as f32;
    v[i] + (v[i + 1] - v[i]) * frac
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onset::onset_envelope;
    use crate::testing::click_track;

    const RATE: u32 = 44_100;

    fn erkenne(bpm: f64, offset_secs: f64, secs: f64) -> Beatgrid {
        let track = click_track(bpm, RATE, secs, offset_secs);
        let env = onset_envelope(&track, RATE);
        detect(&env).expect("kein Beatgrid erkannt")
    }

    fn deutlichkeit(samples: &[f32]) -> Option<f32> {
        let env = onset_envelope(samples, RATE);
        let sm = smooth(&env.values, SMOOTH);
        let p_short = env.rate * 60.0 / MAX_BPM;
        let p_long = env.rate * 60.0 / MIN_BPM;
        coarse_period(&sm, env.rate, p_short, p_long).map(|(_, sal)| sal)
    }

    fn dauerton(secs: f64) -> Vec<f32> {
        let frames = (RATE as f64 * secs) as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let v = (2.0 * std::f32::consts::PI * 220.0 * i as f32 / RATE as f32).sin() * 0.5;
            samples.push(v);
            samples.push(v);
        }
        samples
    }

    /// Klicks auf einem durchgehenden Klangteppich.
    ///
    /// Das ist der Fall, an dem die alte Deutlichkeit gescheitert ist: der Beat
    /// ist so gerade wie beim Klick-Track, aber der Ton dazwischen hebt den
    /// Sockel der Autokorrelation an. Echte Musik sieht so aus, ein nackter
    /// Klick-Track nicht.
    fn klicks_auf_teppich(bpm: f64, secs: f64) -> Vec<f32> {
        let mut samples = click_track(bpm, RATE, secs, 0.0);
        let ton = dauerton(secs);
        for (s, t) in samples.iter_mut().zip(ton.iter()) {
            *s += *t;
        }
        samples
    }

    /// Material knapp über [`MIN_BPM`] — gebaut, mit Kick auf jedem Beat und
    /// Snare auf zwei und vier, auf durchgehendem Bass.
    fn langsam(bpm: f64, secs: f64) -> Vec<f32> {
        let je_beat = RATE as f64 * 60.0 / bpm;
        let n = (RATE as f64 * secs) as usize;
        let mut mono = vec![0.0f32; n];
        // Durchgehender Ton, damit der Sockel der Autokorrelation liegt wie
        // bei echter Musik und nicht wie bei einem Klick-Track.
        for (i, s) in mono.iter_mut().enumerate() {
            let t = i as f32 / RATE as f32;
            *s += 0.20 * (std::f32::consts::TAU * 55.0 * t).sin();
            *s += 0.12 * (std::f32::consts::TAU * 220.0 * t).sin();
        }
        let mut beat = 0usize;
        loop {
            let start = (beat as f64 * je_beat) as usize;
            if start + RATE as usize / 4 >= n {
                break;
            }
            for i in 0..RATE as usize / 4 {
                let t = i as f32 / RATE as f32;
                let f = 110.0 * (-t * 30.0).exp() + 45.0;
                mono[start + i] += (std::f32::consts::TAU * f * t).sin() * (-t * 22.0).exp() * 0.9;
                if beat % 2 == 1 {
                    let rausch = ((i * 1103515245 + 12345) % 2048) as f32 / 1024.0 - 1.0;
                    mono[start + i] += rausch * (-t * 40.0).exp() * 0.35;
                }
            }
            beat += 1;
        }
        mono.iter().flat_map(|v| [*v, *v]).collect()
    }

    /// **Langsames Material mit Backbeat kippt nicht auf die halbe Periode.**
    ///
    /// Der Test steht hier, weil genau das beim Versuch passiert ist, das
    /// Suchfenster zu öffnen: Bei Snare auf zwei und vier gewann der
    /// Zweitakt-Zyklus, und der Demo-Track mit 124 BPM bekam gar kein Grid
    /// mehr. Solange [`MIN_BPM`] das Fenster eng hält, hält auch das hier —
    /// wer daran rührt, sieht es sofort.
    #[test]
    fn langsames_material_mit_backbeat_kippt_nicht_auf_die_haelfte() {
        for bpm in [71.0, 75.0, 92.0] {
            let grid = detect(&crate::onset::onset_envelope(&langsam(bpm, 40.0), RATE))
                .unwrap_or_else(|| panic!("{bpm} BPM wurde nicht erkannt"));
            let ab = (grid.bpm as f64 - bpm).abs();
            assert!(
                ab < 1.5,
                "{bpm} BPM wurde als {:.2} erkannt — auf die Hälfte gekippt?",
                grid.bpm
            );
        }
    }

    /// Sichert den Abstand ab, auf dem [`MIN_SALIENCE`] beruht. Rutschen die
    /// Enden zusammen, ist die Schwelle nicht mehr begründet.
    #[test]
    fn schwellen_trennen_perkussiv_von_dauerton() {
        for bpm in [100.0, 128.0, 174.0] {
            let sal = deutlichkeit(&click_track(bpm, RATE, 25.0, 0.0))
                .unwrap_or_else(|| panic!("keine Periode bei {bpm} BPM"));
            assert!(
                sal > 4.0,
                "Klick-Track bei {bpm} BPM nur bei Deutlichkeit {sal:.2}"
            );
        }

        let sal = deutlichkeit(&dauerton(20.0)).unwrap_or(0.0);
        assert!(
            sal < MIN_SALIENCE,
            "Dauerton erreicht Deutlichkeit {sal:.2} und käme durch"
        );
    }

    /// Der eigentliche Grund für den z-Wert: dichtes Material muss durchkommen.
    ///
    /// Mit Spitze-durch-Mittelwert lag genau dieser Fall bei rund 2 und wurde
    /// abgewiesen, obwohl das Tempo richtig erkannt war.
    #[test]
    fn ein_durchgehender_klangteppich_verdeckt_den_beat_nicht() {
        for bpm in [100.0, 128.0] {
            let samples = klicks_auf_teppich(bpm, 25.0);

            let sal =
                deutlichkeit(&samples).unwrap_or_else(|| panic!("keine Periode bei {bpm} BPM"));
            assert!(
                sal > MIN_SALIENCE,
                "Beat unter Dauerton nur bei Deutlichkeit {sal:.2} — käme nicht durch"
            );

            let grid = detect(&onset_envelope(&samples, RATE))
                .unwrap_or_else(|| panic!("kein Grid bei {bpm} BPM"));
            assert!(
                (grid.bpm as f64 - bpm).abs() < 1.0,
                "{} statt {bpm} BPM",
                grid.bpm
            );
        }
    }

    #[test]
    fn erkennt_gaengige_tempi() {
        for bpm in [100.0, 128.0, 140.0, 174.0] {
            let grid = erkenne(bpm, 0.0, 25.0);
            assert!(
                (grid.bpm as f64 - bpm).abs() < 1.0,
                "erwartet {bpm} BPM, erkannt {:.2}",
                grid.bpm
            );
        }
    }

    /// Abstand des Ankers zum nächstgelegenen echten Beat, in Sekunden.
    /// Der Anker darf auf jedem Beat sitzen, nur eben genau.
    fn rasterfehler(grid: &Beatgrid, bpm: f64, offset: f64) -> f64 {
        let period = 60.0 / bpm;
        let anchor = grid.anchor_frames as f64 / RATE as f64;
        let versatz = ((anchor - offset) / period).rem_euclid(1.0);
        versatz.min(1.0 - versatz) * period
    }

    #[test]
    fn findet_den_ersten_beat() {
        let offset = 0.3;
        let grid = erkenne(128.0, offset, 25.0);
        let fehler = rasterfehler(&grid, 128.0, offset);

        assert!(
            fehler < 0.025,
            "Anker liegt {:.1} ms neben dem Raster",
            fehler * 1000.0
        );
    }

    #[test]
    fn nachkorrektur_schaerft_den_anker() {
        let offset = 0.4;
        let samples = click_track(128.0, RATE, 25.0, offset);
        let env = onset_envelope(&samples, RATE);
        let roh = detect(&env).expect("kein Beatgrid");
        let fein = refine_anchor(&samples, RATE, roh);

        let vorher = rasterfehler(&roh, 128.0, offset);
        let nachher = rasterfehler(&fein, 128.0, offset);

        assert!(
            nachher < 0.005,
            "nach der Korrektur immer noch {:.1} ms daneben",
            nachher * 1000.0
        );
        assert!(
            nachher <= vorher,
            "Korrektur hat verschlechtert: {:.1} ms → {:.1} ms",
            vorher * 1000.0,
            nachher * 1000.0
        );
    }

    #[test]
    fn nachkorrektur_springt_nicht_auf_einen_anderen_beat() {
        let offset = 0.4;
        let samples = click_track(128.0, RATE, 25.0, offset);
        let env = onset_envelope(&samples, RATE);
        let roh = detect(&env).expect("kein Beatgrid");
        let fein = refine_anchor(&samples, RATE, roh);

        let period = 60.0 / 128.0 * RATE as f64;
        let sprung = (fein.anchor_frames as f64 - roh.anchor_frames as f64).abs();
        assert!(
            sprung <= period / 2.0 + 1.0,
            "Anker ist um {sprung:.0} Frames gewandert, mehr als eine halbe Periode"
        );
        assert_eq!(fein.bpm, roh.bpm, "Tempo darf sich nicht ändern");
    }

    #[test]
    fn waehlt_die_grundperiode_statt_der_haelfte() {
        // 87 BPM korreliert bei 174er Material genauso stark wie 174 selbst.
        // Der Abstieg muss die kleinere Periode liefern.
        let grid = erkenne(174.0, 0.0, 25.0);
        assert!(
            grid.bpm > 150.0,
            "in der halben Frequenz hängengeblieben: {:.1} BPM",
            grid.bpm
        );
    }

    #[test]
    fn verdoppelt_nicht_ueber_das_material_hinaus() {
        // Bei 100 BPM läge 200 noch im Suchbereich, dort ist aber ein Tal.
        let grid = erkenne(100.0, 0.0, 25.0);
        assert!(
            (grid.bpm - 100.0).abs() < 1.0,
            "fälschlich verdoppelt: {:.1} BPM",
            grid.bpm
        );
    }

    #[test]
    fn zu_kurzes_material_ergibt_none() {
        let track = click_track(128.0, RATE, 1.0, 0.0);
        let env = onset_envelope(&track, RATE);
        assert!(detect(&env).is_none());
    }

    #[test]
    fn stille_ergibt_none() {
        let env = onset_envelope(&vec![0.0; RATE as usize * 2 * 30], RATE);
        assert!(detect(&env).is_none());
    }
}

//! Surveys the PQTZ beat grids of a real stick in `local-fixtures/` against their tracks in
//! `export.pdb`, to find the rules rekordbox follows for constant-tempo grids, and checks
//! `BeatGrid::constant_tempo` against them. Skips itself when the fixture is missing.
//!
//! The stick holds the user's library, so the tests print counts and shapes only, never
//! library text.
//!
//! Notation: `t_n` is the time of beat `n` (0 for the first), `I` = 60000 / BPM ms is the
//! interval of the stored tempo, and `φ` is a real-valued phase.

mod common;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cdj_media::anlz::{AnlzFile, Beat, BeatGrid, ConstantGrid, FourCc, Tag};
use cdj_media::encoding::{BeatInBar, Millis};
use cdj_media::pdb::{Database, Pdb};

/// One track with analysis files on the stick.
struct Sample {
    beats: Vec<Beat>,
    /// The pdb duration, in whole seconds.
    duration_s: u16,
    /// The pdb tempo, in centi-BPM.
    track_tempo: u32,
    sample_rate: u32,
    /// Entries of the `.EXT` PWV3 tag (one per 1/150 s), if there is one.
    pwv3_entries: Option<usize>,
}

impl Sample {
    /// The track's end as the PWV3 waveform gives it, in ms.
    fn pwv3_end(&self) -> Option<f64> {
        self.pwv3_entries.map(|n| n as f64 * 1000.0 / 150.0)
    }
}

/// Loads every track whose `.DAT` exists, or returns `None` when the fixture is missing.
fn samples() -> Option<Vec<Sample>> {
    let root = common::local_fixture("real-stick")?;
    let bytes = std::fs::read(root.join("PIONEER/rekordbox/export.pdb")).expect("read pdb");
    let pdb = Pdb::from_bytes(&bytes).expect("decode pdb");
    let db = Database::from_pdb(&pdb).expect("load database");
    let mut out = Vec::new();
    for track in db.tracks() {
        let dat = root.join(track.analysis_path().trim_start_matches('/'));
        let Ok(bytes) = std::fs::read(&dat) else {
            continue;
        };
        let file = AnlzFile::from_bytes(&bytes).expect("decode .DAT");
        let beats = file.beat_grid().expect("beat grid").beats().to_vec();
        let pwv3_entries = std::fs::read(dat.with_extension("EXT"))
            .ok()
            .and_then(|bytes| {
                let ext = AnlzFile::from_bytes(&bytes).ok()?;
                match ext.tags_with(FourCc::PWV3).next()? {
                    Tag::Raw(raw) => Some(raw.body().len()),
                    _ => None,
                }
            });
        out.push(Sample {
            beats,
            duration_s: track.duration(),
            track_tempo: track.tempo().centi(),
            sample_rate: track.sample_rate(),
            pwv3_entries,
        });
    }
    assert!(!out.is_empty(), "no tracks with analysis files");
    Some(out)
}

/// Counts by key.
struct Hist<K: Ord>(BTreeMap<K, usize>);

impl<K: Ord> Default for Hist<K> {
    fn default() -> Self {
        Self(BTreeMap::new())
    }
}

impl<K: Ord + std::fmt::Debug> Hist<K> {
    fn add(&mut self, key: K) {
        *self.0.entry(key).or_default() += 1;
    }

    fn report(&self, out: &mut String, title: &str) {
        let _ = writeln!(out, "  {title}:");
        for (k, n) in &self.0 {
            let _ = writeln!(out, "    {n:6} {k:?}");
        }
    }
}

/// The beat interval of a tempo in centi-BPM, in ms.
fn interval(centi: u32) -> f64 {
    6_000_000.0 / f64::from(centi)
}

fn times(beats: &[Beat]) -> Vec<f64> {
    beats.iter().map(|b| f64::from(b.time().get())).collect()
}

/// Whether every beat has the first beat's tempo.
fn is_constant(beats: &[Beat]) -> bool {
    beats.windows(2).all(|w| w[0].tempo() == w[1].tempo())
}

/// Whether every interval is `floor(I)` or `ceil(I)`.
fn is_regular(t: &[f64], i: f64) -> bool {
    t.windows(2).all(|w| {
        let d = w[1] - w[0];
        d == i.floor() || d == i.ceil()
    })
}

/// The phases φ for which `floor(φ + n·j)` gives every `t_n`, as `[lo, hi)`, if any.
///
/// `round(φ + n·j)` is the same rule with φ shifted by ½, so it fits the same grids.
fn floor_phases(t: &[f64], j: f64) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (f64::MIN, f64::MAX);
    for (n, &tn) in t.iter().enumerate() {
        let x = n as f64 * j;
        lo = lo.max(tn - x);
        hi = hi.min(tn + 1.0 - x);
    }
    (lo < hi).then_some((lo, hi))
}

/// The intervals j for which some φ makes `floor(φ + n·j)` give every `t_n`. In one
/// dimension, intervals that meet pairwise meet in common, so the pairwise bounds are exact.
fn floor_intervals(t: &[f64]) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (f64::MIN, f64::MAX);
    for n in 0..t.len() {
        for m in n + 1..t.len() {
            let k = (m - n) as f64;
            lo = lo.max((t[m] - t[n] - 1.0) / k);
            hi = hi.min((t[m] - t[n] + 1.0) / k);
        }
    }
    (lo < hi).then_some((lo, hi))
}

/// Splits `t` greedily into runs that each fit `floor(φ + n·I)` with their own φ. Returns
/// the number of runs.
fn runs_at(t: &[f64], i: f64) -> usize {
    let mut count = 1;
    let mut start = 0;
    let (mut lo, mut hi) = (f64::MIN, f64::MAX);
    for (m, &tm) in t.iter().enumerate() {
        let x = (m - start) as f64 * i;
        let (new_lo, new_hi) = (lo.max(tm - x), hi.min(tm + 1.0 - x));
        if new_lo < new_hi {
            (lo, hi) = (new_lo, new_hi);
        } else {
            count += 1;
            start = m;
            (lo, hi) = (tm, tm + 1.0);
        }
    }
    count
}

/// Whether some `s0` makes a two-stage rule give every `t_n`: a whole sample
/// `s_n = round(s0 + n·P)` (or `floor`), then `t_n = floor(s_n · 1000 / sr)` (or `round`).
fn two_stage_fits(t: &[f64], sr: f64, p: f64, sample_round: bool, ms_round: bool) -> bool {
    let (mut lo, mut hi) = (f64::MIN, f64::MAX);
    for (n, &tn) in t.iter().enumerate() {
        let (lo_ms, hi_ms) = if ms_round {
            (tn - 0.5, tn + 0.5)
        } else {
            (tn, tn + 1.0)
        };
        // The whole samples that give `tn`.
        let a = (lo_ms * sr / 1000.0).ceil();
        let b = (hi_ms * sr / 1000.0).ceil() - 1.0;
        let (a, b) = if sample_round {
            (a - 0.5, b + 0.5)
        } else {
            (a, b + 1.0)
        };
        let x = n as f64 * p;
        lo = lo.max(a - x);
        hi = hi.min(b - x);
    }
    lo < hi
}

/// `floor(t0 + c + n·I)`: the grid anchored at the first beat with sub-ms offset `c`.
fn anchored(t0: f64, c: f64, i: f64, len: usize) -> Vec<f64> {
    (0..len).map(|n| (t0 + c + n as f64 * i).floor()).collect()
}

/// Buckets of a tenth.
fn tenths(x: f64) -> i64 {
    (x * 10.0).floor() as i64
}

/// Buckets for run counts.
fn run_bucket(runs: usize) -> &'static str {
    match runs {
        0 | 1 => "1",
        2 => "2",
        3..=5 => "3-5",
        6..=10 => "6-10",
        _ => "11+",
    }
}

#[test]
fn real_stick_beat_grid_survey() {
    let Some(samples) = samples() else {
        return;
    };
    let mut out = String::new();
    let mut shape = Hist::default();
    let mut grid_vs_pdb_tempo = Hist::default();
    let mut irregular_worst = Hist::default();
    let mut drift_last = Hist::default();
    let mut exact = Hist::default();
    let mut phase_lo = Hist::default();
    let mut offsets = Hist::default();
    let mut exact_grids = 0;
    let mut runs = Hist::default();
    let mut fitted = Hist::default();
    let mut fitted_offset = Hist::default();
    let mut two_stage = Hist::default();
    let mut first_in_intervals = Hist::default();
    let mut first_zero = Hist::default();
    let mut first_number = Hist::default();
    let mut phasing = Hist::default();
    let mut pwv3_vs_pdb = Hist::default();
    let mut tail_pwv3 = Hist::default();
    let mut count_vs: BTreeMap<&str, Hist<i64>> = BTreeMap::new();

    for s in &samples {
        let beats = &s.beats;
        if beats.is_empty() {
            shape.add("empty");
            continue;
        }
        if !is_constant(beats) {
            shape.add("tempo varies");
            continue;
        }
        let centi = beats[0].tempo().centi();
        grid_vs_pdb_tempo.add(i64::from(centi) - i64::from(s.track_tempo));
        let i = interval(centi);
        let t = times(beats);
        let (t0, last) = (t[0], t.len() - 1);
        let is_exact = floor_phases(&t, i).is_some();

        // Where the grid starts, and how beats are numbered.
        first_in_intervals.add((t0 / i).floor() as i64);
        let b0 = beats[0].bar_position().number();
        if t0 == 0.0 {
            first_zero.add((b0, is_exact));
        }
        first_number.add(b0);
        phasing.add(beats.iter().enumerate().all(|(n, b)| {
            usize::from(b.bar_position().number()) == (usize::from(b0) - 1 + n) % 4 + 1
        }));

        // Where it ends.
        if let Some(end) = s.pwv3_end() {
            pwv3_vs_pdb.add(((end - f64::from(s.duration_s) * 1000.0) / 100.0).floor() as i64);
            tail_pwv3.add(tenths((end - t[last]) / i));
            // Beats t0 + n·I before `end`.
            let predicted = |end: f64| ((end - t0) / i).ceil() as i64;
            let actual = t.len() as i64;
            for (name, end) in [
                ("PWV3 end", end),
                ("pdb seconds", f64::from(s.duration_s) * 1000.0),
                ("pdb seconds + 1", f64::from(s.duration_s + 1) * 1000.0),
            ] {
                count_vs
                    .entry(name)
                    .or_default()
                    .add(actual - predicted(end));
            }
        }

        if !is_regular(&t, i) {
            shape.add("one tempo, some intervals off by 1 ms or more");
            let worst = t
                .windows(2)
                .map(|w| (w[1] - w[0] - i).abs())
                .fold(0.0, f64::max);
            irregular_worst.add(worst.ceil() as i64);
            continue;
        }
        shape.add("one tempo, every interval floor(I) or ceil(I)");
        drift_last.add(((t[last] - t0 - last as f64 * i) / 10.0).round() as i64 * 10);
        exact.add(is_exact);

        let sr = f64::from(s.sample_rate);
        let p = sr * 60.0 / (f64::from(centi) / 100.0);
        two_stage.add((
            is_exact,
            [(true, false), (false, false), (true, true), (false, true)]
                .map(|(samples, ms)| two_stage_fits(&t, sr, p, samples, ms)),
        ));

        if let Some((lo, _)) = floor_phases(&t, i) {
            exact_grids += 1;
            phase_lo.add(tenths(lo - t0));
            for c in 0..10 {
                if anchored(t0, f64::from(c) / 10.0, i, t.len()) == t {
                    offsets.add(c);
                }
            }
            continue;
        }

        // Inexact: runs at the stored tempo, and finer tempos that fit.
        runs.add(run_bucket(runs_at(&t, i)));
        match floor_intervals(&t) {
            None => fitted.add("no single tempo fits"),
            Some((jlo, jhi)) => {
                fitted.add("a finer tempo fits");
                let mid = (6_000_000.0 / jhi + 6_000_000.0 / jlo) / 2.0;
                // In milli-BPM.
                fitted_offset.add(((mid - f64::from(centi)) * 10.0).round() as i64);
            }
        }
    }

    let _ = writeln!(out, "beat grid survey over {} tracks", samples.len());
    shape.report(&mut out, "grid shapes");
    grid_vs_pdb_tempo.report(&mut out, "grid centi-BPM − pdb track tempo");
    irregular_worst.report(&mut out, "irregular: worst |interval − I|, ms (ceil)");
    drift_last.report(&mut out, "regular: t_last − (t0 + last·I), ms (to 10)");
    exact.report(
        &mut out,
        "regular: some φ gives floor(φ + n·I) = t_n (exact)",
    );
    phase_lo.report(&mut out, "exact: lowest φ − t0, tenths of ms");
    let _ = writeln!(
        out,
        "  exact: floor(t0 + c/10 + n·I) = t_n, of {exact_grids} (c = 5 is round):"
    );
    offsets.report(&mut out, "    by c");
    two_stage.report(
        &mut out,
        "regular: (exact, two-stage samples/ms fits: [round/floor, floor/floor, round/round, \
         floor/round])",
    );
    runs.report(&mut out, "inexact: runs that each fit the stored tempo");
    fitted.report(&mut out, "inexact: one finer tempo for the whole grid");
    fitted_offset.report(&mut out, "inexact: that tempo − stored, milli-BPM");
    first_in_intervals.report(&mut out, "floor(t0 / I)");
    first_zero.report(&mut out, "t0 = 0: (first beat_number, exact)");
    first_number.report(&mut out, "first beat_number");
    phasing.report(&mut out, "beat_number cycles 1-4 from the first");
    pwv3_vs_pdb.report(&mut out, "PWV3 duration − pdb duration, 100 ms buckets");
    tail_pwv3.report(&mut out, "(PWV3 end − last beat) / I, tenths");
    for (name, hist) in &count_vs {
        hist.report(&mut out, &format!("beat count − beats t0 + n·I < {name}"));
    }
    eprintln!("{out}");

    // The rules every grid on the stick follows.
    assert!(
        !shape.0.contains_key("tempo varies"),
        "a grid changes tempo"
    );
    let only = |hist: &Hist<i64>, value| hist.0.keys().all(|&k| k == value);
    assert!(
        only(&grid_vs_pdb_tempo, 0),
        "grid tempo equals the pdb tempo"
    );
    assert!(
        only(&first_in_intervals, 0),
        "first beat within one interval of 0"
    );
    assert!(phasing.0.keys().all(|&c| c), "bar positions cycle 1 to 4");
}

/// How a rebuilt grid compares with rekordbox's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    /// Every beat equal, and as many.
    Equal,
    /// The same beats as far as both go, but one grid is longer, by this many beats.
    CountDiffers(i64),
    /// The grids start on a different beat.
    StartDiffers,
    /// Aligned, with every time within 1 ms and equal bar positions.
    WithinOneMs,
    /// Aligned, with some time off by more than 1 ms.
    Further,
    /// The real grid has no beat 1 to rebuild from.
    NoDownbeat,
}

/// Rebuilds `beats` from its first downbeat, its tempo and `duration`.
fn rebuild(beats: &[Beat], duration: Millis) -> Option<BeatGrid> {
    let anchor = beats.iter().find(|b| b.bar_position() == BeatInBar::One)?;
    let grid = ConstantGrid::new(anchor.time(), anchor.tempo(), duration)?;
    Some(BeatGrid::constant_tempo(&grid))
}

/// Compares a rebuilt grid with the real one; also returns the number of beats equal on
/// both sides, position by position.
fn compare(real: &[Beat], rebuilt: &[Beat]) -> (Outcome, usize) {
    let equal = real.iter().zip(rebuilt).filter(|(a, b)| a == b).count();
    let worst = real
        .iter()
        .zip(rebuilt)
        .map(|(a, b)| a.time().get().abs_diff(b.time().get()))
        .max()
        .unwrap_or(0);
    let outcome = if real == rebuilt {
        Outcome::Equal
    } else if real.first().map(Beat::bar_position) != rebuilt.first().map(Beat::bar_position)
        || worst > 400
    {
        Outcome::StartDiffers
    } else if equal == real.len().min(rebuilt.len()) {
        Outcome::CountDiffers(rebuilt.len() as i64 - real.len() as i64)
    } else if worst <= 1
        && real
            .iter()
            .zip(rebuilt)
            .all(|(a, b)| a.bar_position() == b.bar_position())
    {
        Outcome::WithinOneMs
    } else {
        Outcome::Further
    };
    (outcome, equal)
}

/// Rebuilds every real grid from its first downbeat, tempo and track length, and counts how
/// many `BeatGrid::constant_tempo` gives back exactly. A grid can only come back exactly
/// when it is `round(φ + n·I)` for some φ (exact in the survey) and the sub-ms phase φ
/// happens to sit where rounding from the whole-ms downbeat puts it.
#[test]
fn real_stick_constant_grids_rebuild() {
    let Some(samples) = samples() else {
        return;
    };
    let mut out = String::new();
    let mut outcomes: BTreeMap<&str, Hist<Outcome>> = BTreeMap::new();
    let mut by_class: BTreeMap<(&str, &str), Hist<Outcome>> = BTreeMap::new();
    let mut start_differs: BTreeMap<&str, Hist<bool>> = BTreeMap::new();
    let mut exact_outcomes = Hist::default();
    let (mut beats_total, mut beats_equal) = (0, 0);
    let mut grids = 0;
    for s in &samples {
        let beats = &s.beats;
        if beats.is_empty() {
            continue;
        }
        grids += 1;
        let t = times(beats);
        let i = interval(beats[0].tempo().centi());
        let class = if !is_regular(&t, i) {
            "irregular"
        } else if floor_phases(&t, i).is_some() {
            "exact"
        } else {
            "inexact"
        };
        let pdb_end = Millis::new(u32::from(s.duration_s) * 1000);
        let pwv3_end = s.pwv3_entries.map(|n| Millis::new((n as u32) * 20 / 3));
        for (name, end) in [("PWV3 end", pwv3_end), ("pdb seconds", Some(pdb_end))] {
            let Some(end) = end else {
                continue;
            };
            let Some(rebuilt) = rebuild(beats, end) else {
                outcomes.entry(name).or_default().add(Outcome::NoDownbeat);
                continue;
            };
            let (outcome, equal) = compare(beats, rebuilt.beats());
            if outcome == Outcome::StartDiffers {
                // Whether the grid that has the extra first beat has it within 1 ms of 0.
                let near_zero = |b: &[Beat]| b.first().is_some_and(|b| b.time().get() <= 1);
                start_differs
                    .entry(name)
                    .or_default()
                    .add(near_zero(beats) || near_zero(rebuilt.beats()));
            }
            if class == "exact" && name == "PWV3 end" {
                exact_outcomes.add(outcome);
            }
            outcomes.entry(name).or_default().add(outcome);
            by_class.entry((name, class)).or_default().add(outcome);
            if name == "PWV3 end" {
                beats_total += beats.len();
                beats_equal += equal;
            }
        }
    }
    let _ = writeln!(
        out,
        "rebuilt {grids} grids from first downbeat, tempo and length"
    );
    for (name, hist) in &outcomes {
        hist.report(&mut out, &format!("length from {name}"));
    }
    for ((name, class), hist) in &by_class {
        hist.report(&mut out, &format!("length from {name}, {class} grids"));
    }
    for (name, hist) in &start_differs {
        hist.report(
            &mut out,
            &format!("length from {name}, start differs: one grid starts within 1 ms of 0"),
        );
    }
    let _ = writeln!(
        out,
        "  beats equal (PWV3 length): {beats_equal} of {beats_total}"
    );
    eprintln!("{out}");

    // Exact grids differ from their rebuild only by the unknown sub-ms phase: never by more
    // than 1 ms, and at the start only by a beat within 1 ms of 0.
    assert!(
        !exact_outcomes.0.contains_key(&Outcome::Further),
        "an exact grid rebuilt more than 1 ms off"
    );
    for hist in start_differs.values() {
        assert!(
            !hist.0.contains_key(&false),
            "a grid starts on another beat for a reason other than a beat near 0"
        );
    }
    let equal = outcomes["PWV3 end"].0.get(&Outcome::Equal).copied();
    assert_eq!(equal, Some(121), "grids rebuilt exactly (PWV3 length)");
}

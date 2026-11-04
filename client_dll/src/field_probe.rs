//! Guided field discovery: record bounded windows of the player's actor,
//! physics, body and scene while a person performs one action per timed phase,
//! then point offline at the words each action changes and at the float triples
//! that match the speed derived from the position. Candidates only: a word that
//! follows a key press is not a proven contract, and nothing here calls the engine.
//! Recordings hold raw game memory (including pointers): private, never versioned.
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::collections::BTreeMap;

pub const MAGIC: &[u8; 6] = b"FPRB1\n";
/// Word-aligned window sizes in bytes, in file order.
pub const WINDOWS: [(&str, usize); 4] = [
    ("actor", 0x2000),
    ("physics", 0x800),
    ("body", 0x400),
    ("scene", 0x5000),
];
/// Body position, in the same chain the pose capture reads.
pub const BODY_POSITION: usize = 0x30;
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FRAMES: usize = 20_000;
/// Time a person needs to complete an action before a phase counts as settled.
pub const SETTLE_MS: u32 = 3_000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Phase {
    pub name: &'static str,
    pub prompt: &'static str,
    pub seconds: u32,
    /// Motion phases feed the speed correlation instead of the toggle report.
    pub motion: bool,
}
const fn step(name: &'static str, prompt: &'static str, seconds: u32) -> Phase {
    Phase {
        name,
        prompt,
        seconds,
        motion: false,
    }
}
const fn drive(name: &'static str, prompt: &'static str, seconds: u32) -> Phase {
    Phase {
        name,
        prompt,
        seconds,
        motion: true,
    }
}
pub const SCRIPT: &[Phase] = &[
    step(
        "idle",
        "Caminhao parado, motor ligado. Nao toque em nada.",
        10,
    ),
    step("lights_on", "LIGUE os farois.", 8),
    step("lights_off", "DESLIGUE os farois.", 8),
    step("right_on", "LIGUE a seta DIREITA.", 8),
    step("right_off", "DESLIGUE a seta direita.", 8),
    step("left_on", "LIGUE a seta ESQUERDA.", 8),
    step("left_off", "DESLIGUE a seta esquerda.", 8),
    step("hazard_on", "LIGUE o pisca-alerta.", 8),
    step("hazard_off", "DESLIGUE o pisca-alerta.", 8),
    step("wipers_on", "LIGUE os limpadores.", 8),
    step("wipers_off", "DESLIGUE os limpadores.", 8),
    step("handbrake_on", "PUXE o freio de mao.", 8),
    step("handbrake_off", "SOLTE o freio de mao.", 8),
    step("horn_on", "SEGURE a buzina ate a proxima fase.", 8),
    step("horn_off", "SOLTE a buzina.", 8),
    drive("straight", "ACELERE em linha reta.", 25),
    drive("curve_left", "Continue andando e vire para a ESQUERDA.", 10),
    drive("curve_right", "Continue andando e vire para a DIREITA.", 10),
    drive("stop", "FREIE ate parar.", 10),
    drive("reverse", "Ande de RE devagar.", 10),
    drive("stopped", "Pare o caminhao.", 8),
];
pub const TRAILER: &[Phase] = &[
    step("trailer_attach", "ENGATE o reboque.", 25),
    step("trailer_attached", "Reboque engatado. Fique parado.", 8),
    step("trailer_detach", "DESENGATE o reboque.", 25),
    step("trailer_detached", "Reboque solto. Fique parado.", 8),
];

/// Writes each window as deltas against the previous frame.
pub struct Recorder {
    out: Vec<u8>,
    previous: Vec<Vec<u32>>,
    frames: usize,
}
impl Recorder {
    pub fn new(phases: &[Phase]) -> Result<Self> {
        ensure!(!phases.is_empty() && phases.len() <= 256, "phase count");
        let names: Vec<_> = phases
            .iter()
            .map(|p| serde_json::json!({"name": p.name, "seconds": p.seconds, "motion": p.motion}))
            .collect();
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(serde_json::to_string(&names)?.as_bytes());
        out.push(b'\n');
        Ok(Self {
            out,
            previous: WINDOWS.iter().map(|(_, n)| vec![0; n / 4]).collect(),
            frames: 0,
        })
    }
    pub fn len(&self) -> usize {
        self.out.len()
    }
    pub fn is_empty(&self) -> bool {
        self.frames == 0
    }
    pub fn bytes(&self) -> &[u8] {
        &self.out
    }
    /// `windows` must match `WINDOWS`. A frame that would exceed the limits is refused.
    pub fn frame(&mut self, ms: u32, phase: u16, actor: u32, windows: &[&[u8]]) -> Result<()> {
        ensure!(windows.len() == WINDOWS.len(), "window count");
        ensure!(self.frames < MAX_FRAMES, "frame limit");
        let mut frame = Vec::new();
        frame.extend_from_slice(&ms.to_le_bytes());
        frame.extend_from_slice(&phase.to_le_bytes());
        frame.extend_from_slice(&actor.to_le_bytes());
        for (i, window) in windows.iter().enumerate() {
            ensure!(window.len() == WINDOWS[i].1, "window size");
            let mut changes: Vec<(u32, u32)> = vec![];
            for (w, chunk) in window.as_chunks::<4>().0.iter().enumerate() {
                let value = u32::from_le_bytes(*chunk);
                if self.previous[i][w] != value {
                    self.previous[i][w] = value;
                    changes.push((w as u32, value));
                }
            }
            frame.extend_from_slice(&(changes.len() as u32).to_le_bytes());
            for (w, value) in changes {
                frame.extend_from_slice(&w.to_le_bytes());
                frame.extend_from_slice(&value.to_le_bytes());
            }
        }
        ensure!(
            (self.out.len() + frame.len()) as u64 <= MAX_BYTES,
            "recording size limit"
        );
        self.out.extend_from_slice(&frame);
        self.frames += 1;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PhaseInfo {
    name: String,
    motion: bool,
}
struct Frame {
    ms: u32,
    phase: u16,
    actor: u32,
    changes: Vec<(u8, u32, u32)>,
}
struct Recording {
    phases: Vec<PhaseInfo>,
    frames: Vec<Frame>,
}
fn take<'a>(bytes: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    ensure!(bytes.len() >= n, "truncated recording");
    let (head, tail) = bytes.split_at(n);
    *bytes = tail;
    Ok(head)
}
fn u32_at(bytes: &mut &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(take(bytes, 4)?.try_into()?))
}
fn parse(bytes: &[u8]) -> Result<Recording> {
    ensure!(bytes.len() as u64 <= MAX_BYTES, "recording size limit");
    let mut rest = bytes.strip_prefix(MAGIC).context("not a field recording")?;
    let end = rest
        .iter()
        .position(|b| *b == b'\n')
        .context("missing phase header")?;
    let phases: Vec<PhaseInfo> = serde_json::from_slice(&rest[..end]).context("phase header")?;
    ensure!(!phases.is_empty() && phases.len() <= 256, "phase count");
    rest = &rest[end + 1..];
    let mut frames = vec![];
    while !rest.is_empty() {
        ensure!(frames.len() < MAX_FRAMES, "frame limit");
        let ms = u32_at(&mut rest)?;
        let phase = u16::from_le_bytes(take(&mut rest, 2)?.try_into()?);
        ensure!(usize::from(phase) < phases.len(), "unknown phase");
        let actor = u32_at(&mut rest)?;
        let mut changes = vec![];
        for (i, (_, size)) in WINDOWS.iter().enumerate() {
            let count = u32_at(&mut rest)? as usize;
            ensure!(count <= size / 4, "change count");
            for _ in 0..count {
                let word = u32_at(&mut rest)?;
                ensure!((word as usize) < size / 4, "word outside window");
                changes.push((i as u8, word, u32_at(&mut rest)?));
            }
        }
        frames.push(Frame {
            ms,
            phase,
            actor,
            changes,
        });
    }
    ensure!(!frames.is_empty(), "recording without frames");
    Ok(Recording { phases, frames })
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Toggle {
    pub window: &'static str,
    pub offset: String,
    pub before: String,
    pub after: String,
    /// The value came back in the following phase (a paired on/off action).
    pub returned: bool,
    /// Transitions of this word across the whole script; 2 is a clean on/off.
    pub transitions: usize,
}
#[derive(Debug, Serialize, PartialEq)]
pub struct Speed {
    pub window: &'static str,
    pub offset: String,
    pub kind: &'static str,
    pub scale: f64,
    pub relative_error: f64,
}
#[derive(Debug, Serialize, PartialEq)]
pub struct Counter {
    pub window: &'static str,
    pub offset: String,
    pub kind: &'static str,
    pub rate_per_second: f64,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub frames: usize,
    pub actor_changes: usize,
    pub actions: BTreeMap<String, Vec<Toggle>>,
    pub speed: Vec<Speed>,
    pub counters: Vec<Counter>,
    pub candidate_only: bool,
    pub engine_validated: bool,
}
/// A rendered value: small integers as integers, otherwise a plausible float.
fn show(value: u32) -> String {
    let f = f32::from_bits(value);
    if value < 0x10000 {
        value.to_string()
    } else if f.is_finite() && (1e-3..1e9).contains(&f.abs()) {
        format!("{f}f")
    } else {
        format!("{value:#010x}")
    }
}
const MAX_PER_ACTION: usize = 48;
const MAX_SPEED: usize = 12;
const MAX_COUNTERS: usize = 32;

pub fn analyze(bytes: &[u8]) -> Result<Report> {
    let rec = parse(bytes)?;
    let actor_changes = rec
        .frames
        .windows(2)
        .filter(|f| f[0].actor != f[1].actor)
        .count();
    // Full values per frame; only the first actor's run is comparable word by word.
    let first_actor = rec.frames[0].actor;
    let frames: Vec<&Frame> = rec
        .frames
        .iter()
        .take_while(|f| f.actor == first_actor)
        .collect();
    if frames.len() < 2 {
        bail!("recording too short to analyze");
    }
    let mut series: BTreeMap<(u8, u32), Vec<(usize, u32)>> = BTreeMap::new();
    for (n, frame) in frames.iter().enumerate() {
        for &(window, word, value) in &frame.changes {
            series.entry((window, word)).or_default().push((n, value));
        }
    }
    let at = |s: &[(usize, u32)], n: usize| -> u32 {
        match s.binary_search_by_key(&n, |e| e.0) {
            Ok(i) => s[i].1,
            Err(0) => 0,
            Err(i) => s[i - 1].1,
        }
    };
    // Settled frame range of each phase.
    let mut ranges = vec![None::<(usize, usize)>; rec.phases.len()];
    let mut start_ms = vec![None::<u32>; rec.phases.len()];
    for (n, frame) in frames.iter().enumerate() {
        let p = usize::from(frame.phase);
        let start = *start_ms[p].get_or_insert(frame.ms);
        if frame.ms.saturating_sub(start) >= SETTLE_MS {
            let r = ranges[p].get_or_insert((n, n));
            r.1 = n;
        }
    }
    let settled = |s: &[(usize, u32)], p: usize| -> Option<u32> {
        let (a, b) = ranges[p]?;
        let value = at(s, a);
        s.iter()
            .all(|(n, v)| *n <= a || *n > b || *v == value)
            .then_some(value)
    };
    let mut actions: BTreeMap<String, Vec<Toggle>> = BTreeMap::new();
    for ((window, word), s) in &series {
        // A word that never settles is noise for the toggle report.
        let values: Vec<Option<u32>> = (0..rec.phases.len()).map(|p| settled(s, p)).collect();
        let stable: Vec<(usize, u32)> = values
            .iter()
            .enumerate()
            .filter(|(p, _)| !rec.phases[*p].motion)
            .filter_map(|(p, v)| v.map(|v| (p, v)))
            .collect();
        let transitions = stable.windows(2).filter(|w| w[0].1 != w[1].1).count();
        for pair in stable.windows(2) {
            let ((a, before), (b, after)) = (pair[0], pair[1]);
            if b != a + 1 || before == after {
                continue;
            }
            let returned = values.get(b + 1).copied().flatten() == Some(before);
            actions
                .entry(rec.phases[b].name.clone())
                .or_default()
                .push(Toggle {
                    window: WINDOWS[usize::from(*window)].0,
                    offset: format!("{:#x}", word * 4),
                    before: show(before),
                    after: show(after),
                    returned,
                    transitions,
                });
        }
    }
    for list in actions.values_mut() {
        list.sort_by_key(|t| (!t.returned, t.transitions));
        list.truncate(MAX_PER_ACTION);
    }
    Ok(Report {
        schema: 1,
        frames: rec.frames.len(),
        actor_changes,
        actions,
        speed: speed(&rec, &frames, &series, &at),
        counters: counters(&frames, &series),
        candidate_only: true,
        engine_validated: false,
    })
}
type Series = BTreeMap<(u8, u32), Vec<(usize, u32)>>;
fn speed(
    rec: &Recording,
    frames: &[&Frame],
    series: &Series,
    at: &impl Fn(&[(usize, u32)], usize) -> u32,
) -> Vec<Speed> {
    let body = WINDOWS
        .iter()
        .position(|w| w.0 == "body")
        .expect("body window") as u8;
    let word = (BODY_POSITION / 4) as u32;
    let empty = vec![];
    let axis = |i: u32| series.get(&(body, word + i)).unwrap_or(&empty);
    let (px, py, pz) = (axis(0), axis(1), axis(2));
    // Velocity by central difference over the moving frames only.
    let mut samples: Vec<(usize, [f64; 3])> = vec![];
    for n in 1..frames.len().saturating_sub(1) {
        if !rec.phases[usize::from(frames[n].phase)].motion {
            continue;
        }
        let dt = f64::from(frames[n + 1].ms.saturating_sub(frames[n - 1].ms)) / 1000.0;
        if dt <= 0.0 {
            continue;
        }
        let d = |s: &Vec<(usize, u32)>| {
            let f = |k| f64::from(f32::from_bits(at(s, k)));
            (f(n + 1) - f(n - 1)) / dt
        };
        samples.push((n, [d(px), d(py), d(pz)]));
    }
    let norm = |v: &[f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let peak = samples.iter().map(|s| norm(&s.1)).fold(0.0, f64::max);
    if samples.len() < 8 || peak < 1.0 {
        return vec![];
    }
    let mut out = vec![];
    let value = |key: &(u8, u32), n: usize| -> Option<f64> {
        // A word that never changed is its initial zero, not missing data.
        let v = series
            .get(key)
            .map_or(0.0, |s| f64::from(f32::from_bits(at(s, n))));
        v.is_finite().then_some(v)
    };
    // Scales from cm/s (the world unit) to the units a game is likely to store.
    const SCALES: [f64; 4] = [1.0, 0.01, 0.036, 0.022_369_4];
    for key in series.keys() {
        let (window, w) = *key;
        // Scalar speed: |velocity| times some unit factor.
        for scale in SCALES {
            let (mut err, mut sum, mut seen) = (0.0, 0.0, 0usize);
            for (n, v) in &samples {
                let Some(x) = value(key, *n) else { continue };
                let expected = norm(v) * scale;
                err += (x.abs() - expected).powi(2);
                sum += expected.powi(2);
                seen += 1;
            }
            if seen == samples.len() && sum > 0.0 {
                let relative_error = (err / sum).sqrt();
                if relative_error < 0.25 {
                    out.push(Speed {
                        window: WINDOWS[usize::from(window)].0,
                        offset: format!("{:#x}", w * 4),
                        kind: "scalar",
                        scale,
                        relative_error,
                    });
                }
            }
        }
        // Vector: three consecutive floats, same scale as the world.
        let (kx, ky, kz) = ((window, w), (window, w + 1), (window, w + 2));
        if w + 2 < (WINDOWS[usize::from(window)].1 / 4) as u32 {
            for scale in SCALES {
                let (mut err, mut sum, mut seen) = (0.0, 0.0, 0usize);
                for (n, v) in &samples {
                    let (Some(x), Some(y), Some(z)) =
                        (value(&kx, *n), value(&ky, *n), value(&kz, *n))
                    else {
                        continue;
                    };
                    err += (x - v[0] * scale).powi(2)
                        + (y - v[1] * scale).powi(2)
                        + (z - v[2] * scale).powi(2);
                    sum += (norm(v) * scale).powi(2);
                    seen += 1;
                }
                if seen == samples.len() && sum > 0.0 {
                    let relative_error = (err / sum).sqrt();
                    if relative_error < 0.25 {
                        out.push(Speed {
                            window: WINDOWS[usize::from(window)].0,
                            offset: format!("{:#x}", w * 4),
                            kind: "vector",
                            scale,
                            relative_error,
                        });
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.relative_error.total_cmp(&b.relative_error));
    out.truncate(MAX_SPEED);
    out
}
/// Words that only ever grow at a steady rate: clock and tick candidates.
fn counters(frames: &[&Frame], series: &Series) -> Vec<Counter> {
    let total = frames.len();
    let seconds = f64::from(frames[total - 1].ms.saturating_sub(frames[0].ms)) / 1000.0;
    let mut out = vec![];
    if seconds <= 0.0 {
        return out;
    }
    for ((window, word), s) in series {
        // Must keep changing throughout, not jump once.
        if s.len() < 8 || s.len() * 4 < total {
            continue;
        }
        let as_u = s.windows(2).all(|p| p[1].1 >= p[0].1);
        let floats: Vec<f32> = s.iter().map(|e| f32::from_bits(e.1)).collect();
        let as_f = floats.iter().all(|f| f.is_finite())
            && floats.windows(2).all(|p| p[1] >= p[0])
            && floats[floats.len() - 1] > floats[0];
        let (kind, delta) = if as_f {
            (
                "f32",
                f64::from(floats[floats.len() - 1]) - f64::from(floats[0]),
            )
        } else if as_u {
            ("u32", f64::from(s[s.len() - 1].1 - s[0].1))
        } else {
            continue;
        };
        out.push(Counter {
            window: WINDOWS[usize::from(*window)].0,
            offset: format!("{:#x}", word * 4),
            kind,
            rate_per_second: delta / seconds,
        });
    }
    out.truncate(MAX_COUNTERS);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn phases() -> Vec<Phase> {
        vec![
            step("idle", "", 6),
            step("lights_on", "", 6),
            step("lights_off", "", 6),
            drive("straight", "", 12),
        ]
    }
    /// Own memory: lights at actor+0x40, speed vector at physics+0x20, scalar
    /// km/h at actor+0x80, clock at scene+0x100, a noisy word at actor+0x44.
    fn recording() -> Vec<u8> {
        let list = phases();
        let mut rec = Recorder::new(&list).unwrap();
        let mut ms = 0u32;
        let mut x = 1000.0f32;
        for (p, phase) in list.iter().enumerate() {
            for tick in 0..phase.seconds * 10 {
                let mut windows: Vec<Vec<u8>> =
                    WINDOWS.iter().map(|(_, n)| vec![0u8; *n]).collect();
                let moving = phase.motion;
                let v = if moving { 500.0f32 + tick as f32 } else { 0.0 };
                x += v * 0.1;
                let put = |w: &mut Vec<u8>, at: usize, b: [u8; 4]| {
                    w[at..at + 4].copy_from_slice(&b);
                };
                put(&mut windows[0], 0x40, u32::from(p == 1).to_le_bytes());
                put(&mut windows[0], 0x44, (ms * 7919).to_le_bytes());
                put(&mut windows[0], 0x80, (v * 0.036).to_le_bytes());
                put(&mut windows[1], 0x20, v.to_le_bytes());
                put(&mut windows[1], 0x24, 0f32.to_le_bytes());
                put(&mut windows[1], 0x28, 0f32.to_le_bytes());
                put(&mut windows[2], BODY_POSITION, x.to_le_bytes());
                put(&mut windows[3], 0x100, (ms as f32 / 100.0).to_le_bytes());
                let refs: Vec<&[u8]> = windows.iter().map(Vec::as_slice).collect();
                rec.frame(ms, p as u16, 0x1000, &refs).unwrap();
                ms += 100;
            }
        }
        rec.bytes().to_vec()
    }
    #[test]
    fn action_words_speed_and_clock_are_found_without_the_noise() {
        let report = analyze(&recording()).unwrap();
        let on = &report.actions["lights_on"];
        assert_eq!(on[0].offset, "0x40");
        assert_eq!((on[0].before.as_str(), on[0].after.as_str()), ("0", "1"));
        assert!(on[0].returned && on[0].transitions == 2);
        assert!(
            !report
                .actions
                .values()
                .flatten()
                .any(|t| t.offset == "0x44")
        );
        let vector = report
            .speed
            .iter()
            .find(|s| s.window == "physics" && s.kind == "vector")
            .unwrap();
        assert_eq!(vector.offset, "0x20");
        assert!(vector.relative_error < 0.05);
        assert!(report.speed.iter().any(|s| {
            s.window == "actor" && s.offset == "0x80" && s.kind == "scalar" && s.scale == 0.036
        }));
        let clock = report
            .counters
            .iter()
            .find(|c| c.offset == "0x100")
            .unwrap();
        assert_eq!((clock.window, clock.kind), ("scene", "f32"));
        assert!((clock.rate_per_second - 10.0).abs() < 0.5);
        assert!(report.candidate_only && !report.engine_validated);
    }
    #[test]
    fn deltas_stay_small_and_malformed_recordings_are_refused() {
        let bytes = recording();
        // Far smaller than the 28 KiB-per-frame raw windows.
        assert!(bytes.len() < 300 * 30_000 / 100);
        assert!(analyze(b"nope").is_err());
        assert!(analyze(&bytes[..bytes.len() - 3]).is_err());
        let mut rec = Recorder::new(&phases()).unwrap();
        assert!(analyze(rec.bytes()).is_err());
        assert!(rec.frame(0, 0, 1, &[&[0u8; 4]]).is_err());
        let short: Vec<Vec<u8>> = WINDOWS.iter().map(|(_, n)| vec![0u8; n / 2]).collect();
        let refs: Vec<&[u8]> = short.iter().map(Vec::as_slice).collect();
        assert!(rec.frame(0, 0, 1, &refs).is_err());
        assert!(Recorder::new(&[]).is_err());
    }
    #[test]
    fn a_new_actor_ends_the_comparable_run() {
        let list = phases();
        let mut rec = Recorder::new(&list).unwrap();
        let windows: Vec<Vec<u8>> = WINDOWS.iter().map(|(_, n)| vec![0u8; *n]).collect();
        let refs: Vec<&[u8]> = windows.iter().map(Vec::as_slice).collect();
        rec.frame(0, 0, 1, &refs).unwrap();
        rec.frame(100, 0, 1, &refs).unwrap();
        rec.frame(200, 0, 2, &refs).unwrap();
        let report = analyze(rec.bytes()).unwrap();
        assert_eq!((report.frames, report.actor_changes), (3, 1));
    }
    #[test]
    fn scripts_are_bounded_and_named_uniquely() {
        let mut names: Vec<_> = SCRIPT.iter().chain(TRAILER).map(|p| p.name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
        for phase in SCRIPT.iter().chain(TRAILER) {
            assert!(phase.seconds * 1000 > SETTLE_MS, "{}", phase.name);
            assert!(phase.prompt.is_ascii());
        }
    }
}

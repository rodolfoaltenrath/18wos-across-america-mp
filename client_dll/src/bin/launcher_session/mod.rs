//! One-command session: probes, Drawn network source and the remote visual,
//! then an ordered teardown that waits for safe release and verifies slots.
//! Only composes already validated commands; stops at the first failed step.
use anyhow::{Context, Result, bail};
use client_dll::{
    api::{self, Control, EngineControl},
    controller::Controller,
    draw_observation::{Remote, Visual},
    inspection::process::Process,
    load_gate::Status as GateStatus,
    profile,
    release_policy::{Refusal, Report as ReleaseReport},
};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    path::Path,
    time::{Duration, Instant},
};

pub struct Options {
    pub server: SocketAddr,
    pub player: u32,
    pub offset: [f32; 3],
    /// Apply the AI truck skin (fixture: its own shader name).
    pub skin: bool,
    /// Install the active world-command gate (release/replay/recreate on map, exit, ...).
    pub load_gate: bool,
    /// Gate deadline from the load request to recreation, in milliseconds.
    pub load_timeout_ms: u32,
    /// engine_fixture's first stdout line; None targets the real game.
    pub fixture: Option<Value>,
}

fn engine(c: &Controller, dll: &Path, mut b: EngineControl) -> Result<EngineControl> {
    c.engine_control(dll, &mut b)?;
    Ok(b)
}
fn words<const N: usize>(info: &Value, key: &str) -> Result<[u32; N]> {
    let mut out = [0; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = info[key][i]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .with_context(|| format!("fixture info lacks {key}[{i}]"))?;
    }
    Ok(out)
}

pub fn start(c: &Controller, dll: &Path, o: &Options) -> Result<Value> {
    let mut steps = Vec::new();
    let mut step = |name: &str, result: Result<()>| -> Result<()> {
        match result {
            Ok(()) => {
                steps.push(json!({"step": name, "ok": true}));
                Ok(())
            }
            Err(e) => {
                steps.push(json!({"step": name, "ok": false, "error": e.to_string()}));
                Err(e.context(format!("session-start failed at {name}")))
            }
        }
    };
    let fixture = o.fixture.as_ref();
    let outcome = (|| -> Result<()> {
        step("load", c.load(dll))?;
        let mut attach = EngineControl::new(api::UI_ATTACH);
        if let Some(info) = fixture {
            attach.window = info["window"].as_u64().context("fixture window")? as u32;
        }
        step("attach", engine(c, dll, attach).map(drop))?;
        let pump = match fixture {
            Some(info) => {
                let mut b = EngineControl::new(api::UI_PUMP_FIXTURE);
                b.entity = info["peek_slot"].as_u64().context("fixture peek_slot")? as u32;
                b
            }
            None => EngineControl::new(api::UI_PUMP_ATTACH),
        };
        step("pump-attach", engine(c, dll, pump).map(drop))?;
        let render = match fixture {
            Some(info) => {
                let mut b = EngineControl::new(api::UI_RENDER_FIXTURE);
                b.fixture = words(info, "render_descriptors")?;
                b
            }
            None => EngineControl::new(api::UI_RENDER_ATTACH),
        };
        step("render-attach", engine(c, dll, render).map(drop))?;
        let draw = match fixture {
            Some(info) => {
                let mut b = EngineControl::new(api::UI_DRAW_FIXTURE);
                b.fixture = words(info, "draw_descriptors")?;
                b
            }
            None => EngineControl::new(api::UI_DRAW_ATTACH),
        };
        step("draw-attach", engine(c, dll, draw).map(drop))?;
        let mut connect = Control::new(api::START);
        connect.config.mode = api::DRAWN;
        api::put(&mut connect.config.server, &o.server.to_string()).map_err(anyhow::Error::msg)?;
        api::put(&mut connect.config.profile, profile::ID).map_err(anyhow::Error::msg)?;
        step("connect-drawn", c.control(dll, &mut connect).map(drop))?;
        if o.load_gate {
            let mut gate = match fixture {
                Some(info) => {
                    let mut b = EngineControl::new(api::UI_LOAD_GATE_FIXTURE);
                    b.fixture = words(info, "map_descriptors")?;
                    b
                }
                None => EngineControl::new(api::UI_LOAD_GATE_ATTACH),
            };
            gate.cycles = o.load_timeout_ms;
            step("load-gate-attach", engine(c, dll, gate).map(drop))?;
        }
        let mut show = match fixture {
            Some(info) => {
                let mut b = EngineControl::new(api::VISUAL_SHOW_FIXTURE);
                b.fixture = words(info, "instance_descriptors")?;
                let functions: [u32; 8] = words(info, "visual_descriptors")?;
                b.frames[..8].copy_from_slice(&functions);
                api::put(&mut b.name, "models/fixture/truck.psm").map_err(anyhow::Error::msg)?;
                if o.skin {
                    api::put(&mut b.message, "models/fixture/truck.shd")
                        .map_err(anyhow::Error::msg)?;
                }
                b
            }
            None => {
                let mut b = EngineControl::new(api::VISUAL_SHOW);
                api::put(&mut b.name, "models/ai/truck/models/truck.psm")
                    .map_err(anyhow::Error::msg)?;
                if o.skin {
                    api::put(&mut b.message, "models/ai/truck/shaders/truck.shd")
                        .map_err(anyhow::Error::msg)?;
                }
                b
            }
        };
        show.entity = o.player;
        [show.count_before, show.count_created, show.count_after] = o.offset.map(f32::to_bits);
        step("visual-show", engine(c, dll, show).map(drop))
    })();
    let report = json!({"started": outcome.is_ok(), "steps": steps});
    match outcome {
        Ok(()) => Ok(report),
        Err(e) => {
            eprintln!("{}", serde_json::to_string_pretty(&report)?);
            Err(e)
        }
    }
}

pub fn status(c: &Controller, dll: &Path) -> Result<Value> {
    let mut client = Control::new(api::STATUS);
    c.control(dll, &mut client)?;
    let visual = Visual::decode(&engine(c, dll, EngineControl::new(api::VISUAL_STATUS))?);
    let draw = client_dll::draw_observation::Summary::decode(&engine(
        c,
        dll,
        EngineControl::new(api::DRAW_STATUS),
    )?);
    let gate = gate_status(c, dll)?;
    let mut remotes = Vec::new();
    loop {
        let mut b = EngineControl::new(api::REMOTE_LIST);
        b.reserved = remotes.len() as u32;
        let row = Remote::decode(&engine(c, dll, b)?);
        if row.count == 0 {
            break;
        }
        remotes.push(row);
        if remotes.len() as u32 >= row.count || remotes.len() >= 64 {
            break;
        }
    }
    Ok(json!({
        "client": {"status": client.status, "sample_state": client.sample_state, "sent": client.sent,
            "received": client.received, "diagnostic": api::text(&client.message).unwrap_or("")},
        "visual": visual, "draw": draw, "load_gate": gate, "remotes": remotes,
    }))
}

/// Expected original slot values after restoration, game profile only.
fn verify_slots(pid: u32) -> Result<Vec<Value>> {
    let process = Process::open(pid)?;
    let modules = process.modules()?;
    let base = |name: &str| {
        modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .map(|m| m.base)
            .with_context(|| format!("{name} not loaded"))
    };
    let (game, core, gfx, engine) = (
        base("game.dll")?,
        base("prism3d.exe")?,
        base("gfxshared.dll")?,
        base("p3shared.dll")?,
    );
    let mut rows = Vec::new();
    let mut expected_slots: Vec<(String, u64, u64)> = [
        ("actors.car draw", game + 0x12580c, game + 0x5a6a0),
        ("actors.car init", game + 0x1257fc, game + 0x50f40),
        ("actors.car done", game + 0x125800, game + 0x515d0),
        ("init_eblock import", game + 0x124560, engine + 0xc270),
        ("destroy_eblock import", game + 0x1242b4, engine + 0xc600),
        ("cmd_argv binding", game + 0x160930, core + 0xe210),
        ("render_pool import", core + 0x22240, gfx + 0x3690),
        ("gfx_path_flush binding", game + 0x160ea0, core + 0x4820),
    ]
    .into_iter()
    .map(|(name, slot, expected)| (name.into(), slot, expected))
    .collect();
    let audit = client_dll::inspection::commands::inspect(&process)?;
    for (command, descriptor) in
        client_dll::command_registry::world_gate_descriptors(&audit).map_err(anyhow::Error::msg)?
    {
        expected_slots.push((
            format!("{} descriptor callback", command.name()),
            u64::from(descriptor) + 4,
            u64::from(audit.map_callback),
        ));
    }
    for (name, slot, expected) in expected_slots {
        let mut bytes = [0; 4];
        process.read(slot, &mut bytes)?;
        let actual = u64::from(u32::from_le_bytes(bytes));
        rows.push(json!({"slot": name, "restored": actual == expected,
            "actual": format!("0x{actual:x}"), "expected": format!("0x{expected:x}")}));
    }
    Ok(rows)
}

/// Release the visual before the game starts destroying its world resources.
/// Keep the probes/network alive, so the visual may be shown again after loading.
/// Gate status, or None when this DLL has no gate installed.
pub fn gate_status(c: &Controller, dll: &Path) -> Result<Option<GateStatus>> {
    let status = GateStatus::decode(&engine(c, dll, EngineControl::new(api::LOAD_GATE_STATUS))?)
        .map_err(anyhow::Error::msg)?;
    Ok(status.installed.then_some(status))
}
/// A deferred load owns the visual until it settles; never race its release.
fn wait_gate(c: &Controller, dll: &Path, timeout: Duration) -> Result<Option<GateStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        let status = gate_status(c, dll)?;
        if status.is_none_or(|s| s.quiescent()) {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            bail!(
                "deferred load still active after {timeout:?}: {status:?}; keep the game in the foreground and retry"
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
pub fn prepare_load(c: &Controller, dll: &Path, timeout: Duration) -> Result<Value> {
    let gate = wait_gate(c, dll, timeout)?;
    let mut refusals: Vec<String> = Vec::new();
    let mut attempts = 0;
    let mut checks = 0;
    let mut last_check = None;
    let visual = Visual::decode(&engine(c, dll, EngineControl::new(api::VISUAL_STATUS))?);
    if visual.state == 1 {
        engine(c, dll, EngineControl::new(api::VISUAL_HIDE))?;
        let deadline = Instant::now() + timeout;
        loop {
            checks += 1;
            let report =
                ReleaseReport::decode(&engine(c, dll, EngineControl::new(api::VISUAL_CHECK))?)
                    .map_err(anyhow::Error::msg)?;
            let reason = report.refusal;
            last_check = Some(serde_json::to_value(&report)?);
            match reason {
                Some(Refusal::NotAllocated) => break,
                None => {
                    attempts += 1;
                    // The DLL repeats all checks, on the loop, immediately before free.
                    engine(c, dll, EngineControl::new(api::VISUAL_RELEASE))
                        .context("release did not confirm completion after preflight; do not load; inspect current state before retry")?;
                    break;
                }
                Some(reason) if reason.retryable() => {
                    let message = reason.to_string();
                    if !refusals.contains(&message) {
                        refusals.push(message);
                    }
                }
                Some(reason) => bail!(
                    "cannot prepare load: {reason}; do not load. Release evidence: {last_check:?}"
                ),
            }
            if Instant::now() >= deadline {
                bail!(
                    "visual not released after {checks} checks/{attempts} release attempts: {refusals:?}; keep the game in the foreground and retry session-prepare-load. Evidence: {last_check:?}"
                );
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    Ok(
        json!({"released": visual.state == 1, "release_attempts": attempts,
        "refusals": refusals, "preflight_checks": checks, "last_release_check": last_check,
        "load_gate": gate, "prepared": true}),
    )
}

pub fn stop(c: &Controller, dll: &Path, pid: u32, timeout: Duration, game: bool) -> Result<Value> {
    let mut report = prepare_load(c, dll, timeout)?;
    for command in [
        api::UI_LOAD_GATE_REMOVE,
        api::UI_TRANSITION_REMOVE,
        api::UI_DRAW_REMOVE,
        api::UI_RENDER_REMOVE,
        api::UI_PUMP_REMOVE,
    ] {
        engine(c, dll, EngineControl::new(command))?;
    }
    let mut stop = Control::new(api::STOP);
    c.control(dll, &mut stop)?;
    let slots = if game { verify_slots(pid)? } else { Vec::new() };
    if slots.iter().any(|s| s["restored"] != true) {
        bail!("slots not restored: {slots:?}");
    }
    report["client_status"] = stop.status.into();
    report["slots"] = slots.into();
    report["note"] = "the DLL stays pinned until the game exits".into();
    Ok(report)
}

/// Originals checked around a game session, relative to prism3d.exe's folder.
const ORIGINALS: [&str; 5] = [
    "prism3d.exe",
    "aa/lib/game.dll",
    "p3shared.dll",
    "gfxshared.dll",
    "lib/psm.dll",
];
fn sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn tracked(root: Option<&Path>, saves: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files: Vec<_> = root
        .map(|r| ORIGINALS.iter().map(|o| r.join(o)).collect())
        .unwrap_or_default();
    let mut list: Vec<_> = std::fs::read_dir(saves)
        .with_context(|| format!("saves folder {}", saves.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    list.sort();
    files.extend(list);
    Ok(files)
}
/// Copy saves into a NEW folder and record SHA-256 of originals and saves.
pub fn backup(directory: &Path, root: Option<&Path>, saves: &Path) -> Result<Value> {
    std::fs::create_dir(directory)
        .with_context(|| format!("backup folder must be new: {}", directory.display()))?;
    let copies = directory.join("saves");
    std::fs::create_dir(&copies)?;
    let mut rows = Vec::new();
    for file in tracked(root, saves)? {
        let hash = sha256(&file)?;
        if file.starts_with(saves) {
            let name = file.file_name().context("save name")?;
            std::fs::copy(&file, copies.join(name))?;
            if sha256(&copies.join(name))? != hash {
                bail!("backup copy differs: {}", file.display());
            }
        }
        rows.push(json!({"path": file.to_string_lossy(), "sha256": hash}));
    }
    let manifest = json!({"files": rows});
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}
/// Compare current files with a backup manifest; changed or missing files fail.
pub fn verify(directory: &Path) -> Result<Value> {
    let manifest: Value = serde_json::from_slice(&std::fs::read(directory.join("manifest.json"))?)?;
    let mut rows = Vec::new();
    let mut changed = Vec::new();
    for row in manifest["files"].as_array().context("manifest files")? {
        let path = Path::new(row["path"].as_str().context("manifest path")?);
        let now = sha256(path).ok();
        let same = now.as_deref() == row["sha256"].as_str();
        if !same {
            changed.push(path.to_string_lossy().into_owned());
        }
        rows.push(json!({"path": row["path"], "unchanged": same}));
    }
    if !changed.is_empty() {
        bail!(
            "files changed since backup (copies kept in {}): {changed:?}",
            directory.display()
        );
    }
    Ok(json!({"unchanged": rows.len()}))
}

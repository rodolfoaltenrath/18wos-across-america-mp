//! External process reads of our own memory, no DLL load, hooks or game assets.
#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    appearance_capture::{self, Layout},
    appearance_evidence::{self, Refusal, Role},
    appearance_resources,
    inspection::{appearance, process::Process},
    pose_capture::{self, Unavailable},
};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
};
struct Host(Child);
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn external_read_candidates_follow_owned_changes_and_game_inspector_refuses_the_fixture() {
    let mut host = Host(
        Command::new(env!("CARGO_BIN_EXE_engine_fixture"))
            .args(["--ansi", "--appearance"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut stdout = BufReader::new(host.0.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let header: serde_json::Value = serde_json::from_str(&line).unwrap();
    let values: [u32; 3] =
        serde_json::from_value(header["appearance_descriptors"].clone()).unwrap();
    let process = Process::open(host.0.id()).unwrap();
    let layout = Layout {
        actor: pose_capture::Layout {
            root: values[0],
            steps: &[0x14c, 0x4610, 8, 8],
            vtable: values[1],
        },
        type_record: values[2],
    };
    let capture = || {
        appearance_capture::capture(layout, |at, out| {
            process.read(u64::from(at), out).map_err(|_| ())
        })
    };
    let first = capture().unwrap();
    let resource_ops = header["appearance_resource_ops"].as_u64().unwrap() as u32;
    let resources = || {
        appearance_resources::capture(layout, resource_ops, |at, out| {
            process.read(u64::from(at), out).map_err(|_| ())
        })
    };
    let first_resources = resources().unwrap();
    let evidence = || {
        appearance_evidence::collect(
            Role::Manual,
            || capture().map_err(Refusal::from),
            || resources().map_err(Refusal::from),
        )
    };
    let own_evidence = evidence();
    own_evidence.validate().unwrap();
    let share_temp = tempfile::tempdir().unwrap();
    let raw = share_temp.path().join("raw");
    let bundle = share_temp.path().join("share");
    appearance_evidence::save_next(&raw, &own_evidence).unwrap();
    let manifest = appearance_evidence::bundle(&raw, &bundle).unwrap();
    assert_eq!(manifest.summary.resources_available, 1);
    let doctor = Command::new(env!("CARGO_BIN_EXE_mp_doctor"))
        .arg("--appearance-evidence")
        .arg(&bundle)
        .output()
        .unwrap();
    assert!(
        doctor.status.success(),
        "{}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    let parsed: appearance_evidence::Manifest = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(parsed.summary, manifest.summary);
    let file = bundle.join("appearance-0001.json");
    let compare = Command::new(env!("CARGO_BIN_EXE_mp_doctor"))
        .arg("--appearance-capture")
        .arg(&file)
        .arg("--compare-appearance")
        .arg(&file)
        .output()
        .unwrap();
    assert!(compare.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&compare.stdout).unwrap();
    assert_eq!(parsed["differing_resource_fields"], 0);
    assert_eq!(parsed["world_synchronized"], false);
    let extracted = share_temp.path().join("resources.json");
    let extract = Command::new(env!("CARGO_BIN_EXE_mp_doctor"))
        .arg("--appearance-capture")
        .arg(&file)
        .args(["--extract-appearance", "resources", "--output"])
        .arg(&extracted)
        .output()
        .unwrap();
    assert!(extract.status.success());
    assert_eq!(
        appearance_resources::ResourceObservation::parse(&std::fs::read(&extracted).unwrap())
            .unwrap(),
        first_resources
    );
    let repeat = Command::new(env!("CARGO_BIN_EXE_mp_doctor"))
        .arg("--appearance-capture")
        .arg(&file)
        .args(["--extract-appearance", "resources", "--output"])
        .arg(&extracted)
        .output()
        .unwrap();
    assert!(!repeat.status.success());
    assert_eq!(first_resources.names, first);
    assert!(first_resources.instances[0].is_none() && first_resources.instances[1].is_none());
    assert_eq!(
        first_resources.instances[2]
            .as_ref()
            .unwrap()
            .material_keys
            .len(),
        1
    );
    first.validate().unwrap();
    assert_eq!(first.slots.len(), 3);
    assert!(appearance::inspect(&process).is_err());
    assert!(appearance::inspect_resources(&process).is_err());
    let refused = appearance_evidence::collect_process(Role::Manual, &process);
    assert_eq!(refused.names_refusal, Some(Refusal::GuardRefused));
    assert_eq!(refused.resources_refusal, Some(Refusal::GuardRefused));
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("must-not-exist.json");
    let command = Command::new(env!("CARGO_BIN_EXE_memory_tool"))
        .args(["appearance", "--pid", &host.0.id().to_string(), "--output"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(!command.status.success() && !output.exists());
    let command = Command::new(env!("CARGO_BIN_EXE_memory_tool"))
        .args([
            "appearance",
            "--resources",
            "--pid",
            &host.0.id().to_string(),
            "--output",
        ])
        .arg(&output)
        .output()
        .unwrap();
    assert!(!command.status.success() && !output.exists());
    let mut send = |text: &str| {
        writeln!(host.0.stdin.as_mut().unwrap(), "{text}").unwrap();
        line.clear();
        stdout.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "armed");
    };
    send("appearance-switch");
    let switched = capture().unwrap();
    assert_ne!(switched.make_definition_key, first.make_definition_key);
    assert_ne!(switched.slots[2], first.slots[2]);
    assert_ne!(
        resources().unwrap().instances[2],
        first_resources.instances[2]
    );
    send("appearance-missing");
    assert_eq!(capture(), Err(Unavailable::Missing));
    assert_eq!(resources(), Err(Unavailable::Missing));
    send("appearance-normal");
    assert_eq!(capture().unwrap(), first);
    assert_eq!(resources().unwrap(), first_resources);
    send("appearance-resource-invalid");
    assert_eq!(resources(), Err(Unavailable::Type));
    let partial = evidence();
    assert!(partial.names.is_some());
    assert_eq!(partial.resources_refusal, Some(Refusal::Type));
    send("appearance-normal");
    assert_eq!(resources().unwrap(), first_resources);
    send("appearance-count-invalid");
    assert_eq!(capture(), Err(Unavailable::Invalid));
    send("appearance-normal");
    assert_eq!(capture().unwrap(), first);
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
}

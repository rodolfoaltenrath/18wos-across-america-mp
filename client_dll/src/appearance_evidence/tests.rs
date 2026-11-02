use super::*;
use multiplayer_server::{
    career,
    vehicle_appearance::{
        observation::{Slot, Source},
        resources::Instance,
    },
};
fn names() -> Observation {
    Observation {
        schema: 1,
        profile: crate::profile::ID.into(),
        source: Source::ProfiledActorNameCandidates,
        make_definition_key: career::hash(b"own make"),
        slots: vec![Slot {
            index: 0,
            definition_key: Some(career::hash(b"own accessory")),
        }],
        candidate_only: true,
        engine_verified: false,
        world_synchronized: false,
    }
}
fn resources() -> ResourceObservation {
    ResourceObservation {
        schema: 1,
        names: names(),
        instances: vec![
            None,
            None,
            Some(Instance {
                model_key: career::hash(b"own model"),
                material_keys: vec![career::hash(b"own red"), career::hash(b"own glass")],
            }),
        ],
        candidate_only: true,
        complete_assembly: false,
        engine_verified: false,
        world_synchronized: false,
    }
}
fn both(role: Role) -> Capture {
    collect(role, || Ok(names()), || Ok(resources()))
}
#[test]
fn partial_readers_keep_safe_refusal_codes_and_disagreement_without_repair() {
    let c = collect(Role::Status, || Ok(names()), || Err(Refusal::Type));
    c.validate().unwrap();
    assert!(c.names.is_some() && c.resources.is_none());
    assert_eq!(c.resources_refusal, Some(Refusal::Type));
    assert_eq!(c.names_agree, None);
    let mut different = resources();
    different.names.make_definition_key = career::hash(b"other make");
    let c = collect(Role::Manual, || Ok(names()), || Ok(different));
    assert_eq!(c.names_agree, Some(false));
    c.validate().unwrap();
    let mut bad = names();
    bad.engine_verified = true;
    let c = collect(Role::Start, || Ok(bad), || Err(Refusal::GuardRefused));
    assert_eq!(c.names_refusal, Some(Refusal::Invalid));
    let mut invalid = both(Role::Stop);
    invalid.names_refusal = Some(Refusal::Changed);
    assert!(invalid.validate().is_err());
    let mut invalid = both(Role::Stop);
    invalid.names_agree = Some(false);
    assert!(invalid.validate().is_err());
}
#[test]
fn export_uses_only_dedicated_typed_samples_and_preserves_raw_inputs() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("appearance");
    fs::write(t.path().join("identity.json"), b"PRIVATE TOKEN").unwrap();
    fs::write(t.path().join("own.sav"), b"PRIVATE SAVE").unwrap();
    let a = save_next(&source, &both(Role::Start)).unwrap();
    let original = fs::read(&a).unwrap();
    save_next(
        &source,
        &collect(
            Role::Stop,
            || Err(Refusal::TargetUnavailable),
            || Err(Refusal::TargetUnavailable),
        ),
    )
    .unwrap();
    let out = t.path().join("share");
    let m = bundle(&source, &out).unwrap();
    assert_eq!(m.files.len(), 2);
    assert_eq!(m.summary.names_available, 1);
    assert_eq!(m.summary.resources_available, 1);
    assert_eq!(m.summary.model_fields_observed, 1);
    assert_eq!(m.summary.material_entries_observed, 2);
    assert!(!m.summary.validation_claimed);
    verify(&out).unwrap();
    assert_eq!(fs::read(a).unwrap(), original);
    assert!(!out.join(".INCOMPLETE").exists());
    for row in fs::read_dir(&out).unwrap() {
        let text = fs::read_to_string(row.unwrap().path()).unwrap();
        for raw in [
            "PRIVATE",
            "TOKEN",
            "own.sav",
            "identity.json",
            t.path().to_str().unwrap(),
            "own red",
            "own accessory",
        ] {
            assert!(!text.contains(raw));
        }
    }
    assert!(bundle(&source, &out).is_err());
    fs::write(source.join("identity.json"), b"PRIVATE").unwrap();
    let absent = t.path().join("refused");
    assert!(bundle(&source, &absent).is_err() && !absent.exists());
}
#[test]
fn verifier_refuses_changed_members_summary_unknown_claims_extras_and_incomplete_exports() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("source");
    save_next(&source, &both(Role::Start)).unwrap();
    let out = t.path().join("share");
    bundle(&source, &out).unwrap();
    let p = out.join(sample(0));
    let original = fs::read(&p).unwrap();
    fs::write(&p, b"{}").unwrap();
    assert!(verify(&out).is_err());
    fs::write(&p, &original).unwrap();
    let manifest = out.join(MANIFEST);
    let original_manifest = fs::read(&manifest).unwrap();
    let mut m: Manifest = serde_json::from_slice(&original_manifest).unwrap();
    m.summary.names_available = 7;
    fs::write(&manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(verify(&out).is_err());
    fs::write(&manifest, &original_manifest).unwrap();
    for extra in [".INCOMPLETE", "backup.sav"] {
        fs::write(out.join(extra), b"PRIVATE").unwrap();
        assert!(verify(&out).is_err());
        fs::remove_file(out.join(extra)).unwrap();
    }
    let mut j: serde_json::Value = serde_json::from_slice(&original).unwrap();
    j["pointer"] = 7.into();
    assert!(Capture::parse(&serde_json::to_vec(&j).unwrap()).is_err());
    j.as_object_mut().unwrap().remove("pointer");
    j["engine_verified"] = true.into();
    assert!(Capture::parse(&serde_json::to_vec(&j).unwrap()).is_err());
    #[cfg(unix)]
    {
        fs::remove_file(&p).unwrap();
        std::os::unix::fs::symlink(source.join(sample(0)), &p).unwrap();
        assert!(verify(&out).is_err());
    }
}
#[test]
fn append_cap_never_overwrites_and_unavailable_is_not_comparable_to_an_empty_model() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("source");
    let c = collect(
        Role::Status,
        || Err(Refusal::Missing),
        || Err(Refusal::Missing),
    );
    let first = save_next(&source, &c).unwrap();
    let original = fs::read(&first).unwrap();
    for _ in 1..MAX_SAMPLES {
        save_next(&source, &c).unwrap();
    }
    assert!(save_next(&source, &both(Role::Start)).is_err());
    assert_eq!(fs::read(first).unwrap(), original);
    let out = t.path().join("share");
    assert_eq!(bundle(&source, &out).unwrap().summary.captures, MAX_SAMPLES);
    let compare = compare(&c, &c).unwrap();
    assert!(!compare.names_comparable && !compare.resources_comparable);
    assert_eq!(compare.differing_resource_fields, None);
    assert!(Capture::parse(&vec![b' '; MAX_CAPTURE + 1]).is_err());
}
#[test]
fn explicit_pair_comparison_detects_model_material_and_accessory_order_changes() {
    let mut a = both(Role::Manual);
    a.preflight = Some(Preflight {
        kit_sha256: career::hash(b"own kit"),
        dll_sha256: career::hash(b"own DLL"),
    });
    let mut b = a.clone();
    let same = compare(&a, &b).unwrap();
    assert_eq!(same.make_equal, Some(true));
    assert_eq!(same.same_kit_preflight, Some(true));
    assert_eq!(same.differing_resource_fields, Some(0));
    assert_eq!(same.present_fields_left, Some(1));
    assert!(!same.world_synchronized && !same.engine_verified);
    b.names.as_mut().unwrap().slots[0].definition_key = None;
    b.resources.as_mut().unwrap().instances[2]
        .as_mut()
        .unwrap()
        .material_keys
        .reverse();
    b.names_agree = Some(false);
    b.preflight.as_mut().unwrap().dll_sha256 = career::hash(b"other own DLL");
    let diff = compare(&a, &b).unwrap();
    assert_eq!(diff.make_equal, Some(true));
    assert_eq!(diff.differing_name_slots, Some(1));
    assert_eq!(diff.differing_resource_fields, Some(1));
    assert_eq!(diff.same_dll_preflight, Some(false));
}
#[test]
fn own_decoder_mutations_remain_bounded_and_canonical_when_accepted() {
    let original = serde_json::to_vec(&both(Role::Status)).unwrap();
    let mut seed = 741u64;
    for i in 0..3000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut b = original.clone();
        let at = seed as usize % b.len();
        if i % 3 == 0 {
            b.truncate(at);
        } else {
            b[at] ^= (seed >> 32) as u8 | 1;
        }
        if let Ok(c) = Capture::parse(&b) {
            c.validate().unwrap();
            let bytes = serde_json::to_vec_pretty(&c).unwrap();
            assert!(bytes.len() <= MAX_CAPTURE);
            assert_eq!(Capture::parse(&bytes).unwrap(), c);
        }
    }
}

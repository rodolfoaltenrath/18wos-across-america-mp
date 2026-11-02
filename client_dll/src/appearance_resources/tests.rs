use super::*;
use crate::{pose_capture, profile};
const CAR: u32 = 0x2000;
const ITEM: u32 = 0x10000;
const DATA: u32 = 0x10200;
const INFO: u32 = 0x11000;
const MODEL: u32 = 0x12000;
const RED: u32 = 0x13000;
const GLASS: u32 = 0x14000;
const OPS: u32 = 0x1f000;
fn put(b: &mut [u8], at: u32, value: u32) {
    b[at as usize..at as usize + 4].copy_from_slice(&value.to_le_bytes());
}
fn text(b: &mut [u8], at: u32, t: &[u8]) {
    b[at as usize..at as usize + 64].fill(0);
    b[at as usize..at as usize + t.len()].copy_from_slice(t);
}
fn own() -> Vec<u8> {
    let mut b = vec![0; 0x20000];
    for (at, value) in [
        (0x100, 0x200),
        (0x204, CAR),
        (CAR, 0xa000),
        (CAR + 0x60, 0x6000),
        (CAR + 0x84, 0x9000),
        (CAR + 0x1d0, 0x7000),
        (0x73e4, 0x8000),
        (0x6008, CAR),
        (0x6034, 0xb000),
        (0x6048, profile::CAR_SIZE),
        (CAR + FIELDS[2], ITEM),
        (ITEM, 0x8a),
        (ITEM + 4, 22),
        (ITEM + 8, DATA),
        (ITEM + 0x30, OPS),
        (ITEM + 0x48, 0x400),
        (ITEM + 0x4c, 1),
        (DATA + 4, 2),
        (DATA + 8, INFO),
        (DATA + 0x20, DATA + 0x100),
        (DATA + 0x30, DATA + 0x110),
        (INFO, 1),
        (INFO + 4, MODEL),
        (DATA + 0x100, RED),
        (DATA + 0x104, GLASS),
        (DATA + 0x110, DATA + 0x200),
        (DATA + 0x114, DATA + 0x210),
        (DATA + 0x200, 1),
        (DATA + 0x204, ITEM),
        (DATA + 0x20c, 0),
        (DATA + 0x210, 1),
        (DATA + 0x214, ITEM),
        (DATA + 0x21c, 1),
    ] {
        put(&mut b, at, value);
    }
    b[(CAR + profile::APPEARANCE_MAKE) as usize..(CAR + profile::APPEARANCE_MAKE) as usize + 3]
        .copy_from_slice(b"own");
    for (at, name) in [
        (MODEL, b"makes/truck/own/cabin.psm".as_slice()),
        (RED, b"makes/truck/own/red.shd"),
        (GLASS, b"makes/truck/own/glass.shd"),
    ] {
        put(&mut b, at, 0x448);
        put(&mut b, at + 4, at);
        put(&mut b, at + 8, 0x18000);
        put(&mut b, at + 0x30, 0x19000);
        put(&mut b, at + 0x4c, 1);
        text(&mut b, at + 0x68, name);
    }
    b
}
fn layout() -> Layout<'static> {
    Layout {
        actor: pose_capture::Layout {
            root: 0x100,
            steps: &[4],
            vtable: 0xa000,
        },
        type_record: 0xb000,
    }
}
fn read(b: &[u8], at: u32, out: &mut [u8]) -> Result<(), ()> {
    let start = at as usize;
    out.copy_from_slice(
        b.get(start..start.checked_add(out.len()).ok_or(())?)
            .ok_or(())?,
    );
    Ok(())
}
#[test]
fn named_links_and_material_order_are_hashed_without_claiming_a_complete_assembly() {
    let b = own();
    let v = capture(layout(), OPS, |at, out| read(&b, at, out)).unwrap();
    assert!(v.instances[0].is_none() && v.instances[1].is_none());
    let main = v.instances[2].as_ref().unwrap();
    assert_eq!(
        main.model_key,
        multiplayer_server::career::hash(b"makes/truck/own/cabin.psm")
    );
    assert_eq!(
        main.material_keys,
        [
            multiplayer_server::career::hash(b"makes/truck/own/red.shd"),
            multiplayer_server::career::hash(b"makes/truck/own/glass.shd")
        ]
    );
    let bytes = serde_json::to_vec(&v).unwrap();
    assert_eq!(ResourceObservation::parse(&bytes).unwrap(), v);
    let json = String::from_utf8(bytes).unwrap();
    for raw in [
        "cabin.psm",
        "red.shd",
        "pointer",
        "address",
        "position",
        "root",
    ] {
        assert!(!json.contains(raw));
    }
    let mut upper = b;
    text(&mut upper, RED + 0x68, b"MAKES/TRUCK/OWN/RED.SHD");
    assert_eq!(
        capture(layout(), OPS, |at, out| read(&upper, at, out)).unwrap(),
        v
    );
    let mut empty = upper;
    put(&mut empty, CAR + FIELDS[2], 0);
    assert!(
        capture(layout(), OPS, |at, out| read(&empty, at, out))
            .unwrap()
            .instances
            .iter()
            .all(Option::is_none)
    );
}
#[test]
fn refuses_wrong_types_bounds_packets_names_and_unreadable_graphs() {
    for (at, value) in [
        (ITEM, 0),
        (ITEM + 0x30, OPS + 4),
        (ITEM + 0x4c, 0),
        (ITEM + 8, DATA + 1),
        (ITEM + 8, u32::MAX - 3),
        (ITEM + 0x48, 0),
        (ITEM + 0x48, 17 * 1024 * 1024),
        (DATA + 4, 65),
        (DATA + 0x20, DATA - 4),
        (DATA + 0x30, DATA + 0x400),
        (DATA + 8, 0),
        (INFO, 0),
        (MODEL, 0x88),
        (MODEL + 0x4c, 0),
        (RED + 8, 0),
        (DATA + 0x204, ITEM + 4),
        (DATA + 0x21c, 0),
        (DATA + 0x210, 0),
    ] {
        let mut b = own();
        put(&mut b, at, value);
        assert!(
            capture(layout(), OPS, |at, out| read(&b, at, out)).is_err(),
            "field {at:x} value {value:x}"
        );
    }
    for name in [
        b"../red.shd".as_slice(),
        b"/red.shd",
        b"a//red.shd",
        b"a\\red.shd",
        b"a:bad.shd",
        b"a/./red.shd",
        b"a/red.psm",
        b"",
        &[b'x'; 64],
    ] {
        let mut b = own();
        text(&mut b, RED + 0x68, name);
        assert!(capture(layout(), OPS, |at, out| read(&b, at, out)).is_err());
    }
}
#[test]
fn observed_changes_of_material_identity_or_actor_refuse_the_capture() {
    for changed in [
        RED + 0x68,
        MODEL + 4,
        CAR + FIELDS[2],
        DATA + 0x100,
        CAR + 0x84,
    ] {
        let mut b = own();
        let mut visits = 0;
        let result = capture(layout(), OPS, |at, out| {
            read(&b, at, out)?;
            if at == RED + 0x68 {
                visits += 1;
                if visits == 1 {
                    b[changed as usize] ^= 4;
                }
            }
            Ok(())
        });
        assert_eq!(result, Err(Unavailable::Changed), "field {changed:x}");
    }
}
#[test]
fn mutated_memory_stays_bounded_and_returned_reports_always_validate() {
    let original = own();
    let mut seed = 741u64;
    for _ in 0..3000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut b = original.clone();
        let at = (seed as usize) % b.len();
        b[at] ^= (seed >> 32) as u8 | 1;
        let (mut calls, mut bytes) = (0, 0);
        if let Ok(v) = capture(layout(), OPS, |at, out| {
            calls += 1;
            bytes += out.len();
            read(&b, at, out)
        }) {
            v.validate().unwrap();
            assert!(serde_json::to_vec_pretty(&v).unwrap().len() <= MAX_JSON);
        }
        assert!(calls <= MAX_READ_CALLS && bytes <= MAX_READ_BYTES);
    }
}
#[test]
fn all_three_fields_at_the_material_limit_fit_the_read_budget_and_keep_duplicates() {
    let mut b = own();
    for offset in FIELDS {
        put(&mut b, CAR + offset, ITEM);
    }
    put(&mut b, ITEM + 0x48, 0x2000);
    put(&mut b, DATA + 4, 64);
    put(&mut b, DATA + 0x30, DATA + 0x200);
    for i in 0..64 {
        put(&mut b, DATA + 0x100 + i * 4, RED);
        let packet = DATA + 0x400 + i * 16;
        put(&mut b, DATA + 0x200 + i * 4, packet);
        put(&mut b, packet, 1);
        put(&mut b, packet + 4, ITEM);
        put(&mut b, packet + 12, i);
    }
    let (mut calls, mut bytes) = (0, 0);
    let v = capture(layout(), OPS, |at, out| {
        calls += 1;
        bytes += out.len();
        read(&b, at, out)
    })
    .unwrap();
    assert!(
        v.instances
            .iter()
            .flatten()
            .all(|i| i.material_keys.len() == 64
                && i.material_keys.iter().all(|k| k == &i.material_keys[0]))
    );
    assert!(calls <= MAX_READ_CALLS && bytes <= MAX_READ_BYTES);
}
#[test]
fn contract_rejects_false_claims_unknown_fields_and_excessive_materials() {
    let b = own();
    let v = capture(layout(), OPS, |at, out| read(&b, at, out)).unwrap();
    for field in ["complete_assembly", "engine_verified", "world_synchronized"] {
        let mut j = serde_json::to_value(&v).unwrap();
        j[field] = true.into();
        assert!(ResourceObservation::parse(&serde_json::to_vec(&j).unwrap()).is_err());
    }
    let mut j = serde_json::to_value(&v).unwrap();
    j["pointer"] = 7.into();
    assert!(ResourceObservation::parse(&serde_json::to_vec(&j).unwrap()).is_err());
    let mut many = v;
    many.instances[2].as_mut().unwrap().material_keys = vec!["a".repeat(64); 65];
    assert!(many.validate().is_err());
    assert!(ResourceObservation::parse(&vec![b' '; MAX_JSON + 1]).is_err());
}

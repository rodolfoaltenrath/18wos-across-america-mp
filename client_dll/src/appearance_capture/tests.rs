use super::*;
const CAR: u32 = 0x2000;
fn put(b: &mut [u8], at: u32, value: u32) {
    b[at as usize..at as usize + 4].copy_from_slice(&value.to_le_bytes());
}
fn own() -> Vec<u8> {
    let mut b = vec![0; 0xc000];
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
    ] {
        put(&mut b, at, value);
    }
    let make = (CAR + profile::APPEARANCE_MAKE) as usize;
    b[make..make + 3].copy_from_slice(b"own");
    b[(CAR + profile::APPEARANCE_COUNT) as usize] = 3;
    for (i, name) in [b"own_wheels".as_slice(), b"own_engine", b"paint_red"]
        .iter()
        .enumerate()
    {
        let at = (CAR + profile::APPEARANCE_NAMES) as usize + i * profile::APPEARANCE_STRIDE;
        b[at..at + name.len()].copy_from_slice(name);
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
    let end = start.checked_add(out.len()).ok_or(())?;
    out.copy_from_slice(b.get(start..end).ok_or(())?);
    Ok(())
}
#[test]
fn stable_candidate_names_become_only_hashed_definition_keys_and_keep_table_order() {
    let b = own();
    let result = capture(layout(), |at, out| read(&b, at, out)).unwrap();
    result.validate().unwrap();
    assert_eq!(
        result.make_definition_key,
        career::hash(b"makes/truck/own/description.def")
    );
    assert_eq!(
        result.slots[2].definition_key.as_deref(),
        Some(career::hash(b"makes/truck/own/acc/paint_red/!desc.def").as_str())
    );
    assert_eq!(result.slots.len(), 3);
    assert!(result.candidate_only && !result.engine_verified && !result.world_synchronized);
    let json = serde_json::to_vec(&result).unwrap();
    assert_eq!(Observation::parse(&json).unwrap(), result);
    let text = String::from_utf8(json).unwrap();
    for raw in [
        "paint_red",
        "own_wheels",
        "position",
        "pointer",
        "vtable",
        "root",
    ] {
        assert!(!text.contains(raw));
    }
    let mut upper = b;
    let at = (CAR + profile::APPEARANCE_MAKE) as usize;
    upper[at..at + 3].copy_from_slice(b"OWN");
    assert_eq!(
        capture(layout(), |at, out| read(&upper, at, out)).unwrap(),
        result
    );
}
#[test]
fn missing_types_limits_and_unterminated_or_unsafe_names_never_produce_a_guess() {
    for (at, bytes, expected) in [
        (0x100, vec![0; 4], Unavailable::Missing),
        (CAR, 7u32.to_le_bytes().to_vec(), Unavailable::Type),
        (0x6048, 7u32.to_le_bytes().to_vec(), Unavailable::Type),
        (0x6034, 7u32.to_le_bytes().to_vec(), Unavailable::Type),
        (
            CAR + profile::APPEARANCE_COUNT,
            vec![65],
            Unavailable::Invalid,
        ),
        (
            CAR + profile::APPEARANCE_MAKE,
            vec![b'x'; 32],
            Unavailable::Invalid,
        ),
        (
            CAR + profile::APPEARANCE_MAKE,
            b"../x\0".to_vec(),
            Unavailable::Invalid,
        ),
        (
            CAR + profile::APPEARANCE_MAKE,
            vec![0],
            Unavailable::Missing,
        ),
        (
            CAR + profile::APPEARANCE_NAMES,
            vec![b'x'; 20],
            Unavailable::Invalid,
        ),
    ] {
        let mut b = own();
        b[at as usize..at as usize + bytes.len()].copy_from_slice(&bytes);
        assert_eq!(
            capture(layout(), |at, out| read(&b, at, out)),
            Err(expected)
        );
    }
    let b = own();
    let mut overflow = layout();
    overflow.actor.root = u32::MAX - 1;
    assert!(capture(overflow, |at, out| read(&b, at, out)).is_err());
}
#[test]
fn a_changed_name_count_chain_scene_or_owner_discards_the_entire_attempt() {
    for change in [
        CAR + profile::APPEARANCE_MAKE,
        CAR + profile::APPEARANCE_COUNT,
        0x204,
        CAR + 0x84,
        0x6008,
    ] {
        let mut b = own();
        let mut copies = 0;
        assert_eq!(
            capture(layout(), |at, out| {
                if at == CAR + profile::APPEARANCE_MAKE {
                    copies += 1;
                    if copies == 2 {
                        b[change as usize] ^= 1;
                    }
                }
                read(&b, at, out)
            }),
            Err(Unavailable::Changed)
        );
    }
}
#[test]
fn empty_entries_remain_missing_and_unknown_claims_or_slot_orders_are_refused() {
    let mut b = own();
    b[(CAR + profile::APPEARANCE_NAMES) as usize] = 0;
    let observation = capture(layout(), |at, out| read(&b, at, out)).unwrap();
    assert!(observation.slots[0].definition_key.is_none());
    let mut bad = observation.clone();
    bad.slots[0].index = 1;
    assert!(bad.validate().is_err());
    bad = observation.clone();
    bad.engine_verified = true;
    assert!(bad.validate().is_err());
    let mut value = serde_json::to_value(&observation).unwrap();
    value["raw_make"] = "own".into();
    assert!(Observation::parse(&serde_json::to_vec(&value).unwrap()).is_err());
}
#[test]
fn owned_memory_mutations_have_a_fixed_read_budget_and_never_emit_invalid_observations() {
    let seed = own();
    let mut rng = multiplayer_server::fuzz::Rng::new(981);
    for _ in 0..3000 {
        let b = multiplayer_server::fuzz::mutate(&mut rng, &seed);
        let (mut calls, mut bytes) = (0, 0);
        let result = capture(layout(), |at, out| {
            calls += 1;
            bytes += out.len();
            assert!(calls <= 80 && bytes <= 4096);
            read(&b, at, out)
        });
        if let Ok(o) = result {
            o.validate().unwrap();
            assert_eq!(
                Observation::parse(&serde_json::to_vec(&o).unwrap()).unwrap(),
                o
            );
        }
    }
}

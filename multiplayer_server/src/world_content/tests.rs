use super::*;
pub fn fixture() -> Manifest {
    Manifest {
        schema: 1,
        profile: crate::session_protocol::PROFILE.into(),
        layers: vec![
            layer(Role::Map, b"map", b"original"),
            layer(Role::Mod, b"red", b"red"),
            layer(Role::Mod, b"blue", b"blue"),
        ],
    }
}
fn layer(role: Role, archive: &[u8], paint: &[u8]) -> Layer {
    Layer {
        role,
        sha256: career::hash(archive),
        bytes: 200,
        resources: vec![Resource {
            key: career::hash(b"paint"),
            sha256: career::hash(paint),
            bytes: paint.len() as u64,
        }],
    }
}
#[test]
fn order_is_not_a_set_and_changes_the_paint_winner() {
    let host = fixture();
    let mut peer = host.clone();
    peer.layers.swap(1, 2);
    assert_eq!(host.legacy().unwrap(), peer.legacy().unwrap());
    let c = host.compare(&peer).unwrap();
    assert!(!c.content_equal && !c.ordered_archives_equal && !c.effective_resources_equal);
    assert_eq!(c.differing_resources, 1);
    assert!(!c.engine_verified && !c.world_synchronized);
    let winner = host
        .resolve()
        .unwrap()
        .remove(&career::hash(b"paint"))
        .unwrap();
    assert_eq!(winner.sha256, career::hash(b"blue"));
    assert_eq!(winner.shadowed, 2);
    assert_ne!(host.fingerprint().unwrap(), peer.fingerprint().unwrap());
}
#[test]
fn strict_comparison_refuses_different_archives_even_with_same_winner() {
    let a = fixture();
    let mut b = a.clone();
    b.layers[0].sha256 = career::hash(b"changed hidden resource");
    let c = a.compare(&b).unwrap();
    assert!(c.effective_resources_equal && !c.content_equal);
    b = a.clone();
    b.profile.push('x');
    assert!(!a.compare(&b).unwrap().profile_equal);
    assert!(a.compare(&a).unwrap().content_equal);
}
#[test]
fn resource_add_remove_and_report_are_bounded_without_paths() {
    let a = fixture();
    let mut b = a.clone();
    for i in 0..32 {
        b.layers[0].resources.push(Resource {
            key: career::hash(&[i]),
            sha256: career::hash(b"owned"),
            bytes: 5,
        });
    }
    b.layers[0].resources.sort_by(|a, b| a.key.cmp(&b.key));
    let c = a.compare(&b).unwrap();
    assert_eq!(c.differing_resources, 32);
    assert_eq!(c.differing_keys.len(), 16);
    assert!(c.differing_keys_truncated);
    assert!(!serde_json::to_string(&c).unwrap().contains("paint"));
}
#[test]
fn malformed_ambiguous_or_excessive_inventory_is_refused() {
    let a = fixture();
    let mut b = a.clone();
    b.schema = 2;
    assert!(b.validate().is_err());
    b = a.clone();
    b.layers[0].role = Role::Base;
    assert!(b.validate().is_err());
    b = a.clone();
    b.layers[1].role = Role::Map;
    assert!(b.validate().is_err());
    b = a.clone();
    b.layers.push(b.layers[0].clone());
    assert!(b.validate().is_err());
    b = a.clone();
    let duplicate = b.layers[0].resources[0].clone();
    b.layers[0].resources.push(duplicate);
    assert!(b.validate().is_err());
    b = a.clone();
    b.layers[0].resources[0].bytes = u64::MAX;
    assert!(b.validate().is_err());
    b = a.clone();
    b.layers[0].resources[0].key = "../../private".into();
    assert!(b.validate().is_err());
    let mut json = serde_json::to_value(a).unwrap();
    json["engine_verified"] = true.into();
    assert!(Manifest::parse(&serde_json::to_vec(&json).unwrap()).is_err());
    assert!(Manifest::parse(&vec![b' '; MAX_JSON + 1]).is_err());
}

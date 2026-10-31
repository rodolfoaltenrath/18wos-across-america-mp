//! Deterministic robustness of the asset/localization parsers that read local
//! game files: own fixtures as seeds, mutated inputs, no panic, bounded time.
use crate::{localization, pe32};
use multiplayer_server::fuzz::{Rng, mutate};
use std::{
    panic::{self, AssertUnwindSafe},
    time::{Duration, Instant},
};

fn pe_fixture() -> Vec<u8> {
    // Same own two-section layout as the pe32 unit fixture.
    let mut b = vec![0; 0x600];
    b[..2].copy_from_slice(b"MZ");
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (at, val) in [(0x84, 0x14cu16), (0x86, 2), (0x94, 224), (0x98, 0x10b)] {
        b[at..at + 2].copy_from_slice(&val.to_le_bytes());
    }
    for (at, val) in [
        (60, 0x80u32),
        (0xb4, 0x10000000),
        (0xd0, 0x3000),
        (0xd4, 0x200),
        (0xf4, 16),
        (0x180, 0x100),
        (0x184, 0x1000),
        (0x188, 0x200),
        (0x18c, 0x200),
        (0x19c, 0x60000020),
        (0x1a8, 0x200),
        (0x1ac, 0x2000),
        (0x1b0, 0x200),
        (0x1b4, 0x400),
        (0x1c4, 0x40000040),
    ] {
        b[at..at + 4].copy_from_slice(&val.to_le_bytes());
    }
    b[0x178..0x17d].copy_from_slice(b".text");
    b[0x1a0..0x1a6].copy_from_slice(b".rdata");
    b
}
fn tga() -> Vec<u8> {
    // Uncompressed 32-bit 2x2, top-left origin.
    let mut b = vec![0u8; 18];
    b[2] = 2;
    b[12..14].copy_from_slice(&2u16.to_le_bytes());
    b[14..16].copy_from_slice(&2u16.to_le_bytes());
    b[16] = 32;
    b[17] = 0x28;
    b.extend(std::iter::repeat_n(0xff, 16));
    b
}
type Parser = fn(&[u8]) -> bool;
fn targets() -> Vec<(&'static str, Vec<Vec<u8>>, Parser)> {
    vec![
        (
            "font",
            vec![b"vert_span:12\r\nline_spacing:2\r\nimage:/fonts/own.shd\r\n$065,1,2,3,4,0,1,5\r\nkern: $065, $065, -1\r\n".to_vec()],
            |b| localization::font::parse(b).is_ok(),
        ),
        (
            "def",
            vec![b"# own fixture\r\ndef translate {\r\n__fnt:\"face=own.font\"\r\nown_a:\"Own \\\\ytext\" # c\r\n}\r\n".to_vec()],
            |b| {
                let _ = localization::def::decode(b);
                localization::def::parse(b).is_ok()
            },
        ),
        ("codec", vec![b"3nK\x01\x00\x58\x43".to_vec()], |b| {
            localization::codec::decode(b).is_ok()
        }),
        ("tga", vec![tga()], |b| localization::glyphs::read_tga(b).is_ok()),
        (
            "pml",
            vec![b"<align hstyle=%1><font @@__fnt_own@@>\\\\y@@own@@ %0 Own<ret></font></align>".to_vec()],
            |b| localization::pml::analyze(b).is_ok(),
        ),
        (
            "catalog",
            vec![br#"{"schema":1,"language":"pt-BR","source":"scripts/local.def","entries":{"k":{"src":"0","text":"t"}}}"#.to_vec()],
            |b| localization::catalog::load(b).is_ok(),
        ),
        (
            "literals",
            vec![b"# own\r\ndef own {\r\n name: \"Own lamp\" # retain\r\n price:123\r\n}\r\n".to_vec()],
            |b| {
                let _ = localization::literals::load(b);
                localization::literals::parse("def/own.sii", b, true).is_ok()
            },
        ),
        ("pe32", vec![pe_fixture()], |b| pe32::Pe32::parse(b.to_vec()).is_ok()),
    ]
}
#[test]
fn asset_parsers_never_panic_or_stall_on_mutated_inputs() {
    let mut failures = Vec::new();
    let mut never_accepted = Vec::new();
    for (seed_value, iterations) in [(21u64, 3000), (22, 3000)] {
        let mut rng = Rng::new(seed_value);
        for (name, seeds, parse) in targets() {
            let (mut accepted, mut slowest) = (0, Duration::ZERO);
            for i in 0..iterations {
                let input = match seeds.get(i) {
                    Some(s) => s.clone(),
                    None => {
                        let pick = rng.below(seeds.len());
                        mutate(&mut rng, &seeds[pick])
                    }
                };
                let started = Instant::now();
                match panic::catch_unwind(AssertUnwindSafe(|| parse(&input))) {
                    Ok(true) => accepted += 1,
                    Ok(false) => {}
                    Err(_) => {
                        failures.push((name, input.iter().take(64).copied().collect::<Vec<_>>()))
                    }
                }
                slowest = slowest.max(started.elapsed());
            }
            assert!(slowest < Duration::from_secs(2), "{name}: {slowest:?}");
            if accepted == 0 {
                never_accepted.push(name);
            }
        }
    }
    assert!(failures.is_empty(), "panics: {failures:?}");
    assert!(
        never_accepted.is_empty(),
        "seed never accepted: {never_accepted:?}"
    );
}

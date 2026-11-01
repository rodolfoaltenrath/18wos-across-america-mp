use super::*;
use std::io::Write;
use zip::{ZipWriter, write::SimpleFileOptions};
fn archive(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}
#[test]
fn actual_archives_resolve_priority_without_extraction_or_path_leaks() {
    let t = tempfile::tempdir().unwrap();
    let map = t.path().join("map.scs");
    let a = t.path().join("red.scs");
    let b = t.path().join("blue.scs");
    archive(
        &map,
        &[
            ("map/own.txt", b"own map"),
            ("paint/color.txt", b"original"),
        ],
    );
    archive(&a, &[("paint/color.txt", b"red")]);
    archive(&b, &[("PAINT/color.txt", b"blue")]);
    let inputs = vec![
        Input {
            role: contract::Role::Map,
            path: map,
        },
        Input {
            role: contract::Role::Mod,
            path: a,
        },
        Input {
            role: contract::Role::Mod,
            path: b,
        },
    ];
    let output = t.path().join("manifest.json");
    let summary = create(
        multiplayer_server::session_protocol::PROFILE,
        &inputs,
        &output,
    )
    .unwrap();
    assert_eq!(
        (
            summary.resources,
            summary.effective_resources,
            summary.overridden_resources
        ),
        (4, 2, 1)
    );
    assert!(!summary.engine_verified && !summary.world_synchronized);
    assert!(
        create(
            multiplayer_server::session_protocol::PROFILE,
            &inputs,
            &output
        )
        .is_err()
    );
    let bytes = fs::read(&output).unwrap();
    assert!(!String::from_utf8(bytes).unwrap().contains("color.txt"));
    let a = load(&output).unwrap();
    let mut reversed = inputs;
    reversed.swap(1, 2);
    let b = inventory(&a.profile, &reversed).unwrap();
    assert_eq!(a.legacy().unwrap(), b.legacy().unwrap());
    assert_eq!(a.compare(&b).unwrap().differing_resources, 1);
    assert_eq!(fs::read_dir(t.path()).unwrap().count(), 4);
}
#[test]
fn archive_aliases_links_paths_duplicates_and_limits_are_refused_before_output() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("bad.scs");
    for entries in [
        vec![("../escape.txt", b"x".as_slice())],
        vec![("a.txt", b"x".as_slice()), ("A.txt", b"y".as_slice())],
        vec![("NUL.txt", b"x".as_slice())],
        vec![("dir/x:stream", b"x".as_slice())],
    ] {
        archive(&path, &entries);
        assert!(
            create(
                multiplayer_server::session_protocol::PROFILE,
                &[Input {
                    role: contract::Role::Map,
                    path: path.clone()
                }],
                &t.path().join("out.json")
            )
            .is_err()
        );
        assert!(!t.path().join("out.json").exists());
    }
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.add_symlink("link", "target", SimpleFileOptions::default())
        .unwrap();
    zip.finish().unwrap();
    assert!(
        inventory(
            multiplayer_server::session_protocol::PROFILE,
            &[Input {
                role: contract::Role::Map,
                path: path.clone()
            }]
        )
        .is_err()
    );
    for name in ["", "a//b", "x/..", "foo.", "x\\y", "á.txt"] {
        assert!(resource_key(name).is_err());
    }
    assert!("map=".parse::<Input>().is_err());
    assert!("unknown=file".parse::<Input>().is_err());
    assert!(inventory("bad profile", &[]).is_err());
}
#[test]
fn compressed_oversized_entry_is_refused_without_expansion_or_output() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("oversized.scs");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.start_file(
        "own.txt",
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
    )
    .unwrap();
    let block = [0u8; 64 * 1024];
    for _ in 0..contract::MAX_RESOURCE / block.len() as u64 {
        zip.write_all(&block).unwrap();
    }
    zip.write_all(b"x").unwrap();
    zip.finish().unwrap();
    let output = t.path().join("out.json");
    let error = create(
        multiplayer_server::session_protocol::PROFILE,
        &[Input {
            role: contract::Role::Map,
            path,
        }],
        &output,
    )
    .unwrap_err();
    assert!(error.to_string().contains("size/type budget"));
    assert!(!output.exists());
}
#[cfg(unix)]
#[test]
fn final_symlink_cannot_be_used_as_archive_or_manifest() {
    let t = tempfile::tempdir().unwrap();
    let real = t.path().join("real");
    let link = t.path().join("link");
    archive(&real, &[("own.txt", b"own")]);
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert!(regular(&link).is_err());
    assert!(load(&link).is_err());
}

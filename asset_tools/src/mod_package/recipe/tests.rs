use super::*;
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    project: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        example(&project).unwrap();
        Self { temp, project }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.temp.path().join(name)
    }
    fn archive(&self) -> PathBuf {
        self.project.join("inputs/local/example-original.zip")
    }
    fn edit(&self, edit: impl FnOnce(&mut Recipe)) {
        let path = self.project.join("recipe.json");
        let mut recipe: Recipe = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        edit(&mut recipe);
        fs::write(path, serde_json::to_vec_pretty(&recipe).unwrap()).unwrap();
    }
    fn run(&self, name: &str) -> Result<Report> {
        build(
            &self.project.join("recipe.json"),
            &self.path(name),
            &self.path("cache"),
            &[self.archive()],
            false,
        )
    }
    fn owned(&self) {
        self.edit(|r| {
            r.files
                .retain(|f| matches!(f.source, Source::OwnedFile { .. }))
        });
    }
    fn png(&self, package: &str, entry: &str) -> image::RgbaImage {
        let bytes = fs::read(self.path(package).join(PAYLOAD)).unwrap();
        let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut bytes = Vec::new();
        zip.by_name(entry).unwrap().read_to_end(&mut bytes).unwrap();
        image::load_from_memory(&bytes).unwrap().into_rgba8()
    }
}

#[test]
fn cold_warm_and_independent_roots_have_equal_packages_and_pinned_lineage() {
    let f = Fixture::new();
    let g = Fixture::new();
    let first = f.run("cold").unwrap();
    assert_eq!((first.cache_hits, first.cache_misses), (0, 2));
    let warm = f.run("warm").unwrap();
    assert_eq!((warm.cache_hits, warm.cache_misses), (2, 0));
    assert!(f.run("warm").unwrap().build.reused);
    g.edit(|r| r.files.reverse());
    g.run("other").unwrap();
    assert_eq!(
        contents(&f.path("cold")).unwrap(),
        contents(&f.path("warm")).unwrap()
    );
    assert_eq!(
        contents(&f.path("cold")).unwrap(),
        contents(&g.path("other")).unwrap()
    );
    let manifest = verify(&f.path("cold"), false).unwrap();
    assert_eq!(manifest.schema, 2);
    assert!(!manifest.distributable);
    assert!(manifest.files.iter().all(|f| f.derivation.is_some()));
    assert!(
        !String::from_utf8(fs::read(f.path("cold").join(MANIFEST)).unwrap())
            .unwrap()
            .contains(&f.temp.path().to_string_lossy().to_string())
    );
    // The previous project's schema and canonical serialization remain usable.
    super::super::build(&f.project.join("project.json"), &f.path("legacy"), true).unwrap();
    assert_eq!(verify(&f.path("legacy"), true).unwrap().schema, 1);
}

#[test]
fn png_flip_preserves_rgba_pixels_and_resize_preserves_aspect() {
    let f = Fixture::new();
    f.run("flipped").unwrap();
    let image = f.png("flipped", "textures/mp/own-flipped.png");
    assert_eq!(image.dimensions(), (2, 2));
    assert_eq!(image.get_pixel(0, 0).0, [0, 128, 255, 64]);
    assert_eq!(image.get_pixel(0, 1).0, [255, 128, 0, 255]);
    f.edit(|r| {
        r.files[0].transform = Transform::Png {
            flip_y: false,
            max_edge: Some(1),
        }
    });
    let resized = f.run("resized").unwrap();
    assert_eq!((resized.cache_hits, resized.cache_misses), (1, 1));
    assert_eq!(
        f.png("resized", "textures/mp/own-flipped.png").dimensions(),
        (1, 1)
    );
}

#[test]
fn tga_bmp_and_sixteen_bit_png_preserve_decoded_pixels() {
    let rgba = image::RgbaImage::from_fn(4, 2, |x, y| {
        image::Rgba([
            (x * 61) as u8,
            (y * 127) as u8,
            93,
            if x == y { 64 } else { 255 },
        ])
    });
    let rgba16 = image::ImageBuffer::from_fn(2, 3, |x, y| {
        image::Rgba([(x * 29003) as u16, (y * 17001) as u16, 50003, 32769])
    });
    for (extension, format, image) in [
        (
            "tga",
            image::ImageFormat::Tga,
            image::DynamicImage::ImageRgba8(rgba.clone()),
        ),
        (
            "bmp",
            image::ImageFormat::Bmp,
            image::DynamicImage::ImageRgba8(rgba),
        ),
        (
            "png",
            image::ImageFormat::Png,
            image::DynamicImage::ImageRgba16(rgba16),
        ),
    ] {
        let f = Fixture::new();
        f.owned();
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, format).unwrap();
        let source = format!("inputs/owned/source.{extension}");
        fs::write(f.project.join(&source), encoded.get_ref()).unwrap();
        f.edit(|r| {
            r.files[0].source = Source::OwnedFile {
                path: source,
                sha256: hash(encoded.get_ref()),
            };
            r.files[0].transform = Transform::Png {
                flip_y: false,
                max_edge: None,
            };
        });
        build(
            &f.project.join("recipe.json"),
            &f.path("output"),
            &f.path("cache"),
            &[],
            true,
        )
        .unwrap();
        let payload = fs::read(f.path("output").join(PAYLOAD)).unwrap();
        let mut zip = ZipArchive::new(Cursor::new(payload)).unwrap();
        let mut png = Vec::new();
        zip.by_name("textures/mp/own-flipped.png")
            .unwrap()
            .read_to_end(&mut png)
            .unwrap();
        let expected = image::load_from_memory_with_format(encoded.get_ref(), format)
            .unwrap()
            .to_rgba16();
        let actual = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .unwrap()
            .to_rgba16();
        assert_eq!(actual, expected, "{extension} pixels/alpha changed");
        if extension == "png" {
            assert_eq!(actual, image.to_rgba16(), "16-bit precision lost");
        }
    }
}

#[test]
fn cache_hits_never_bypass_missing_or_modified_original_inputs() {
    let f = Fixture::new();
    f.run("cold").unwrap();
    fs::write(f.project.join("inputs/owned/checker.png"), b"changed").unwrap();
    assert!(f.run("changed").is_err());
    assert!(!f.path("changed").exists());
    let f = Fixture::new();
    f.run("cold").unwrap();
    fs::remove_file(f.archive()).unwrap();
    assert!(f.run("missing").is_err());
    assert!(!f.path("missing").exists());
}

#[test]
fn cache_integrity_failure_refuses_output_and_does_not_repair_silently() {
    let f = Fixture::new();
    f.run("cold").unwrap();
    let path = fs::read_dir(f.path("cache"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut bytes = fs::read(&path).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&path, &bytes).unwrap();
    assert!(f.run("corrupt").is_err());
    assert!(!f.path("corrupt").exists());
    assert_eq!(fs::read(path).unwrap(), bytes);
    verify(&f.path("cold"), false).unwrap();
}

#[test]
fn cached_owned_bytes_cannot_launder_local_archive_origin() {
    let f = Fixture::new();
    f.edit(|r| {
        r.files
            .retain(|f| matches!(f.source, Source::OwnedFile { .. }));
        r.files[0].transform = Transform::Copy;
    });
    let own = build(
        &f.project.join("recipe.json"),
        &f.path("own"),
        &f.path("cache"),
        &[],
        true,
    )
    .unwrap();
    assert!(own.build.manifest.distributable);
    f.edit(|r| {
        let sha = r.files[0].source.sha256().to_owned();
        r.files[0].source = Source::LocalArchive {
            archive_sha256: fingerprint(&f.archive(), None).unwrap().file_sha256,
            entry: "textures/checker.png".into(),
            sha256: sha,
        };
    });
    assert!(
        build(
            &f.project.join("recipe.json"),
            &f.path("export"),
            &f.path("cache"),
            &[f.archive()],
            true
        )
        .is_err()
    );
    assert!(!f.path("export").exists());
    let local = f.run("local").unwrap();
    assert_eq!((local.cache_hits, local.cache_misses), (1, 0));
    assert!(!local.build.manifest.distributable);
    assert_eq!(
        local.build.manifest.files[0].input.origin,
        Origin::DerivedLocal
    );
    assert!(verify(&f.path("local"), true).is_err());
}

#[test]
fn whole_archive_pin_entry_pin_and_case_are_checked_independently() {
    let f = Fixture::new();
    let mut payload = fs::read(f.archive()).unwrap();
    payload.push(0);
    fs::write(f.archive(), payload).unwrap();
    assert!(f.run("changed-archive").is_err());
    assert!(!f.path("cache").exists());
    let f = Fixture::new();
    f.edit(|r| {
        if let Source::LocalArchive { sha256, .. } = &mut r.files[1].source {
            *sha256 = "0".repeat(64);
        }
    });
    assert!(f.run("wrong-entry").is_err());
    let f = Fixture::new();
    f.edit(|r| {
        if let Source::LocalArchive { entry, .. } = &mut r.files[1].source {
            *entry = "Textures/checker.png".into();
        }
    });
    assert!(f.run("wrong-case").is_err());
}

#[test]
fn fingerprints_do_not_extract_and_originals_are_never_modified() {
    let f = Fixture::new();
    assert!(fingerprint(&f.project, None).is_err());
    let archive = fs::read(f.archive()).unwrap();
    let pin = fingerprint(&f.archive(), Some("textures/checker.png")).unwrap();
    assert_eq!(pin.file_sha256, hash(&archive));
    let entry = pin.entry.unwrap();
    assert_eq!(
        entry.sha256,
        hash(&fs::read(f.project.join("inputs/owned/checker.png")).unwrap())
    );
    assert_eq!(entry.bytes, 86);
    f.run("package").unwrap();
    assert_eq!(fs::read(f.archive()).unwrap(), archive);
    assert!(!f.project.join("textures").exists());
}

#[cfg(unix)]
#[test]
fn fifo_original_is_rejected_before_a_blocking_open() {
    let temp = tempfile::tempdir().unwrap();
    let fifo = temp.path().join("original.fifo");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("Unix fixture requires mkfifo");
    assert!(status.success());
    assert!(fingerprint(&fifo, None).is_err());
}

#[test]
fn duplicate_unreferenced_and_missing_archive_bindings_fail_before_cache() {
    let f = Fixture::new();
    for bindings in [vec![], vec![f.archive(), f.archive()]] {
        assert!(
            build(
                &f.project.join("recipe.json"),
                &f.path("package"),
                &f.path("cache"),
                &bindings,
                false
            )
            .is_err()
        );
        assert!(!f.path("cache").exists());
    }
    f.owned();
    assert!(f.run("unreferenced").is_err());
    assert!(!f.path("cache").exists());
}

#[test]
fn malformed_recipe_graph_and_transform_fail_before_outputs() {
    for limit in [0, 8193] {
        let f = Fixture::new();
        f.edit(|r| {
            r.files[0].transform = Transform::Png {
                flip_y: false,
                max_edge: Some(limit),
            }
        });
        assert!(f.run("invalid").is_err());
        assert!(!f.path("cache").exists());
    }
    let f = Fixture::new();
    f.edit(|r| r.files[0].depends_on = vec!["missing.png".into()]);
    assert!(f.run("missing-dependency").is_err());
    assert!(!f.path("cache").exists());
    let f = Fixture::new();
    f.edit(|r| {
        r.files[0].source = Source::OwnedFile {
            path: "inputs/local/checker.png".into(),
            sha256: "0".repeat(64),
        }
    });
    assert!(f.run("wrong-origin-folder").is_err());
    let f = Fixture::new();
    f.edit(|r| {
        if let Source::LocalArchive { entry, .. } = &mut r.files[1].source {
            *entry = "../outside.png".into();
        }
    });
    assert!(f.run("traversal").is_err());
    assert!(!f.path("cache").exists());
}

#[test]
fn cache_inputs_and_output_cannot_share_a_tree() {
    let f = Fixture::new();
    for (output, cache) in [
        (f.path("same"), f.path("same")),
        (f.path("output"), f.project.join("inputs")),
        (f.project.join("inputs/owned/output"), f.path("cache")),
    ] {
        assert!(
            build(
                &f.project.join("recipe.json"),
                &output,
                &cache,
                &[f.archive()],
                false
            )
            .is_err()
        );
    }
    #[cfg(windows)]
    assert!(
        build(
            &f.project.join("recipe.json"),
            &f.path("output"),
            &f.project.join("INPUTS"),
            &[f.archive()],
            false
        )
        .is_err()
    );
    assert!(!f.path("output").exists());
}

#[test]
fn schema_and_lineage_cannot_be_removed_or_relabelled_in_a_verified_package() {
    let f = Fixture::new();
    f.run("package").unwrap();
    let path = f.path("package").join(MANIFEST);
    let original = fs::read(&path).unwrap();
    for variant in 0..3 {
        let mut manifest: Manifest = serde_json::from_slice(&original).unwrap();
        match variant {
            0 => manifest.schema = 1,
            1 => manifest.files[0].derivation = None,
            _ => {
                manifest.files[0].input.origin = Origin::Owned;
                manifest.files[0].input.source = "inputs/owned/fake.png".into();
                manifest.distributable = true;
            }
        }
        fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
        assert!(verify(&f.path("package"), false).is_err());
    }
}

#[test]
fn local_file_origin_and_png_limits_are_enforced() {
    let f = Fixture::new();
    let source = f.project.join("inputs/local/edited.png");
    fs::copy(f.project.join("inputs/owned/checker.png"), &source).unwrap();
    f.edit(|r| {
        r.files.truncate(1);
        r.files[0].source = Source::LocalFile {
            path: "inputs/local/edited.png".into(),
            sha256: fingerprint(&source, None).unwrap().file_sha256,
        };
    });
    let local = build(
        &f.project.join("recipe.json"),
        &f.path("local"),
        &f.path("cache"),
        &[],
        false,
    )
    .unwrap();
    assert!(!local.build.manifest.distributable);
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::new(8193, 1));
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    fs::write(&source, png.get_ref()).unwrap();
    f.edit(|r| {
        if let Source::LocalFile { sha256, .. } = &mut r.files[0].source {
            *sha256 = hash(png.get_ref());
        }
    });
    assert!(
        build(
            &f.project.join("recipe.json"),
            &f.path("too-wide"),
            &f.path("cache"),
            &[],
            false
        )
        .is_err()
    );
    assert!(!f.path("too-wide").exists());
}

#[test]
fn ambiguous_or_symlink_archive_entries_are_not_read_as_regular_assets() {
    for symlink in [false, true] {
        let f = Fixture::new();
        let png = fs::read(f.project.join("inputs/owned/checker.png")).unwrap();
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        if symlink {
            zip.add_symlink(
                "textures/checker.png",
                "outside",
                SimpleFileOptions::default(),
            )
            .unwrap();
        } else {
            for name in ["Textures/checker.png", "textures/checker.png"] {
                zip.start_file(name, SimpleFileOptions::default()).unwrap();
                zip.write_all(&png).unwrap();
            }
        }
        let payload = zip.finish().unwrap().into_inner();
        fs::write(f.archive(), &payload).unwrap();
        f.edit(|r| {
            if let Source::LocalArchive { archive_sha256, .. } = &mut r.files[1].source {
                *archive_sha256 = hash(&payload);
            }
        });
        assert!(f.run("invalid-archive").is_err());
        assert!(!f.path("invalid-archive").exists());
    }
}

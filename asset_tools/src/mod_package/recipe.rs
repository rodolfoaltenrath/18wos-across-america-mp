//! Hash-pinned local reconstruction. No original paths or original bytes enter
//! source bundles. Derived payloads stay local even after a cached conversion.
use super::*;
use std::io::{Seek, SeekFrom};

const MAX_ORIGINAL: u64 = 2 * 1024 * 1024 * 1024;
const MAX_BINDINGS: usize = 16;
const CACHE_HEADER: usize = 80;
const CACHE_MAGIC: &[u8; 8] = b"AAMPC01\0";
const REVISION: u8 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    OwnedFile {
        path: String,
        sha256: String,
    },
    LocalFile {
        path: String,
        sha256: String,
    },
    LocalArchive {
        archive_sha256: String,
        entry: String,
        sha256: String,
    },
}

impl Source {
    fn sha256(&self) -> &str {
        match self {
            Self::OwnedFile { sha256, .. }
            | Self::LocalFile { sha256, .. }
            | Self::LocalArchive { sha256, .. } => sha256,
        }
    }
    fn name(&self) -> &str {
        match self {
            Self::OwnedFile { path, .. } | Self::LocalFile { path, .. } => path,
            Self::LocalArchive { entry, .. } => entry,
        }
    }
    fn input(&self, path: String, depends_on: Vec<String>) -> Result<Input> {
        ensure!(
            valid_hash(self.sha256()),
            "source SHA-256 must be lowercase hex"
        );
        let (source, origin) = match self {
            Self::OwnedFile { path, .. } => (path.clone(), Origin::Owned),
            Self::LocalFile { path, .. } => (path.clone(), Origin::DerivedLocal),
            Self::LocalArchive {
                archive_sha256,
                entry,
                sha256,
            } => {
                ensure!(
                    valid_hash(archive_sha256),
                    "invalid original archive SHA-256"
                );
                relative(entry)?;
                (format!("inputs/local/{sha256}.asset"), Origin::DerivedLocal)
            }
        };
        Ok(Input {
            source,
            path,
            origin,
            depends_on,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Transform {
    Copy,
    Png { flip_y: bool, max_edge: Option<u32> },
}

impl Transform {
    fn validate(&self, source: &Source, output: &str) -> Result<()> {
        if let Self::Png { max_edge, .. } = self {
            ensure!(
                max_edge.is_none_or(|n| (1..=8192).contains(&n)),
                "PNG max_edge must be 1..=8192"
            );
            ensure!(
                super::super::extension(Path::new(output)) == "png",
                "PNG output must end in .png"
            );
            let ext = super::super::extension(Path::new(source.name()));
            ensure!(
                matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "tga" | "dds"),
                "unsupported texture source format"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeFile {
    pub source: Source,
    pub transform: Transform,
    pub path: String,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub schema: u8,
    pub id: String,
    pub version: String,
    pub files: Vec<RecipeFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Derivation {
    pub source: Source,
    pub transform: Transform,
    pub pipeline_revision: u8,
    pub dependency_lock_sha256: String,
}

impl Derivation {
    pub(super) fn validate(&self, input: &Input) -> Result<()> {
        ensure!(
            self.pipeline_revision == REVISION && valid_hash(&self.dependency_lock_sha256),
            "unsupported derivation revision/lock hash"
        );
        let expected = self
            .source
            .input(input.path.clone(), input.depends_on.clone())?;
        ensure!(
            input.source == expected.source && input.origin == expected.origin,
            "source/derivation origin mismatch"
        );
        self.transform.validate(&self.source, &input.path)
    }
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub build: BuildReport,
}

#[derive(Debug, Serialize)]
pub struct Fingerprint {
    pub file_sha256: String,
    pub entry: Option<EntryFingerprint>,
}

#[derive(Debug, Serialize)]
pub struct EntryFingerprint {
    pub path: String,
    pub bytes: usize,
    pub sha256: String,
}

/// Streams a bounded original without ever loading its full archive into RAM.
fn hashed_file(path: &Path) -> Result<(String, File)> {
    let path = checked(path)?;
    // Reject FIFOs/devices before opening: opening a FIFO can otherwise block
    // before the handle-level regular-file check below. Single writer required.
    let before = fs::metadata(&path)?;
    ensure!(
        before.is_file() && before.len() <= MAX_ORIGINAL,
        "original must be a regular file <=2 GiB"
    );
    let mut file = File::open(&path)?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file() && meta.len() <= MAX_ORIGINAL,
        "original must be a regular file <=2 GiB"
    );
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        ensure!(total <= MAX_ORIGINAL, "original exceeded 2 GiB during read");
        digest.update(&buffer[..n]);
    }
    ensure!(
        total == meta.len(),
        "original changed size during fingerprinting"
    );
    file.seek(SeekFrom::Start(0))?;
    Ok((format!("{:x}", digest.finalize()), file))
}

fn archive_entry(zip: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>> {
    relative(name)?;
    ensure!(zip.len() <= 100_000, "too many original archive entries");
    let mut index = None;
    for i in 0..zip.len() {
        let file = zip.by_index(i)?;
        if file.name()?.eq_ignore_ascii_case(name) {
            ensure!(
                index.is_none() && file.name()?.as_ref() == name,
                "ambiguous/case-mismatched original entry"
            );
            index = Some(i);
        }
    }
    let mut file = zip.by_index(index.context("original archive entry not found")?)?;
    ensure!(
        file.is_file()
            && file.size() <= MAX_FILE as u64
            && file
                .unix_mode()
                .is_none_or(|m| matches!(m & 0o170000, 0 | 0o100000)),
        "oversized/nonregular original entry"
    );
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_FILE && bytes.len() as u64 == file.size(),
        "original entry size mismatch/limit"
    );
    Ok(bytes)
}

pub fn fingerprint(path: &Path, entry: Option<&str>) -> Result<Fingerprint> {
    let (file_sha256, file) = hashed_file(path)?;
    let entry = entry
        .map(|name| -> Result<_> {
            let bytes = archive_entry(&mut ZipArchive::new(file)?, name)?;
            Ok(EntryFingerprint {
                path: name.into(),
                bytes: bytes.len(),
                sha256: hash(&bytes),
            })
        })
        .transpose()?;
    Ok(Fingerprint { file_sha256, entry })
}

fn convert(source: &Source, transform: &Transform, bytes: &[u8]) -> Result<Vec<u8>> {
    match transform {
        Transform::Copy => Ok(bytes.to_vec()),
        Transform::Png { flip_y, max_edge } => {
            let format = image::ImageFormat::from_extension(super::super::extension(Path::new(
                source.name(),
            )))
            .context("unknown texture format")?;
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(8192);
            limits.max_image_height = Some(8192);
            limits.max_alloc = Some(64 * 1024 * 1024);
            let mut probe = image::ImageReader::with_format(Cursor::new(bytes), format);
            probe.limits(limits.clone());
            let (width, height) = probe.into_dimensions()?;
            ensure!(
                u64::from(width) * u64::from(height) <= 16 * 1024 * 1024,
                "texture exceeds 16 megapixels"
            );
            let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
            reader.limits(limits);
            let mut image = reader
                .decode()
                .context("decoding source texture within limits")?;
            ensure!(
                u64::from(image.width()) * u64::from(image.height()) <= 16 * 1024 * 1024,
                "texture exceeds 16 megapixels"
            );
            if *flip_y {
                image = image.flipv();
            }
            if let Some(edge) = *max_edge
                && image.width().max(image.height()) > edge
            {
                image = image.resize(edge, edge, image::imageops::FilterType::Lanczos3);
            }
            let mut encoded = Cursor::new(Vec::new());
            image.write_to(&mut encoded, image::ImageFormat::Png)?;
            let bytes = encoded.into_inner();
            ensure!(bytes.len() <= MAX_FILE, "converted PNG exceeds 16 MiB");
            Ok(bytes)
        }
    }
}

fn cache_key(derivation: &Derivation) -> Result<[u8; 32]> {
    // Include the source extension: it selects the decoder (not just its bytes).
    let key = serde_json::to_vec(&(
        derivation.pipeline_revision,
        &derivation.dependency_lock_sha256,
        derivation.source.sha256(),
        super::super::extension(Path::new(derivation.source.name())),
        &derivation.transform,
    ))?;
    Ok(Sha256::digest(key).into())
}

fn cached(cache: &Path, derivation: &Derivation, source: &[u8]) -> Result<(Vec<u8>, bool)> {
    let key = cache_key(derivation)?;
    let name = key.iter().map(|n| format!("{n:02x}")).collect::<String>();
    let file = cache.join(format!("{name}.bin"));
    match fs::symlink_metadata(&file) {
        Ok(_) => {
            let bytes = read(&file, MAX_FILE + CACHE_HEADER)?;
            ensure!(
                bytes.len() >= CACHE_HEADER && &bytes[..8] == CACHE_MAGIC && bytes[8..40] == key,
                "invalid cache header/key; preserve or move the cache entry"
            );
            let payload = &bytes[CACHE_HEADER..];
            let len = u64::from_le_bytes(bytes[72..80].try_into().unwrap());
            ensure!(
                payload.len() as u64 == len && Sha256::digest(payload)[..] == bytes[40..72],
                "corrupt cache payload; no output written"
            );
            Ok((payload.to_vec(), true))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let bytes = convert(&derivation.source, &derivation.transform, source)?;
            let mut entry = Vec::with_capacity(CACHE_HEADER + bytes.len());
            entry.extend_from_slice(CACHE_MAGIC);
            entry.extend_from_slice(&key);
            entry.extend_from_slice(&Sha256::digest(&bytes));
            entry.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            entry.extend_from_slice(&bytes);
            write_new(&file, &entry)?;
            Ok((bytes, false))
        }
        Err(e) => Err(e.into()),
    }
}

fn location(path: &Path) -> Result<PathBuf> {
    let resolved = if path.exists() {
        checked(path)?.canonicalize()?
    } else {
        path.parent()
            .context("destination parent missing")?
            .canonicalize()?
            .join(path.file_name().context("missing filename")?)
    };
    #[cfg(windows)]
    let resolved = PathBuf::from(resolved.to_string_lossy().to_lowercase());
    Ok(resolved)
}

fn directory_outside_inputs(root: &Path, path: &Path) -> Result<PathBuf> {
    let path = checked(path)?;
    let resolved = location(&path)?;
    let inputs = root.join("inputs");
    if inputs.exists() {
        ensure!(
            !resolved.starts_with(location(&checked(&inputs)?)?),
            "cache/output must be outside inputs"
        );
    }
    Ok(path)
}

pub fn build(
    recipe: &Path,
    output: &Path,
    cache: &Path,
    archives: &[PathBuf],
    distributable: bool,
) -> Result<Report> {
    ensure!(
        archives.len() <= MAX_BINDINGS,
        "at most 16 original archive bindings"
    );
    let path = checked(recipe)?;
    let mut recipe: Recipe = serde_json::from_slice(&read(&path, MAX_JSON)?)?;
    ensure!(recipe.schema == 1, "unsupported recipe schema");
    label(&recipe.id)?;
    label(&recipe.version)?;
    let inputs = recipe
        .files
        .iter()
        .map(|f| {
            f.transform.validate(&f.source, &f.path)?;
            f.source.input(f.path.clone(), f.depends_on.clone())
        })
        .collect::<Result<Vec<_>>>()?;
    graph(&inputs)?;
    ensure!(
        !distributable || inputs.iter().all(|i| i.origin == Origin::Owned),
        "derived_local assets cannot enter a distributable package"
    );
    let root = path
        .parent()
        .context("recipe parent missing")?
        .canonicalize()?;
    let output = directory_outside_inputs(&root, output)?;
    let cache = directory_outside_inputs(&root, cache)?;
    let resolved_cache = location(&cache)?;
    let resolved_output = location(&output)?;
    ensure!(
        !resolved_cache.starts_with(&resolved_output)
            && !resolved_output.starts_with(&resolved_cache),
        "cache and output must be separate trees"
    );
    let mut originals = BTreeMap::new();
    for archive in archives {
        let (sha, file) = hashed_file(archive)?;
        ensure!(
            !originals.contains_key(&sha),
            "duplicate original archive binding"
        );
        ensure!(recipe.files.iter().any(|f| matches!(&f.source, Source::LocalArchive { archive_sha256, .. } if *archive_sha256 == sha)), "archive binding is not referenced by recipe");
        originals.insert(sha, ZipArchive::new(file)?);
    }
    for item in &recipe.files {
        if let Source::LocalArchive { archive_sha256, .. } = &item.source {
            ensure!(
                originals.contains_key(archive_sha256),
                "required original archive hash not bound"
            );
        }
    }
    if !cache.exists() {
        fresh_dir(&cache)?;
    }
    ensure!(cache.is_dir(), "cache must be a directory");
    let lock = hash(include_bytes!("../../../Cargo.lock"));
    let mut entries = BTreeMap::new();
    let mut files = BTreeMap::new();
    let mut report_hits = 0;
    let mut total = 0;
    recipe.files.sort_by(|a, b| a.path.cmp(&b.path));
    for mut item in recipe.files {
        let source = match &item.source {
            Source::OwnedFile { path, .. } | Source::LocalFile { path, .. } => {
                read(&root.join(path), MAX_FILE)?
            }
            Source::LocalArchive {
                archive_sha256,
                entry,
                ..
            } => archive_entry(
                originals
                    .get_mut(archive_sha256)
                    .context("required original archive hash not bound")?,
                entry,
            )?,
        };
        // A cache hit never bypasses verification of the actual local original.
        ensure!(
            hash(&source) == item.source.sha256(),
            "source SHA-256 mismatch; original/project changed"
        );
        item.depends_on.sort();
        let input = item.source.input(item.path.clone(), item.depends_on)?;
        let derivation = Derivation {
            source: item.source,
            transform: item.transform,
            pipeline_revision: REVISION,
            dependency_lock_sha256: lock.clone(),
        };
        let (bytes, hit) = cached(&cache, &derivation, &source)?;
        total += bytes.len();
        ensure!(total <= MAX_TOTAL, "converted overlay exceeds 32 MiB");
        data_only(&item.path, &bytes)?;
        report_hits += usize::from(hit);
        entries.insert(
            item.path.clone(),
            Entry {
                input,
                bytes: bytes.len(),
                sha256: hash(&bytes),
                derivation: Some(derivation),
            },
        );
        files.insert(item.path, bytes);
    }
    let count = files.len();
    let build = emit(
        recipe.id,
        recipe.version,
        entries,
        files,
        &output,
        distributable,
    )?;
    Ok(Report {
        cache_hits: report_hits,
        cache_misses: count - report_hits,
        build,
    })
}

/// An artificial archive is deliberately classified local, to exercise the
/// same restrictions as commercial inputs without requiring commercial assets.
pub fn example(output: &Path) -> Result<()> {
    super::example(output)?;
    let png_path = output.join("inputs/owned/checker.png");
    let png = read(&png_path, MAX_FILE)?;
    fs::create_dir(output.join("inputs/local"))?;
    let payload = archive(&BTreeMap::from([(
        "textures/checker.png".into(),
        png.clone(),
    )]))?;
    write_new(&output.join("inputs/local/example-original.zip"), &payload)?;
    let recipe = Recipe {
        schema: 1,
        id: "recipe-example".into(),
        version: "0.1.0".into(),
        files: vec![
            RecipeFile {
                source: Source::OwnedFile {
                    path: "inputs/owned/checker.png".into(),
                    sha256: hash(&png),
                },
                transform: Transform::Png {
                    flip_y: true,
                    max_edge: None,
                },
                path: "textures/mp/own-flipped.png".into(),
                depends_on: vec![],
            },
            RecipeFile {
                source: Source::LocalArchive {
                    archive_sha256: hash(&payload),
                    entry: "textures/checker.png".into(),
                    sha256: hash(&png),
                },
                transform: Transform::Copy,
                path: "textures/mp/local-copy.png".into(),
                depends_on: vec![],
            },
        ],
    };
    write_new(
        &output.join("recipe.json"),
        &serde_json::to_vec_pretty(&recipe)?,
    )
}

#[cfg(test)]
mod tests;

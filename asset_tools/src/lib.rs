pub mod bindings;
use anyhow::{Context, Result, ensure};
use image::{ImageFormat, imageops::FilterType};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};
use walkdir::WalkDir;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const MAX_ENTRIES: usize = 100_000;

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn open_archive(path: &Path) -> Result<ZipArchive<File>> {
    ZipArchive::new(File::open(path).with_context(|| format!("opening {}", path.display()))?)
        .with_context(|| format!("{} is not a supported ZIP/SCS archive", path.display()))
}

pub fn scan(root: &Path) -> Result<()> {
    let mut count = 0;
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            !matches!(
                entry.file_name().to_str(),
                Some("target" | ".git" | "extracted" | "generated")
            )
        })
    {
        let entry = entry?;
        if !entry.file_type().is_file()
            || !matches!(extension(entry.path()).as_str(), "zip" | "scs")
        {
            continue;
        }
        let mut archive = open_archive(entry.path())?;
        let mut formats = BTreeMap::<String, usize>::new();
        for index in 0..archive.len() {
            let file = archive.by_index(index)?;
            if file.is_file() {
                *formats
                    .entry(extension(Path::new(file.name()?.as_ref())))
                    .or_default() += 1;
            }
        }
        println!(
            "{}: {} entries, {} bytes",
            entry.path().display(),
            archive.len(),
            entry.metadata()?.len()
        );
        println!("  extensions: {formats:?}");
        count += 1;
    }
    println!("{count} archives found");
    Ok(())
}

pub fn list(path: &Path, limit: usize) -> Result<()> {
    let mut archive = open_archive(path)?;
    println!("{} entries", archive.len());
    for index in 0..archive.len().min(limit) {
        let file = archive.by_index(index)?;
        println!(
            "{:>12} bytes  {:?}  {}",
            file.size(),
            file.compression(),
            file.name()?
        );
    }
    Ok(())
}

/// Reads one exact UTF-8 entry without extracting or changing any file.
/// Both the advertised and expanded lengths are bounded independently.
pub fn read_text(path: &Path, name: &str, max_bytes: usize) -> Result<String> {
    ensure!(
        (1..=1024 * 1024).contains(&max_bytes),
        "text limit must be 1..=1048576 bytes"
    );
    safe_relative(name)?;
    let mut archive = open_archive(path)?;
    ensure!(archive.len() <= MAX_ENTRIES, "too many archive entries");
    let mut found = None;
    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        if file.name()?.as_ref() == name {
            ensure!(found.is_none(), "ambiguous duplicate archive entry: {name}");
            found = Some(index);
        }
    }
    let mut file = archive.by_index(found.context("archive entry not found")?)?;
    ensure!(file.is_file(), "entry is not a regular file");
    if let Some(mode) = file.unix_mode() {
        ensure!(
            matches!(mode & 0o170000, 0 | 0o100000),
            "links/special files are unsupported"
        );
    }
    ensure!(
        file.size() <= max_bytes as u64,
        "entry exceeds text size limit"
    );
    let mut bytes = Vec::new();
    (&mut file)
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= max_bytes,
        "expanded entry exceeds text size limit"
    );
    ensure!(
        bytes.len() as u64 == file.size(),
        "archive entry size mismatch"
    );
    String::from_utf8(bytes).context("archive entry is not UTF-8 text")
}

// Strict on Windows as well as Unix: reject traversal, alternate streams,
// device names, trailing-dot aliases and archive-provided symlinks.
fn safe_relative(name: &str) -> Result<PathBuf> {
    ensure!(
        !name.contains(['\\', ':', '\0']),
        "unsafe archive name: {name:?}"
    );
    let path = Path::new(name);
    ensure!(!path.is_absolute(), "absolute archive name: {name:?}");
    for part in name.trim_end_matches('/').split('/') {
        ensure!(
            !part.is_empty() && part != "." && part != "..",
            "unsafe path: {name:?}"
        );
        ensure!(
            !part.ends_with(['.', ' ']),
            "ambiguous Windows name: {name:?}"
        );
        ensure!(
            !part.chars().any(|c| c < ' ' || "<>\"|?*".contains(c)),
            "invalid filename: {name:?}"
        );
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        let numbered_device = stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9');
        ensure!(
            !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") && !numbered_device,
            "reserved Windows name: {name:?}"
        );
    }
    ensure!(
        path.components().all(|c| matches!(c, Component::Normal(_))),
        "unsafe path: {name:?}"
    );
    Ok(path.to_path_buf())
}

/// Writes exclusively to a newly created output directory. Partial output is
/// retained on failure for inspection; a retry must use a fresh directory.
pub fn extract(input: &Path, output: &Path, max_bytes: u64) -> Result<usize> {
    let mut archive = open_archive(input)?;
    ensure!(archive.len() <= MAX_ENTRIES, "too many archive entries");
    let mut total = 0u64;
    let mut names = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        let relative = safe_relative(&file.name()?)?;
        if let Some(mode) = file.unix_mode() {
            let kind = mode & 0o170000;
            ensure!(
                matches!(kind, 0 | 0o040000 | 0o100000),
                "links/special files are unsupported"
            );
        }
        total = total
            .checked_add(file.size())
            .context("archive size overflow")?;
        ensure!(total <= max_bytes, "archive exceeds extraction size limit");
        names.push(relative);
    }
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(output)
        .with_context(|| format!("output must be a new directory: {}", output.display()))?;
    let mut remaining = max_bytes;
    let mut files = 0;
    for (index, relative) in names.iter().enumerate() {
        let mut file = archive.by_index(index)?;
        let destination = output.join(relative);
        if file.is_dir() {
            fs::create_dir_all(destination)?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .with_context(|| format!("refusing to overwrite {}", destination.display()))?;
        let written = io::copy(&mut (&mut file).take(remaining.saturating_add(1)), &mut out)?;
        ensure!(
            written <= remaining,
            "actual expanded data exceeds size limit"
        );
        ensure!(written == file.size(), "archive entry size mismatch");
        remaining -= written;
        files += 1;
    }
    Ok(files)
}

pub fn convert(input: &Path, output: &Path, flip_y: bool, max_edge: Option<u32>) -> Result<()> {
    ensure!(extension(output) == "png", "output must end in .png");
    let mut image = image::ImageReader::open(input)?
        .decode()
        .with_context(|| format!("unsupported/damaged texture: {}", input.display()))?;
    if flip_y {
        image = image.flipv();
    }
    if let Some(edge) = max_edge {
        ensure!((1..=8192).contains(&edge), "max edge must be 1..=8192");
        if image.width() > edge || image.height() > edge {
            image = image.resize(edge, edge, FilterType::Lanczos3);
        }
    }
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .with_context(|| format!("output must be a new file: {}", output.display()))?;
    let mut writer = BufWriter::new(file);
    image.write_to(&mut writer, ImageFormat::Png)?;
    writer.flush()?;
    println!(
        "PNG {} ({} x {})",
        output.display(),
        image.width(),
        image.height()
    );
    Ok(())
}

pub fn pack(input: &Path, output: &Path) -> Result<usize> {
    ensure!(
        matches!(extension(output).as_str(), "scs" | "zip"),
        "output must end in .scs or .zip"
    );
    let root = input.canonicalize()?;
    ensure!(root.is_dir(), "input must be a directory");
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    ensure!(
        !parent.canonicalize()?.starts_with(&root),
        "archive output must be outside input directory"
    );
    let mut entries = Vec::new();
    for entry in WalkDir::new(&root)
        .follow_links(false)
        .sort_by_file_name()
        .min_depth(1)
    {
        let entry = entry?;
        ensure!(
            !entry.file_type().is_symlink(),
            "refusing symlink: {}",
            entry.path().display()
        );
        ensure!(
            entry.file_type().is_dir() || entry.file_type().is_file(),
            "special files unsupported"
        );
        let name = entry
            .path()
            .strip_prefix(&root)?
            .to_str()
            .context("filename is not UTF-8")?
            .replace('\\', "/");
        safe_relative(&name)?;
        ensure!(
            entry.metadata()?.len() < u64::from(u32::MAX),
            "ZIP64 files unsupported for legacy game"
        );
        entries.push((entry.path().to_path_buf(), name, entry.file_type().is_dir()));
    }
    ensure!(
        entries.len() < usize::from(u16::MAX),
        "ZIP64 archives unsupported for legacy game"
    );
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .with_context(|| format!("output must be a new file: {}", output.display()))?;
    let mut zip = ZipWriter::new(file);
    // Stored + ZIP32 avoids introducing newer compression methods into the game.
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut count = 0;
    for (path, name, directory) in entries {
        if directory {
            zip.add_directory(format!("{name}/"), options)?;
        } else {
            zip.start_file(name, options)?;
            io::copy(&mut File::open(path)?, &mut zip)?;
            count += 1;
        }
    }
    let file = zip.finish()?;
    ensure!(
        file.metadata()?.len() < u64::from(u32::MAX),
        "archive exceeds legacy ZIP32 size"
    );
    Ok(count)
}

pub fn inspect_pe(path: &Path) -> Result<()> {
    let mut file = File::open(path)?;
    let mut dos = [0; 64];
    file.read_exact(&mut dos)?;
    ensure!(&dos[..2] == b"MZ", "missing MZ header");
    let offset = u32::from_le_bytes(dos[60..64].try_into()?);
    file.seek(SeekFrom::Start(u64::from(offset)))?;
    let mut pe = [0; 6];
    file.read_exact(&mut pe)?;
    ensure!(&pe[..4] == b"PE\0\0", "missing PE signature");
    let machine = u16::from_le_bytes(pe[4..6].try_into()?);
    let architecture = match machine {
        0x014c => "x86 (32-bit)",
        0x8664 => "x86_64 (64-bit)",
        _ => "other",
    };
    println!(
        "{}: {architecture}, machine=0x{machine:04x}",
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rejects_traversal_device_names_and_alternate_streams() {
        for name in [
            "../escape",
            "/absolute",
            "C:/outside",
            "dir\\outside",
            "a/../b",
            "CON.txt",
            "x:stream",
            "a./b",
            "LPT1",
            "a//b",
        ] {
            assert!(safe_relative(name).is_err(), "accepted {name}");
        }
        assert!(safe_relative("vehicle/truck/body.dds").is_ok());
    }

    #[test]
    fn pack_extract_roundtrip_limits_and_no_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("sub")).unwrap();
        fs::write(source.join("sub/test.txt"), b"own test content").unwrap();
        let archive = temp.path().join("own.scs");
        assert_eq!(pack(&source, &archive).unwrap(), 1);
        assert!(pack(&source, &archive).is_err());
        assert!(pack(&source, &source.join("recursive.scs")).is_err());
        let dest = temp.path().join("result");
        assert!(extract(&archive, &dest, 1).is_err());
        assert!(!dest.exists());
        assert_eq!(extract(&archive, &dest, 1024).unwrap(), 1);
        assert_eq!(
            fs::read(dest.join("sub/test.txt")).unwrap(),
            b"own test content"
        );
        assert!(extract(&archive, &dest, 1024).is_err());
    }

    #[test]
    fn reads_only_bounded_exact_text_without_extraction() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("own.scs");
        let mut zip = ZipWriter::new(File::create(&archive).unwrap());
        zip.start_file("models/own.def", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"model: own\n").unwrap();
        zip.start_file("binary", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&[0xff]).unwrap();
        zip.finish().unwrap();
        let before = fs::read(&archive).unwrap();
        assert_eq!(
            read_text(&archive, "models/own.def", 11).unwrap(),
            "model: own\n"
        );
        assert!(read_text(&archive, "models/own.def", 10).is_err());
        assert!(read_text(&archive, "models/own.def", 0).is_err());
        assert!(read_text(&archive, "models/own.def", 1024 * 1024 + 1).is_err());
        assert!(read_text(&archive, "Models/own.def", 100).is_err());
        assert!(read_text(&archive, "../own.def", 100).is_err());
        assert!(read_text(&archive, "binary", 100).is_err());
        assert_eq!(fs::read(&archive).unwrap(), before);
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    fn malicious_zip_is_rejected_before_output_is_created() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("bad.zip");
        let mut zip = ZipWriter::new(File::create(&archive).unwrap());
        zip.start_file("../escape.txt", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"test").unwrap();
        zip.finish().unwrap();
        let output = temp.path().join("output");
        assert!(extract(&archive, &output, 1024).is_err());
        assert!(!output.exists());
        assert!(!temp.path().join("escape.txt").exists());
    }

    #[test]
    fn converts_flips_and_resizes_a_real_texture() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("own.bmp");
        let output = temp.path().join("own.png");
        let image =
            image::RgbImage::from_fn(4, 4, |_, y| image::Rgb([if y < 2 { 255 } else { 0 }, 0, 0]));
        image.save(&input).unwrap();
        convert(&input, &output, true, Some(2)).unwrap();
        let result = image::open(&output).unwrap().to_rgb8();
        assert_eq!(result.dimensions(), (2, 2));
        assert!(result.get_pixel(0, 0)[0] < result.get_pixel(0, 1)[0]);
        assert!(convert(&input, &output, false, None).is_err());
    }
}
pub mod binary;
pub mod pe32;

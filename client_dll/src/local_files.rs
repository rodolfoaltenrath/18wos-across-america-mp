//! Bounded local reads, not a sandbox against concurrent path replacement.
use anyhow::{Result, ensure};
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

pub(crate) fn metadata(path: &Path) -> Result<fs::Metadata> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        !meta.file_type().is_symlink(),
        "linked local files are unsupported"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            meta.file_attributes() & 0x400 == 0,
            "reparse files are unsupported"
        );
    }
    Ok(meta)
}
pub(crate) fn read(path: &Path, cap: usize) -> Result<Vec<u8>> {
    let meta = metadata(path)?;
    ensure!(
        meta.is_file() && meta.len() <= cap as u64,
        "local file type/size exceeds limit"
    );
    let file = File::open(path)?;
    let opened = file.metadata()?;
    ensure!(
        opened.is_file() && opened.len() == meta.len(),
        "local file changed before read"
    );
    let mut bytes = Vec::new();
    file.take(cap as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 == meta.len(),
        "local file changed or exceeds limit"
    );
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_reads_reject_directories_large_files_and_links() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("own.txt");
        fs::write(&p, b"12345").unwrap();
        assert!(read(&p, 4).is_err());
        assert_eq!(read(&p, 5).unwrap(), b"12345");
        assert!(read(t.path(), 100).is_err());
        #[cfg(unix)]
        {
            let link = t.path().join("link.txt");
            std::os::unix::fs::symlink(&p, &link).unwrap();
            assert!(read(&link, 100).is_err());
        }
    }
}

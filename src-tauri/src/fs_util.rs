use std::fs::File;
use std::io::{Error, ErrorKind, Read};
use std::path::Path;

pub fn read_to_string_limited(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    if let Ok(meta) = path.metadata() {
        if meta.is_file() && meta.len() > max_bytes {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!(
                    "file is too large ({} bytes; limit is {max_bytes} bytes)",
                    meta.len()
                ),
            ));
        }
    }

    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    let mut limited = file.by_ref().take(max_bytes + 1);
    limited.read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("file is too large (limit is {max_bytes} bytes)"),
        ));
    }
    String::from_utf8(bytes).map_err(|e| Error::new(ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_small_utf8_file() {
        let dir = std::env::temp_dir().join("droidsmith-read-limit-small");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("small.txt");
        std::fs::write(&path, "hello").unwrap();
        assert_eq!(read_to_string_limited(&path, 16).unwrap(), "hello");
    }

    #[test]
    fn rejects_oversized_file() {
        let dir = std::env::temp_dir().join("droidsmith-read-limit-large");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("large.txt");
        std::fs::write(&path, "hello").unwrap();
        let err = read_to_string_limited(&path, 4).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData);
    }
}

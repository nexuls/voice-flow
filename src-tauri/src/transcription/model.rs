//! Speech model files: discovery, verification and explicit installation.
//!
//! Model weights are never bundled or committed. They are downloaded only when
//! the user asks, from a pinned Hugging Face revision, and checked against a
//! SHA-256 checksum before use. Downloads go to `<file>.part` and are renamed
//! into place only after verification.

use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub file_name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    pub license: &'static str,
}

/// Cactus Whistle (Apache-2.0), pinned to revision 741118687e1f of
/// huggingface.co/Cactus-Compute/whistle.
pub const WHISTLE: ModelSpec = ModelSpec {
    id: "whistle",
    display_name: "Cactus Whistle",
    file_name: "whistle.cact",
    url: "https://huggingface.co/Cactus-Compute/whistle/resolve/741118687e1f008f06d1351a402a731274789790/whistle.cact",
    sha256: "b6e02f048568ac5d01a2042556c658061e699acbc0aa2a1439f52f3d461dffeb",
    size: 16_919_407,
    license: "Apache-2.0",
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ModelStatus {
    NotInstalled,
    #[serde(rename_all = "camelCase")]
    Installed {
        path: PathBuf,
        size: u64,
    },
    /// A file exists but is unusable (wrong size or checksum).
    #[serde(rename_all = "camelCase")]
    Invalid {
        path: PathBuf,
        reason: String,
    },
    /// A partial download from an interrupted install.
    #[serde(rename_all = "camelCase")]
    Incomplete {
        path: PathBuf,
        bytes: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("download failed: {0}")]
    Network(String),
    #[error("download was cancelled")]
    Cancelled,
    #[error("downloaded file has the wrong size (expected {expected} bytes, got {actual})")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("downloaded file failed checksum verification")]
    ChecksumMismatch,
    #[error("file error: {0}")]
    Io(String),
}

impl From<std::io::Error> for ModelError {
    fn from(e: std::io::Error) -> Self {
        ModelError::Io(e.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct ModelStore {
    dir: PathBuf,
}

/// Size and checksum of a file, computed in one pass.
pub fn sha256_file(path: &Path) -> std::io::Result<(u64, String)> {
    let mut reader = BufReader::with_capacity(1 << 20, File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((total, hex(&hasher.finalize())))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl ModelStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self, spec: &ModelSpec) -> PathBuf {
        self.dir.join(spec.file_name)
    }

    fn part_path(&self, spec: &ModelSpec) -> PathBuf {
        self.dir.join(format!("{}.part", spec.file_name))
    }

    /// Fast status check based on file size only. Use [`Self::verify`] for a
    /// checksum check.
    pub fn status(&self, spec: &ModelSpec) -> ModelStatus {
        let path = self.path(spec);
        match std::fs::metadata(&path) {
            Ok(m) if m.len() == spec.size => ModelStatus::Installed {
                path,
                size: m.len(),
            },
            Ok(m) => ModelStatus::Invalid {
                path,
                reason: format!("unexpected size {} bytes", m.len()),
            },
            Err(_) => match std::fs::metadata(self.part_path(spec)) {
                Ok(m) => ModelStatus::Incomplete {
                    path: self.part_path(spec),
                    bytes: m.len(),
                },
                Err(_) => ModelStatus::NotInstalled,
            },
        }
    }

    /// Full integrity check. Returns the verified path.
    pub fn verify(&self, spec: &ModelSpec) -> Result<PathBuf, ModelError> {
        let path = self.path(spec);
        let (size, digest) = sha256_file(&path)?;
        if size != spec.size {
            return Err(ModelError::SizeMismatch {
                expected: spec.size,
                actual: size,
            });
        }
        if digest != spec.sha256 {
            return Err(ModelError::ChecksumMismatch);
        }
        Ok(path)
    }

    /// Deletes the model and any partial download.
    pub fn remove(&self, spec: &ModelSpec) -> Result<(), ModelError> {
        for p in [self.path(spec), self.part_path(spec)] {
            match std::fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    /// Downloads and verifies `spec` from `spec.url`. Blocking: call it from a
    /// blocking thread. `progress(downloaded, total)` is called as data
    /// arrives.
    pub fn install(
        &self,
        spec: &ModelSpec,
        cancel: &Arc<AtomicBool>,
        progress: impl FnMut(u64, u64),
    ) -> Result<PathBuf, ModelError> {
        self.install_from(spec, spec.url, cancel, progress)
    }

    /// Like [`Self::install`] with an explicit URL (used by tests).
    pub fn install_from(
        &self,
        spec: &ModelSpec,
        url: &str,
        cancel: &Arc<AtomicBool>,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<PathBuf, ModelError> {
        std::fs::create_dir_all(&self.dir)?;
        let part = self.part_path(spec);
        let result = (|| {
            let response = ureq::get(url)
                .header(
                    "User-Agent",
                    concat!("VoiceFlow/", env!("CARGO_PKG_VERSION")),
                )
                .call()
                .map_err(|e| ModelError::Network(e.to_string()))?;
            let mut reader = response.into_body().into_reader();
            let mut file = File::create(&part)?;
            let mut hasher = Sha256::new();
            let mut buf = vec![0u8; 256 * 1024];
            let mut total = 0u64;
            loop {
                if cancel.load(Ordering::SeqCst) {
                    return Err(ModelError::Cancelled);
                }
                let n = reader
                    .read(&mut buf)
                    .map_err(|e| ModelError::Network(e.to_string()))?;
                if n == 0 {
                    break;
                }
                total += n as u64;
                if total > spec.size {
                    return Err(ModelError::SizeMismatch {
                        expected: spec.size,
                        actual: total,
                    });
                }
                hasher.update(&buf[..n]);
                file.write_all(&buf[..n])?;
                progress(total, spec.size);
            }
            if total != spec.size {
                return Err(ModelError::SizeMismatch {
                    expected: spec.size,
                    actual: total,
                });
            }
            if hex(&hasher.finalize()) != spec.sha256 {
                return Err(ModelError::ChecksumMismatch);
            }
            file.sync_all()?;
            drop(file);
            let dest = self.path(spec);
            std::fs::rename(&part, &dest)?;
            Ok(dest)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader as StdBufReader};
    use std::net::TcpListener;

    use super::*;

    const DATA: &[u8] = b"pretend this is a speech model";

    fn spec_for(data: &[u8]) -> ModelSpec {
        // Leaked once per test run, so `&'static` fields are fine.
        let sha: &'static str = Box::leak(hex(&Sha256::digest(data)).into_boxed_str());
        ModelSpec {
            id: "test",
            display_name: "Test",
            file_name: "test.cact",
            url: "http://unused",
            sha256: sha,
            size: data.len() as u64,
            license: "none",
        }
    }

    /// Serves `body` once over plain HTTP. If `truncate` is set, it announces
    /// the full length but closes the connection early.
    fn serve_once(body: Vec<u8>, truncate: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = StdBufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            let n = if truncate { body.len() / 2 } else { body.len() };
            stream.write_all(&body[..n]).unwrap();
        });
        format!("http://{addr}/model")
    }

    fn no_cancel() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[test]
    fn status_reports_missing_invalid_incomplete_and_installed() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let spec = spec_for(DATA);
        assert_eq!(store.status(&spec), ModelStatus::NotInstalled);
        std::fs::write(dir.path().join("test.cact.part"), b"half").unwrap();
        assert!(matches!(
            store.status(&spec),
            ModelStatus::Incomplete { bytes: 4, .. }
        ));
        std::fs::write(store.path(&spec), b"short").unwrap();
        assert!(matches!(store.status(&spec), ModelStatus::Invalid { .. }));
        std::fs::write(store.path(&spec), DATA).unwrap();
        assert!(matches!(store.status(&spec), ModelStatus::Installed { .. }));
        store.remove(&spec).unwrap();
        assert_eq!(store.status(&spec), ModelStatus::NotInstalled);
    }

    #[test]
    fn verify_detects_same_size_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let spec = spec_for(DATA);
        let mut corrupt = DATA.to_vec();
        corrupt[3] ^= 0xff;
        std::fs::write(store.path(&spec), &corrupt).unwrap();
        assert!(matches!(store.status(&spec), ModelStatus::Installed { .. }));
        assert_eq!(store.verify(&spec), Err(ModelError::ChecksumMismatch));
        std::fs::write(store.path(&spec), DATA).unwrap();
        assert_eq!(store.verify(&spec).unwrap(), store.path(&spec));
    }

    #[test]
    fn install_downloads_and_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path().join("models"));
        let spec = spec_for(DATA);
        let url = serve_once(DATA.to_vec(), false);
        let mut last = (0, 0);
        let path = store
            .install_from(&spec, &url, &no_cancel(), |d, t| last = (d, t))
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), DATA);
        assert_eq!(last, (DATA.len() as u64, DATA.len() as u64));
        assert!(!dir.path().join("models/test.cact.part").exists());
    }

    #[test]
    fn install_rejects_tampered_download_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let spec = spec_for(DATA);
        let mut tampered = DATA.to_vec();
        tampered[0] ^= 1;
        let url = serve_once(tampered, false);
        let err = store.install_from(&spec, &url, &no_cancel(), |_, _| {});
        assert_eq!(err, Err(ModelError::ChecksumMismatch));
        assert_eq!(store.status(&spec), ModelStatus::NotInstalled);
    }

    #[test]
    fn install_rejects_truncated_download() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let spec = spec_for(DATA);
        let url = serve_once(DATA.to_vec(), true);
        let err = store
            .install_from(&spec, &url, &no_cancel(), |_, _| {})
            .unwrap_err();
        assert!(
            matches!(
                err,
                ModelError::Network(_) | ModelError::SizeMismatch { .. }
            ),
            "{err:?}"
        );
        assert_eq!(store.status(&spec), ModelStatus::NotInstalled);
    }

    #[test]
    fn install_rejects_oversized_download() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let spec = spec_for(DATA);
        let url = serve_once([DATA, b"extra"].concat(), false);
        let err = store
            .install_from(&spec, &url, &no_cancel(), |_, _| {})
            .unwrap_err();
        assert!(matches!(err, ModelError::SizeMismatch { .. }));
    }

    #[test]
    fn install_can_be_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let spec = spec_for(DATA);
        let url = serve_once(DATA.to_vec(), false);
        let cancel = Arc::new(AtomicBool::new(true));
        assert_eq!(
            store.install_from(&spec, &url, &cancel, |_, _| {}),
            Err(ModelError::Cancelled)
        );
        assert_eq!(store.status(&spec), ModelStatus::NotInstalled);
    }

    #[test]
    fn unreachable_server_is_a_network_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let err = store
            .install_from(
                &spec_for(DATA),
                "http://127.0.0.1:1/x",
                &no_cancel(),
                |_, _| {},
            )
            .unwrap_err();
        assert!(matches!(err, ModelError::Network(_)));
    }

    #[test]
    fn whistle_spec_is_pinned() {
        assert!(
            WHISTLE
                .url
                .contains("/resolve/741118687e1f008f06d1351a402a731274789790/")
        );
        assert_eq!(WHISTLE.sha256.len(), 64);
    }
}

//! 固定只读源数据，供解析和后续解码复用同一份字节。
//!
//! 当前只在 Windows 提供受保护读取：句柄仅共享读取权限，在完整复制期间
//! 拒绝并发写入、删除和路径替换。复制完成即释放句柄，后续操作不再读取源路径。
//! 本模块仅负责文件边界与字节预算，不依赖解析器、应用状态或 Tauri。

use std::{
    error::Error,
    fmt,
    fs::File,
    io::{self, Read},
    path::Path,
};

#[derive(Debug)]
pub(super) struct SourceData {
    pub(super) bytes: Vec<u8>,
}

impl SourceData {
    /// 在 `max_bytes` 字节预算内取得稳定副本；成功或失败都会释放源文件句柄。
    pub(super) fn open(path: &Path, max_bytes: u64) -> Result<Self, SourceReadError> {
        let (mut file, length) = open_guarded_file(path)?;
        if length > max_bytes {
            return Err(SourceReadError::TooLarge { limit: max_bytes });
        }

        let length = usize::try_from(length).map_err(|error| {
            SourceReadError::Io(io::Error::new(io::ErrorKind::InvalidData, error))
        })?;
        // read_to_end 可能为探测 EOF 扩容；按已锁定文件的长度预留，避免超过字节预算。
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(|error| {
            SourceReadError::Io(io::Error::new(io::ErrorKind::OutOfMemory, error))
        })?;
        bytes.resize(length, 0);
        file.read_exact(&mut bytes).map_err(SourceReadError::Io)?;

        let mut extra = [0];
        loop {
            match file.read(&mut extra) {
                Ok(0) => return Ok(Self { bytes }),
                Ok(_) => {
                    return Err(SourceReadError::Io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "源文件在受保护读取期间改变了长度",
                    )));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(SourceReadError::Io(error)),
            }
        }
    }
}

#[derive(Debug)]
pub(super) enum SourceReadError {
    Io(io::Error),
    TooLarge {
        limit: u64,
    },
    NotAFile,
    #[cfg_attr(
        windows,
        expect(dead_code, reason = "保留其他平台拒绝受保护读取的统一错误分支")
    )]
    UnsupportedPlatform,
}

impl fmt::Display for SourceReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "读取 PSD 源数据失败：{error}"),
            Self::TooLarge { limit } => write!(formatter, "PSD 源文件超过 {limit} 字节限制"),
            Self::NotAFile => formatter.write_str("PSD 源路径不是普通文件"),
            Self::UnsupportedPlatform => formatter.write_str("当前平台尚未实现受保护的 PSD 源读取"),
        }
    }
}

impl Error for SourceReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

fn open_guarded_file(path: &Path) -> Result<(File, u64), SourceReadError> {
    #[cfg(windows)]
    {
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};

        const FILE_SHARE_READ: u32 = 0x0000_0001;
        // 允许取得目录句柄后按句柄元数据拒绝目录，避免先检查路径再打开的竞态。
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .map_err(SourceReadError::Io)?;
        let metadata = file.metadata().map_err(SourceReadError::Io)?;
        if !metadata.is_file() {
            return Err(SourceReadError::NotAFile);
        }
        Ok((file, metadata.len()))
    }

    #[cfg(not(windows))]
    {
        let _ = path;
        Err(SourceReadError::UnsupportedPlatform)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::{
        fs::{self, OpenOptions},
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            for _ in 0..100 {
                let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir()
                    .join(format!("layerlens-source-test-{}-{id}", std::process::id()));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("创建隔离测试目录失败：{error}"),
                }
            }
            panic!("无法分配隔离测试目录");
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_dir_all(&self.0) {
                if std::thread::panicking() {
                    eprintln!("清理测试目录失败：{error}");
                } else {
                    panic!("清理测试目录失败：{error}");
                }
            }
        }
    }

    #[test]
    fn guarded_handle_rejects_writes_delete_and_replace_but_allows_reads() {
        let directory = TestDirectory::new();
        let path = directory.0.join("source.psd");
        let replacement = directory.0.join("replacement.psd");
        fs::write(&path, b"original").unwrap();
        fs::write(&replacement, b"replacement").unwrap();

        let (guard, length) = open_guarded_file(&path).unwrap();
        assert_eq!(length, 8);
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(fs::rename(&path, directory.0.join("moved.psd")).is_err());
        assert!(fs::rename(&replacement, &path).is_err());
        assert!(fs::remove_file(&path).is_err());

        drop(guard);
        fs::rename(&replacement, &path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"replacement");
    }

    #[test]
    fn existing_writer_prevents_opening_a_guarded_source() {
        let directory = TestDirectory::new();
        let path = directory.0.join("source.psd");
        fs::write(&path, b"original").unwrap();
        let writer = OpenOptions::new().write(true).open(&path).unwrap();

        assert!(matches!(
            SourceData::open(&path, 8),
            Err(SourceReadError::Io(_))
        ));
        drop(writer);
        assert_eq!(SourceData::open(&path, 8).unwrap().bytes, b"original");
    }

    #[test]
    fn owned_bytes_survive_source_replacement_and_deletion() {
        let directory = TestDirectory::new();
        let path = directory.0.join("source.psd");
        let replacement = directory.0.join("replacement.psd");
        fs::write(&path, b"original").unwrap();
        let source = SourceData::open(&path, 8).unwrap();

        fs::write(&replacement, b"replacement").unwrap();
        fs::rename(&replacement, &path).unwrap();
        assert_eq!(source.bytes, b"original");
        assert_eq!(fs::read(&path).unwrap(), b"replacement");
        fs::remove_file(&path).unwrap();
        assert_eq!(source.bytes, b"original");
    }

    #[test]
    fn enforces_byte_limit_and_releases_handle_after_failure() {
        let directory = TestDirectory::new();
        let path = directory.0.join("source.psd");
        fs::write(&path, b"12345").unwrap();

        assert!(matches!(
            SourceData::open(&path, 4),
            Err(SourceReadError::TooLarge { limit: 4 })
        ));
        assert_eq!(SourceData::open(&path, 5).unwrap().bytes, b"12345");
        fs::write(&path, b"").unwrap();
        assert!(SourceData::open(&path, 0).unwrap().bytes.is_empty());
    }

    #[test]
    fn rejects_directory_and_preserves_missing_file_cause() {
        let directory = TestDirectory::new();
        assert!(matches!(
            SourceData::open(&directory.0, 100),
            Err(SourceReadError::NotAFile)
        ));

        let error = SourceData::open(&directory.0.join("missing.psd"), 100).unwrap_err();
        assert!(matches!(
            &error,
            SourceReadError::Io(cause) if cause.kind() == io::ErrorKind::NotFound
        ));
        assert!(error.source().is_some());
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn unsupported_platform_does_not_claim_a_stable_source() {
        assert!(matches!(
            SourceData::open(Path::new("unused.psd"), 100),
            Err(SourceReadError::UnsupportedPlatform)
        ));
    }
}

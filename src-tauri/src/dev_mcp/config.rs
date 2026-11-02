//! 开发实例配置与受控样本路径；只读原稿，每次运行创建独立目录与 WebView profile。

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Config {
    pub sample_root: PathBuf,
    pub run_root: PathBuf,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub web_driver_url: Option<String>,
    #[serde(default)]
    pub native_windows: bool,
    #[serde(default)]
    pub render_lab: bool,
    #[serde(default)]
    pub large_render: bool,
    #[serde(default)]
    pub profile_key: Option<Uuid>,
}

pub(crate) struct Instance {
    pub(super) id: String,
    pub(super) sample_root: PathBuf,
    pub(crate) directory: PathBuf,
    pub(crate) webview_directory: PathBuf,
    _profile_lock: Option<fs::File>,
    pub(super) token: String,
    pub(super) port: u16,
    pub(crate) visible: bool,
    pub(super) web_driver_url: Option<reqwest::Url>,
    pub(crate) observations: std::sync::Arc<super::ui::Observations>,
    pub(crate) native_windows: bool,
    pub(crate) render_lab: bool,
    pub(crate) large_render: bool,
    pub(crate) windows: std::sync::OnceLock<std::sync::Arc<super::windows::Windows>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Connection<'a> {
    pub format_version: u32,
    pub instance_id: &'a str,
    pub process_id: u32,
    pub endpoint: &'a str,
    pub token: &'a str,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

impl Config {
    pub(super) fn load(path: &Path) -> io::Result<Self> {
        let file = fs::File::open(path)?;
        if file.metadata()?.len() > 16 * 1024 {
            return Err(invalid("开发 MCP 配置超过 16 KiB"));
        }
        let config: Self = serde_json::from_reader(std::io::Read::take(file, 16 * 1024 + 1))
            .map_err(|e| invalid(format!("开发 MCP 配置无效：{e}")))?;
        Ok(config)
    }

    pub(super) fn prepare(self) -> io::Result<Instance> {
        if self.render_lab && !cfg!(feature = "render-lab") {
            return Err(invalid("当前构建未启用 render-lab 特性"));
        }
        if self.large_render && !self.render_lab {
            return Err(invalid("largeRender 仅允许在显式 renderLab 实例启用"));
        }
        if self.native_windows && (!self.visible || self.web_driver_url.is_none()) {
            return Err(invalid("nativeWindows 需显式可见窗口及 WebDriver 配置"));
        }
        if self.profile_key.is_some() && !self.native_windows {
            return Err(invalid("profileKey 仅允许显式原生窗口测试实例。"));
        }
        let web_driver_url = self
            .web_driver_url
            .map(|value| {
                let url = reqwest::Url::parse(&value).map_err(|_| invalid("webDriverUrl 无效"))?;
                if url.scheme() != "http"
                    || url.host_str() != Some("127.0.0.1")
                    || url.port().is_none_or(|p| p == 0)
                    || url.path() != "/"
                    || url.query().is_some()
                    || url.fragment().is_some()
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err(invalid(
                        "webDriverUrl 必须是带明确端口的 IPv4 回环 HTTP 根地址",
                    ));
                }
                Ok(url)
            })
            .transpose()?;
        if !self.sample_root.is_absolute() || !self.run_root.is_absolute() {
            return Err(invalid("sampleRoot 与 runRoot 必须是已存在目录的绝对路径"));
        }
        let sample_root = self.sample_root.canonicalize()?;
        let run_root = self.run_root.canonicalize()?;
        if !sample_root.is_dir()
            || !run_root.is_dir()
            || sample_root.starts_with(&run_root)
            || run_root.starts_with(&sample_root)
        {
            return Err(invalid("样本目录与运行目录必须存在且互不包含"));
        }
        let id = Uuid::new_v4().to_string();
        let directory = run_root.join(&id);
        let reusable = self
            .profile_key
            .map(|key| isolated_profile(&run_root, key))
            .transpose()?;
        fs::create_dir(&directory)?;
        let (webview_directory, profile_lock) = if let Some((profile, lock)) = reusable {
            (profile, Some(lock))
        } else {
            let profile = directory.join("webview");
            fs::create_dir(&profile)?;
            (profile, None)
        };
        Ok(Instance {
            id,
            sample_root,
            directory,
            webview_directory,
            _profile_lock: profile_lock,
            token: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
            port: self.port,
            visible: self.visible,
            web_driver_url,
            observations: std::sync::Arc::new(super::ui::Observations::default()),
            native_windows: self.native_windows,
            render_lab: self.render_lab,
            large_render: self.large_render,
            windows: std::sync::OnceLock::new(),
        })
    }
}

/// 重启测试仅复用同一隔离根内的 UUID profile，进程锁禁止并发使用；不接受外部目录。
fn isolated_profile(root: &Path, key: Uuid) -> io::Result<(PathBuf, fs::File)> {
    let parent = root.join("profiles");
    let directory = parent.join(key.to_string());
    for path in [&parent, &directory] {
        match fs::create_dir(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        if path.canonicalize()? != *path {
            return Err(invalid("测试 profile 不允许目录别名或重解析路径。"));
        }
    }
    let lock_path = directory.join("profile.lock");
    let lock = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            if lock_path.canonicalize()? != lock_path {
                return Err(invalid("测试 profile 锁不允许链接。"));
            }
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lock_path)?
        }
        Err(e) => return Err(e),
    };
    lock.try_lock()
        .map_err(|e| io::Error::other(format!("测试 profile 正由其他进程使用或不可锁定：{e}")))?;
    Ok((directory, lock))
}

impl Instance {
    /// 大稿额度只在已验证的隔离实验实例启用；正常桌面继续使用默认配置。
    pub(crate) fn document_config(&self) -> layerlens_core::documents::DocumentServiceConfig {
        let mut config = layerlens_core::documents::DocumentServiceConfig::default();
        if self.large_render {
            config.parse_limits.max_file_bytes = 512 * 1024 * 1024;
            config.parse_limits.max_total_pixels = 128 * 1024 * 1024;
            config.max_live_revisions = 2;
            config.max_source_bytes = 1024 * 1024 * 1024;
            config.max_declared_pixels = 256 * 1024 * 1024;
        }
        config
    }

    pub(crate) fn instance_id(&self) -> &str {
        &self.id
    }
    pub(crate) fn from_args() -> io::Result<Option<Self>> {
        Self::from_arguments(std::env::args_os().skip(1))
    }

    pub(super) fn from_arguments(
        mut args: impl Iterator<Item = std::ffi::OsString>,
    ) -> io::Result<Option<Self>> {
        let mut file = None;
        while let Some(arg) = args.next() {
            let attached = arg
                .to_str()
                .and_then(|value| value.strip_prefix("--dev-mcp-config="));
            if arg == "--dev-mcp-config" || attached.is_some() {
                if file.is_some() {
                    return Err(invalid("不能重复指定 --dev-mcp-config"));
                }
                let path = match attached {
                    Some(path) => std::ffi::OsString::from(path),
                    None => args
                        .next()
                        .ok_or_else(|| invalid("--dev-mcp-config 缺少配置路径"))?,
                };
                if path.is_empty() {
                    return Err(invalid("--dev-mcp-config 缺少配置路径"));
                }
                file = Some(path);
            } else if arg.to_string_lossy().starts_with("--dev-mcp") {
                return Err(invalid(
                    "未知开发 MCP 参数；使用 --dev-mcp-config <配置文件>",
                ));
            }
        }
        file.map(|p| Config::load(Path::new(&p))?.prepare())
            .transpose()
    }

    pub(super) fn sample(&self, value: &str) -> io::Result<PathBuf> {
        if value.is_empty() || value.len() > 4096 || value.as_bytes().contains(&0) {
            return Err(invalid("样本路径长度无效"));
        }
        let input = Path::new(value);
        if input
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(invalid("样本路径不能包含父目录跳转"));
        }
        let file = if input.is_absolute() {
            input.to_path_buf()
        } else {
            self.sample_root.join(input)
        }
        .canonicalize()?;
        if !file.starts_with(&self.sample_root)
            || !file.is_file()
            || !file
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("psd"))
        {
            return Err(invalid(
                "仅允许样本根目录内的 PSD 普通文件（含符号链接／目录联接检查）",
            ));
        }
        Ok(file)
    }

    pub(super) fn publish(&self, endpoint: &str) -> io::Result<()> {
        let connection = Connection {
            format_version: 1,
            instance_id: &self.id,
            process_id: std::process::id(),
            endpoint,
            token: &self.token,
        };
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.directory.join("connection.json"))?;
        serde_json::to_writer(&mut file, &connection).map_err(io::Error::other)?;
        file.flush()?;
        drop(file);
        // 客户端以 ready 为发布标记，避免读到尚未写完的连接信息。
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.directory.join("ready"))?;
        Ok(())
    }

    pub(super) fn finish(&self) -> io::Result<()> {
        self.remove_connection()?;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.directory.join("stopped"))?;
        Ok(())
    }

    fn remove_connection(&self) -> io::Result<()> {
        let mut failure = None;
        for name in ["ready", "connection.json"] {
            match fs::remove_file(self.directory.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => failure = Some(error),
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // 窗口、监听或 ready 发布中途失败时，仍回收本实例已经写出的连接信息。
        // 不递归删除样本或 WebView profile；它们属于运行报告保留目录。
        if let Err(error) = self.remove_connection() {
            eprintln!(
                "operation=dev_mcp_connection_cleanup instance={} error={error}",
                self.id
            );
        }
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;
    #[test]
    fn reusable_profile_is_exclusive_persistent_and_scoped_to_the_run_root() {
        let temp = crate::dev_mcp::tests::Temporary::new();
        let root = temp.0.canonicalize().unwrap();
        let key = Uuid::new_v4();
        let (directory, lock) = isolated_profile(&root, key).unwrap();
        fs::write(directory.join("proof"), b"retained").unwrap();
        assert!(isolated_profile(&root, key).is_err());
        let (other, _other_lock) = isolated_profile(&root, Uuid::new_v4()).unwrap();
        assert_ne!(directory, other);
        drop(lock);
        let (same, _lock) = isolated_profile(&root, key).unwrap();
        assert_eq!(same, directory);
        assert_eq!(fs::read(same.join("proof")).unwrap(), b"retained");
        let invalid =
            serde_json::json!({"sampleRoot":"x","runRoot":"y","profileKey":"../ordinary-profile"});
        assert!(serde_json::from_value::<Config>(invalid).is_err());
    }
}

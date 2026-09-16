//! M0-01 同步解析实验入口：只读 PSD，向全新目录输出报告与已支持的 PNG。
//!
//! 用法：`cargo run -p layerlens-core --example inspect_psd -- INPUT.psd OUTPUT_DIR [OPTIONS]`。
//! 输出目录必须不存在，父目录须预先创建；单项导出失败仍记录其余结果并返回非零。

use std::{
    collections::HashSet,
    env,
    error::Error,
    ffi::OsString,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

use layerlens_core::psd::{
    Capability, DocumentInfo, LayerId, PARSER_BUILD, ParseLimits, PsdDocument, PsdError, Support,
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report<'a> {
    parser: &'static str,
    mode: OutputMode,
    source_sha256: &'a str,
    source_size_bytes: usize,
    parse_elapsed_ms: f64,
    limits: ParseLimits,
    document: &'a DocumentInfo,
    artifacts: Vec<ArtifactReport>,
    limitations: [&'static str; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum OutputMode {
    All,
    MetadataOnly,
    PreviewOnly,
}

#[derive(Debug)]
struct Options {
    input: PathBuf,
    output: PathBuf,
    limits: ParseLimits,
    mode: OutputMode,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactReport {
    file: String,
    layer_id: Option<LayerId>,
    capability: Capability,
    decode_png_elapsed_ms: Option<f64>,
    write_elapsed_ms: Option<f64>,
    #[serde(flatten)]
    outcome: ArtifactOutcome,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum ArtifactOutcome {
    Exported { bytes: usize },
    Skipped { reason: String },
    Failed { code: String, message: String },
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("部分 PNG 输出失败，详情见输出目录中的 report.json。");
            ExitCode::FAILURE
        }
        Err(error) => {
            if let Some(error) = error.downcast_ref::<PsdError>() {
                eprintln!("PSD 解析失败 [{:?}]：{error}", error.code);
            } else {
                eprintln!("PSD 实验失败：{error}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let Options {
        input,
        output,
        limits,
        mode,
    } = parse_args(&args)?;
    let started = Instant::now();
    let document = PsdDocument::open(&input, limits)?;
    let parse_elapsed_ms = elapsed_ms(started);

    // 由操作系统原子创建目录；既有目录、文件及源文件本身均不能成为覆盖目标。
    fs::create_dir(&output)?;
    let info = document.info();
    let mut artifacts = Vec::new();
    if mode != OutputMode::MetadataOnly {
        artifacts.push(export_artifact(
            &output,
            "preview.png".into(),
            None,
            &info.preview,
            || document.preview_png(),
        ));
    }
    if mode == OutputMode::All {
        for layer in &info.layers {
            artifacts.push(export_artifact(
                &output,
                format!("layer-{}.png", layer.id.0),
                Some(layer.id),
                &layer.export,
                || document.layer_png(layer.id),
            ));
        }
    }

    let exported = artifacts
        .iter()
        .filter(|artifact| matches!(artifact.outcome, ArtifactOutcome::Exported { .. }))
        .count();
    let succeeded = !artifacts
        .iter()
        .any(|artifact| matches!(artifact.outcome, ArtifactOutcome::Failed { .. }));
    let report = Report {
        parser: PARSER_BUILD,
        mode,
        source_sha256: document.source_sha256(),
        source_size_bytes: document.source_size_bytes(),
        parse_elapsed_ms,
        limits,
        document: info,
        artifacts,
        limitations: [
            "按 mode 输出；预览允许 partial 并保留限制，独立素材仅导出 supported 项。",
            "耗时为本次进程内墙钟记录，解析包含固定源数据与指纹计算；不代表稳定性能基线。",
            "当前实验未测量峰值内存；真实 PSD 准确性和其他平台兼容性仍需独立验收。",
        ],
    };
    let mut json = serde_json::to_vec_pretty(&report)?;
    json.push(b'\n');
    write_new(&output.join("report.json"), &json)?;
    println!("已写入 report.json，成功输出 {exported} 项 PNG。");
    Ok(succeeded)
}

fn invalid_argument(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn positive_integer(value: &OsString, option: &str) -> io::Result<u64> {
    let value = value
        .to_str()
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| invalid_argument(format!("{option} 必须使用正整数")))?;
    value
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid_argument(format!("{option} 必须在正整数 u64 范围内")))
}

fn mebibytes(value: u64, option: &str) -> io::Result<u64> {
    value
        .checked_mul(1024 * 1024)
        .ok_or_else(|| invalid_argument(format!("{option} 换算为字节后超出 u64 范围")))
}

fn parse_args(args: &[OsString]) -> io::Result<Options> {
    let [input, output, options @ ..] = args else {
        return Err(invalid_argument(
            "用法：inspect_psd INPUT.psd OUTPUT_DIR [--max-file-mib N] [--max-total-pixels N] \
             [--max-decoded-mib N] [--max-layers N] [--metadata-only | --preview-only]",
        ));
    };
    let mut limits = ParseLimits::default();
    let mut mode = OutputMode::All;
    let mut seen = HashSet::new();
    let mut options = options.iter();
    while let Some(option) = options.next() {
        let option = option
            .to_str()
            .ok_or_else(|| invalid_argument("选项必须使用有效文本"))?;
        if !seen.insert(option) {
            return Err(invalid_argument(format!("重复选项：{option}")));
        }
        if matches!(option, "--metadata-only" | "--preview-only") {
            if mode != OutputMode::All {
                return Err(invalid_argument("--metadata-only 与 --preview-only 互斥"));
            }
            mode = if option == "--metadata-only" {
                OutputMode::MetadataOnly
            } else {
                OutputMode::PreviewOnly
            };
            continue;
        }
        if !matches!(
            option,
            "--max-file-mib" | "--max-total-pixels" | "--max-decoded-mib" | "--max-layers"
        ) {
            return Err(invalid_argument(format!("未知选项：{option}")));
        }
        let value = options
            .next()
            .ok_or_else(|| invalid_argument(format!("{option} 缺少数值")))?;
        let value = positive_integer(value, option)?;
        match option {
            "--max-file-mib" => limits.max_file_bytes = mebibytes(value, option)?,
            "--max-total-pixels" => limits.max_total_pixels = value,
            "--max-decoded-mib" => limits.max_decoded_bytes = mebibytes(value, option)?,
            "--max-layers" => {
                limits.max_layers = u32::try_from(value)
                    .map_err(|_| invalid_argument("--max-layers 超出 u32 范围"))?;
            }
            _ => unreachable!("上方已限定预算选项集合"),
        }
    }
    Ok(Options {
        input: input.into(),
        output: output.into(),
        limits,
        mode,
    })
}

fn export_artifact(
    directory: &Path,
    file: String,
    layer_id: Option<LayerId>,
    capability: &Capability,
    decode_png: impl FnOnce() -> Result<Vec<u8>, PsdError>,
) -> ArtifactReport {
    let mut report = ArtifactReport {
        file,
        layer_id,
        capability: capability.clone(),
        decode_png_elapsed_ms: None,
        write_elapsed_ms: None,
        outcome: ArtifactOutcome::Skipped {
            reason: capability.reason.clone(),
        },
    };
    if capability.status == Support::Unsupported
        || (layer_id.is_some() && capability.status != Support::Supported)
    {
        return report;
    }

    let started = Instant::now();
    let decoded = decode_png();
    report.decode_png_elapsed_ms = Some(elapsed_ms(started));
    let bytes = match decoded {
        Ok(bytes) => bytes,
        Err(error) => {
            report.outcome = ArtifactOutcome::Failed {
                code: format!("{:?}", error.code),
                message: describe_error(&error),
            };
            return report;
        }
    };

    let started = Instant::now();
    let written = write_new(&directory.join(&report.file), &bytes);
    report.write_elapsed_ms = Some(elapsed_ms(started));
    report.outcome = match written {
        Ok(()) => ArtifactOutcome::Exported { bytes: bytes.len() },
        Err(error) => ArtifactOutcome::Failed {
            code: "Io".into(),
            message: format!("写入 PNG 失败：{}", describe_error(&error)),
        },
    };
    report
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create_new(path)?;
    file.write_all(bytes)
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn describe_error(error: &dyn Error) -> String {
    let mut message = error.to_string();
    let mut cause = error.source();
    while let Some(error) = cause {
        message.push_str("；原因：");
        message.push_str(&error.to_string());
        cause = error.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(options: &[&str]) -> Vec<OsString> {
        ["输入.psd", "输出目录"]
            .into_iter()
            .chain(options.iter().copied())
            .map(OsString::from)
            .collect()
    }

    #[test]
    fn defaults_and_explicit_budgets_keep_units_and_paths() {
        let defaults = parse_args(&arguments(&[])).unwrap();
        assert_eq!(defaults.input, Path::new("输入.psd"));
        assert_eq!(defaults.output, Path::new("输出目录"));
        assert_eq!(defaults.mode, OutputMode::All);
        assert_eq!(
            serde_json::to_value(defaults.limits).unwrap(),
            serde_json::to_value(ParseLimits::default()).unwrap()
        );

        let explicit = parse_args(&arguments(&[
            "--max-file-mib",
            "512",
            "--max-total-pixels",
            "80000000",
            "--max-decoded-mib",
            "256",
            "--max-layers",
            "8192",
            "--preview-only",
        ]))
        .unwrap();
        assert_eq!(explicit.limits.max_file_bytes, 512 * 1024 * 1024);
        assert_eq!(explicit.limits.max_total_pixels, 80_000_000);
        assert_eq!(explicit.limits.max_decoded_bytes, 256 * 1024 * 1024);
        assert_eq!(explicit.limits.max_layers, 8192);
        assert_eq!(explicit.mode, OutputMode::PreviewOnly);
        assert_eq!(
            parse_args(&arguments(&["--metadata-only"])).unwrap().mode,
            OutputMode::MetadataOnly
        );
    }

    #[test]
    fn budgets_reject_nonpositive_noninteger_missing_and_overflow_values() {
        for option in [
            "--max-file-mib",
            "--max-total-pixels",
            "--max-decoded-mib",
            "--max-layers",
        ] {
            for value in ["0", "-1", "+1", "1.5", "", " 1", "18446744073709551616"] {
                let error = parse_args(&arguments(&[option, value])).unwrap_err();
                assert_eq!(
                    error.kind(),
                    io::ErrorKind::InvalidInput,
                    "{option} {value}"
                );
            }
            assert!(parse_args(&arguments(&[option])).is_err());
            assert!(parse_args(&arguments(&[option, "1", option, "2"])).is_err());
        }
        for option in ["--max-file-mib", "--max-decoded-mib"] {
            assert!(parse_args(&arguments(&[option, "17592186044416"])).is_err());
            assert!(parse_args(&arguments(&[option, "17592186044415"])).is_ok());
        }
        assert!(parse_args(&arguments(&["--max-layers", "4294967296"])).is_err());
        assert!(parse_args(&arguments(&["--max-layers", "4294967295"])).is_ok());
    }

    #[test]
    fn unknown_duplicate_and_conflicting_modes_are_rejected() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&[OsString::from("input.psd")]).is_err());
        for options in [
            vec!["--unknown"],
            vec!["extra-positional"],
            vec!["--metadata-only", "--metadata-only"],
            vec!["--preview-only", "--preview-only"],
            vec!["--metadata-only", "--preview-only"],
            vec!["--preview-only", "--metadata-only"],
            vec!["--max-file-mib", "--preview-only"],
        ] {
            assert!(parse_args(&arguments(&options)).is_err(), "{options:?}");
        }
    }
}

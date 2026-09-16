//! M0-01 同步解析实验入口：只读 PSD，向全新目录输出报告与已支持的 PNG。
//!
//! 用法：`cargo run -p layerlens-core --example inspect_psd -- INPUT.psd OUTPUT_DIR`。
//! 输出目录必须不存在，父目录须预先创建；单项导出失败仍记录其余结果并返回非零。

use std::{
    env,
    error::Error,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

use layerlens_core::psd::{
    Capability, DocumentInfo, LayerId, ParseLimits, PsdDocument, PsdError, Support,
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report<'a> {
    parser: ParserInfo,
    source_sha256: &'a str,
    source_size_bytes: usize,
    parse_elapsed_ms: f64,
    limits: LimitsReport,
    document: &'a DocumentInfo,
    artifacts: Vec<ArtifactReport>,
    limitations: [&'static str; 3],
}

#[derive(Serialize)]
struct ParserInfo {
    name: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LimitsReport {
    max_file_bytes: u64,
    max_total_pixels: u64,
    max_layers: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactReport {
    file: String,
    layer_id: Option<LayerId>,
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
    let (input, output) = match args.as_slice() {
        [input, output] => (PathBuf::from(input), PathBuf::from(output)),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "用法：cargo run -p layerlens-core --example inspect_psd -- INPUT.psd OUTPUT_DIR",
            )
            .into());
        }
    };

    let limits = ParseLimits::default();
    let started = Instant::now();
    let document = PsdDocument::open(&input, limits)?;
    let parse_elapsed_ms = elapsed_ms(started);

    // 由操作系统原子创建目录；既有目录、文件及源文件本身均不能成为覆盖目标。
    fs::create_dir(&output)?;
    let info = document.info();
    let mut artifacts = vec![export_artifact(
        &output,
        "preview.png".into(),
        None,
        &info.preview,
        || document.preview_png(),
    )];
    for layer in &info.layers {
        artifacts.push(export_artifact(
            &output,
            format!("layer-{}.png", layer.id.0),
            Some(layer.id),
            &layer.export,
            || document.layer_png(layer.id),
        ));
    }

    let exported = artifacts
        .iter()
        .filter(|artifact| matches!(artifact.outcome, ArtifactOutcome::Exported { .. }))
        .count();
    let succeeded = !artifacts
        .iter()
        .any(|artifact| matches!(artifact.outcome, ArtifactOutcome::Failed { .. }));
    let report = Report {
        parser: ParserInfo {
            name: "ag-psd",
            version: "0.3.0",
        },
        source_sha256: document.source_sha256(),
        source_size_bytes: document.source_size_bytes(),
        parse_elapsed_ms,
        limits: LimitsReport {
            max_file_bytes: limits.max_file_bytes,
            max_total_pixels: limits.max_total_pixels,
            max_layers: limits.max_layers,
        },
        document: info,
        artifacts,
        limitations: [
            "仅导出 supported 项；partial 和 unsupported 保留原因并标记 skipped。",
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
        decode_png_elapsed_ms: None,
        write_elapsed_ms: None,
        outcome: ArtifactOutcome::Skipped {
            reason: capability.reason.clone(),
        },
    };
    if capability.status != Support::Supported {
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

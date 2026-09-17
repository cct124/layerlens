//! 由 Windows 测量脚本驱动的实验会话：检查点握手供父进程读取 OS 资源计量。
//!
//! 每轮重新打开并显式释放文档；保留小型汇总，绝不复制完整文档到性能日志。

use super::{summary::DocumentSummary, *};
use std::io::{BufRead, Read};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunReport {
    schema_version: u32,
    run: u32,
    parser: &'static str,
    build_mode: &'static str,
    mode: OutputMode,
    source_sha256: String,
    limits: ParseLimits,
    open_timings: OpenTimings,
    document_release_ms: f64,
    summary: DocumentSummary,
    artifacts: Vec<ArtifactReport>,
    succeeded: bool,
}

pub(super) fn run(options: &Options, runs: u32) -> Result<bool, Box<dyn Error>> {
    fs::create_dir(&options.output)?;
    checkpoint(0, "ready")?;
    let mut fingerprint = None;
    for run in 1..=runs {
        let (document, open_timings) = PsdDocument::open_measured(&options.input, options.limits)?;
        if fingerprint
            .as_ref()
            .is_some_and(|value| value != document.source_sha256())
        {
            return Err(invalid_argument("重复测量期间源指纹发生变化，基线无效").into());
        }
        fingerprint = Some(document.source_sha256().to_owned());
        checkpoint(run, "opened")?;
        let output = options.output.join(format!("run-{run:03}"));
        fs::create_dir(&output)?;
        let mut artifacts = export_artifacts(&document, &output, options.mode);
        // 当前没有解码缓存；此项测的是同一固定文档的再次解码与编码，不称为缓存命中。
        if options.mode != OutputMode::MetadataOnly {
            artifacts.push(export_artifact(
                &output,
                "preview-repeat.png".into(),
                None,
                &document.info().preview,
                || document.preview_png_measured(),
            ));
        }
        checkpoint(run, "exported")?;
        let summary = DocumentSummary::from_info(document.info());
        let source_sha256 = document.source_sha256().to_owned();
        let started = Instant::now();
        drop(document);
        let document_release_ms = elapsed_ms(started);
        let succeeded = artifacts_succeeded(&artifacts);
        let report = RunReport {
            schema_version: 1,
            run,
            parser: PARSER_BUILD,
            build_mode: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            mode: options.mode,
            source_sha256,
            limits: options.limits,
            open_timings,
            document_release_ms,
            summary,
            artifacts,
            succeeded,
        };
        write_new(
            &output.join("report.json"),
            &serde_json::to_vec_pretty(&report)?,
        )?;
        drop(report);
        checkpoint(run, "released")?;
        if !succeeded {
            return Ok(false);
        }
    }
    Ok(true)
}

/// 检查点等待时间不在核心阶段耗时内；父进程负责超时、资源终止和异常清理。
fn checkpoint(run: u32, phase: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "{}",
        serde_json::json!({"run": run, "phase": phase})
    )?;
    stdout.flush()?;
    let mut acknowledgement = String::new();
    io::stdin()
        .lock()
        .take(32)
        .read_line(&mut acknowledgement)?;
    if acknowledgement != "continue\n" && acknowledgement != "continue\r\n" {
        return Err(invalid_argument(
            "测量会话需要父进程检查点确认；请使用 measure-psd.ps1",
        ));
    }
    Ok(())
}

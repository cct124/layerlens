//! 本机只读生命周期烟测：多个输入共用一份服务，核对关闭与保留修订的资源归还。
//! 输出 JSON Lines，不输出 PSD 文字或像素，不写入任何设计稿或派生资源。

use std::{error::Error, path::PathBuf};

use layerlens_core::documents::{
    DocumentService, DocumentServiceConfig, OpenOutcome, ResourceAccounting,
};
use serde_json::json;

fn accounting(stage: &str, resources: ResourceAccounting) {
    println!(
        "{}",
        json!({
            "stage": stage, "liveRevisions": resources.live_revisions,
            "sourceBytes": resources.source_bytes, "declaredPixels": resources.declared_pixels,
        })
    );
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut config = DocumentServiceConfig::default();
    let mut paths = Vec::new();
    for argument in std::env::args_os().skip(1) {
        if argument == "--large" {
            // 显式实验配置，容纳 M0 私有大稿与声明像素较多的公开长页。
            config.parse_limits.max_file_bytes = 512 * 1024 * 1024;
            config.parse_limits.max_total_pixels = 128 * 1024 * 1024;
            config.max_source_bytes = 1024 * 1024 * 1024;
            config.max_declared_pixels = 384 * 1024 * 1024;
        } else if argument.to_string_lossy().starts_with("--") {
            return Err("未知选项；用法：document_lifecycle [--large] <PSD 路径>...".into());
        } else {
            paths.push(PathBuf::from(argument));
        }
    }
    if paths.is_empty() {
        return Err("用法：document_lifecycle [--large] <PSD 路径>...".into());
    }
    let mut service = DocumentService::new(config)?;
    let mut retained = None;
    let mut failed = false;
    for path in paths {
        let job = service.open(&path)?;
        let result = job.wait();
        match &result.outcome {
            OpenOutcome::Opened {
                document_id,
                revision_id,
                reused,
            } => {
                let lease = service.lease(*document_id)?;
                println!(
                    "{}",
                    json!({
                    "stage": "opened", "file": path.file_name().map(|name| name.to_string_lossy()), "jobId": job.id().get(),
                        "documentId": document_id.get(), "revisionId": revision_id.get(), "reused": reused,
                        "width": lease.info().width, "height": lease.info().height, "layers": lease.info().layers.len(),
                        "queuedMs": result.queued_for.as_secs_f64() * 1000.0,
                        "executedMs": result.executed_for.as_secs_f64() * 1000.0,
                    })
                );
                if retained.is_none() {
                    retained = Some(lease);
                }
            }
            OpenOutcome::Failed(error) => {
                println!(
                    "{}",
                    json!({ "stage": "failed", "jobId": job.id().get(), "error": error.to_string() })
                );
                failed = true;
            }
            OpenOutcome::Cancelled => {
                println!(
                    "{}",
                    json!({ "stage": "cancelled", "jobId": job.id().get() })
                );
                failed = true;
            }
        }
    }
    accounting("all-open", service.snapshot().resources);
    for document in service.snapshot().documents {
        service.close(document.id)?;
    }
    let held = service.snapshot().resources;
    accounting("closed-with-lease", held);
    if held.live_revisions != u64::from(retained.is_some()) {
        return Err("关闭后的修订计费不符".into());
    }
    drop(retained);
    service.shutdown()?;
    let released = service.snapshot().resources;
    accounting("released", released);
    if released != ResourceAccounting::default() {
        return Err("资源额度未完整归还".into());
    }
    if failed {
        return Err("一个或多个输入未能打开".into());
    }
    Ok(())
}

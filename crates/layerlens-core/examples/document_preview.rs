//! 只读应用预览烟测。JSON Lines 只含指纹、尺寸、计时和资源计数，不输出文字／像素。
//! --checkpoint 在每条记录后等待 stdin 换行，供外部进程采集 OS 内存；计时不含等待。

use layerlens_core::{
    documents::{
        DocumentService, DocumentServiceConfig, OpenOutcome, PreviewAccounting, PreviewOutcome,
        ResourceAccounting,
    },
    psd::PARSER_BUILD,
};
use serde_json::{Value, json};
use std::{
    error::Error,
    io::{self, Write},
    path::PathBuf,
    time::Instant,
};

fn emit(
    mut value: Value,
    service: &DocumentService,
    checkpoint: bool,
) -> Result<(), Box<dyn Error>> {
    let snapshot = service.snapshot();
    value["resources"] = json!({
        "revisions": snapshot.resources.live_revisions, "sourceBytes": snapshot.resources.source_bytes,
        "declaredPixels": snapshot.resources.declared_pixels, "decodedBytes": snapshot.previews.resources.decoded_bytes,
        "outputBytes": snapshot.previews.resources.output_bytes, "cacheBytes": snapshot.previews.cache_bytes,
        "cacheEntries": snapshot.previews.cache_entries, "cacheHits": snapshot.previews.cache_hits, "evictions": snapshot.previews.evictions,
    });
    println!("{value}");
    if checkpoint {
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            return Err("检查点缺少确认".into());
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut config = DocumentServiceConfig::default();
    let mut checkpoint = false;
    let mut paths = Vec::new();
    for argument in std::env::args_os().skip(1) {
        if argument == "--large" {
            config.parse_limits.max_file_bytes = 512 * 1024 * 1024;
            config.parse_limits.max_total_pixels = 128 * 1024 * 1024;
            config.max_source_bytes = 1024 * 1024 * 1024;
            config.max_declared_pixels = 384 * 1024 * 1024;
        } else if argument == "--checkpoint" {
            checkpoint = true;
        } else if argument.to_string_lossy().starts_with("--") {
            return Err("未知选项".into());
        } else {
            paths.push(PathBuf::from(argument));
        }
    }
    if paths.is_empty() {
        return Err("用法：document_preview [--large] [--checkpoint] <PSD 路径>...".into());
    }
    let mut service = DocumentService::new(config)?;
    emit(
        json!({ "stage": "baseline", "parser": PARSER_BUILD, "config": {
            "maxFileBytes": config.parse_limits.max_file_bytes, "maxTotalPixels": config.parse_limits.max_total_pixels,
            "maxLayers": config.parse_limits.max_layers, "maxDecodedBytes": config.parse_limits.max_decoded_bytes,
            "maxSourceBytes": config.max_source_bytes, "maxDeclaredPixels": config.max_declared_pixels,
            "maxPendingJobs": config.max_pending_jobs, "maxLiveRevisions": config.max_live_revisions,
            "maxPreviewDecodedBytes": config.preview.max_decoded_bytes, "maxPngBytes": config.preview.max_png_bytes,
            "maxOutputBytes": config.preview.max_output_bytes, "maxCacheBytes": config.preview.max_cache_bytes, "maxCacheEntries": config.preview.max_cache_entries,
        }}),
        &service,
        checkpoint,
    )?;
    let mut held = Vec::new();
    for path in paths {
        let opened = service.open(&path)?.wait();
        let OpenOutcome::Opened { document_id, .. } = opened.outcome else {
            return Err(format!("打开失败：{:?}", opened.outcome).into());
        };
        let source = service.lease(document_id)?;
        emit(
            json!({ "stage": "opened", "file": path.file_name().map(|name| name.to_string_lossy()),
            "sha256": source.source_sha256(), "width": source.info().width, "height": source.info().height,
            "layers": source.info().layers.len(), "executedMs": opened.executed_for.as_secs_f64()*1000.0 }),
            &service,
            checkpoint,
        )?;
        drop(source);
        for request in 0..2 {
            let started = Instant::now();
            let result = service.preview(document_id)?.wait();
            let elapsed = started.elapsed();
            let PreviewOutcome::Ready(image) = &result.outcome else {
                return Err(format!("预览失败：{:?}", result.outcome).into());
            };
            if result.cache_hit != (request == 1) {
                return Err("首次请求／缓存命中状态不符".into());
            }
            emit(
                json!({ "stage": if request == 0 { "rendered" } else { "cache-hit" },
                    "documentId": document_id.get(), "revisionId": image.revision_id().get(),
                    "pngBytes": image.png().len(), "chargedBytes": image.charged_bytes(), "capability": image.capability(),
                    "requestMs": elapsed.as_secs_f64()*1000.0, "queuedMs": result.queued_for.as_secs_f64()*1000.0,
                    "executedMs": result.executed_for.as_secs_f64()*1000.0, "pngTimings": result.png_timings,
                }),
                &service,
                checkpoint,
            )?;
            if request == 0 {
                held.push(image.clone());
            }
        }
    }
    for document in service.snapshot().documents {
        service.close(document.id)?;
    }
    emit(json!({ "stage": "closed-with-png" }), &service, checkpoint)?;
    if service.snapshot().resources != ResourceAccounting::default() {
        return Err("PNG 不应保留源修订".into());
    }
    drop(held);
    service.shutdown()?;
    emit(json!({ "stage": "released" }), &service, checkpoint)?;
    if service.snapshot().previews.resources != PreviewAccounting::default() {
        return Err("预览额度未归还".into());
    }
    Ok(())
}

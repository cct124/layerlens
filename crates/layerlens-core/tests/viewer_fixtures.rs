//! 默认桌面预算下的规模／文字分页回归；预期来自人工 fixture，不从解析器反推。
#![cfg(windows)]

use layerlens_core::{
    documents::{
        DocumentLease, DocumentService, OpenOutcome, PreviewAccounting, PreviewOutcome,
        ResourceAccounting,
    },
    psd::{Bounds, LayerId, LayerKind, Support, TextIndexMapping, TextUnit},
};
use serde_json::Value;
use std::{path::Path, time::Duration};

fn open(service: &DocumentService, name: &str) -> DocumentLease {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd")
        .join(name);
    let result = service
        .open(path)
        .unwrap()
        .wait_timeout(Duration::from_secs(10))
        .unwrap();
    let OpenOutcome::Opened { document_id, .. } = result.outcome else {
        panic!("默认额度打开失败：{result:?}")
    };
    service.lease(document_id).unwrap()
}

fn close_and_check(mut service: DocumentService, lease: DocumentLease) {
    service.close(lease.document_id()).unwrap();
    drop(lease);
    service.shutdown().unwrap();
    let snapshot = service.snapshot();
    assert!(snapshot.documents.is_empty());
    assert_eq!(snapshot.resources, ResourceAccounting::default());
    assert_eq!(snapshot.previews.resources, PreviewAccounting::default());
}

#[test]
fn default_budget_pages_all_512_layers_and_releases_the_saved_preview() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service, "viewer-scale.psd");
    assert_eq!((lease.info().width, lease.info().height), (256, 128));
    for page_index in 0..4 {
        let offset = page_index * 128;
        let page = lease.layers(offset, 128).unwrap();
        assert_eq!(page.offset, offset);
        assert_eq!(page.total, 512);
        assert_eq!(page.layers.len(), 128);
        assert_eq!(page.next_offset, (page_index < 3).then_some(offset + 128));
        assert!(serde_json::to_vec(&page).unwrap().len() <= 256 * 1024);
        for (index, layer) in page.layers.iter().enumerate() {
            let position = offset + index as u32;
            // 文件记录底到顶；查看器按顶到底呈现，分页不能漏掉或重复节点。
            let tile = 511 - position;
            assert_eq!(layer.id, LayerId(position));
            assert_eq!(layer.name, format!("tile-{tile:03}"));
            assert_eq!(layer.parent_id, None);
            assert_eq!(layer.kind, LayerKind::Bitmap);
            assert!(layer.visible && layer.effective_visible);
            assert_eq!(layer.opacity, 255);
            assert_eq!(
                layer.bounds,
                Bounds {
                    x: (tile % 32 * 8) as i32,
                    y: (tile / 32 * 8) as i32,
                    width: 8,
                    height: 8,
                }
            );
        }
    }
    service.select_layer(&lease, Some(LayerId(511))).unwrap();
    assert_eq!(
        service.snapshot().documents[0].selected_layer,
        Some(LayerId(511))
    );
    assert_eq!(
        lease.layer_details(LayerId(511), 0).unwrap().layer.name,
        "tile-000"
    );
    let result = service
        .preview(lease.document_id())
        .unwrap()
        .wait_timeout(Duration::from_secs(10))
        .unwrap();
    let PreviewOutcome::Ready(image) = &result.outcome else {
        panic!("预览失败：{result:?}")
    };
    let mut reader = png::Decoder::new(image.png()).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((frame.width, frame.height), (256, 128));
    assert_eq!(frame.color_type, png::ColorType::Rgba);
    for (index, pixel) in pixels[..frame.buffer_size()].chunks_exact(4).enumerate() {
        let column = (index % 256 / 8) as u8;
        let row = (index / 256 / 8) as u8;
        assert_eq!(pixel, [32 + column * 7, 48 + row * 11, 160, 255]);
    }
    drop(reader);
    drop(result);
    close_and_check(service, lease);
}

#[test]
fn default_budget_reconstructs_text_across_unit_run_and_byte_limits() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service, "viewer-text.psd");
    let manifest: Value = serde_json::from_str(include_str!("fixtures/psd/manifest.json")).unwrap();
    let fixture = manifest["samples"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sample| sample["file"] == "viewer-text.psd")
        .unwrap();
    let layers = lease.layers(0, 128).unwrap();
    assert_eq!(layers.total, 3);
    for case in fixture["textCases"].as_array().unwrap() {
        let layer = layers
            .layers
            .iter()
            .find(|layer| layer.name == case["name"])
            .unwrap();
        assert_eq!(layer.kind, LayerKind::Text);
        assert_eq!(
            serde_json::to_value(layer.bounds).unwrap(),
            fixture["bounds"]
        );
        let expected = &case["expected"];
        let total = expected["utf16Length"].as_u64().unwrap() as u32;
        let mut ranges = vec![];
        let mut end = 0;
        for length in expected["styleRunLengths"].as_array().unwrap() {
            let start = end;
            end += length.as_u64().unwrap() as u32;
            ranges.push((start, end));
        }
        assert_eq!(end, total);
        let mut rebuilt = String::new();
        let mut start = 0;
        let mut page_count = 0;
        loop {
            let details = lease.layer_details(layer.id, start).unwrap();
            assert!(serde_json::to_vec(&details).unwrap().len() <= 256 * 1024);
            let text = details.text.unwrap();
            assert_eq!(text.start, start);
            assert_eq!(text.total_length, total);
            assert!(text.end > start && text.end - start <= 2048);
            assert_eq!(text.text.encode_utf16().count() as u32, text.end - start);
            assert_eq!(text.index_mapping, Some(TextIndexMapping::Exact));
            assert_eq!(text.styles.status, Support::Partial);
            let transform: [f64; 6] =
                serde_json::from_value(fixture["textTransform"].clone()).unwrap();
            assert_eq!(text.transform, Some(transform));
            if start == 0 {
                match layer.name.as_str() {
                    "long-text" => assert_eq!(text.end, 2047),
                    "many-style-runs" => assert_eq!(text.end, 512),
                    "large-styles" => assert!(text.end < total),
                    _ => panic!("未声明的文字场景"),
                }
            }
            let runs = text.style_runs.as_ref().unwrap();
            let overlapping: Vec<_> = ranges
                .iter()
                .enumerate()
                .filter(|(_, (a, b))| *a < text.end && *b > start)
                .collect();
            assert_eq!(runs.len(), overlapping.len());
            assert!(runs.len() <= 128);
            for (run, (index, &(a, b))) in runs.iter().zip(overlapping) {
                // 相交段保留完整原文的绝对范围，不裁成页内坐标。
                assert_eq!((run.start, run.end), (a, b));
                assert!(serde_json::to_vec(run).unwrap().len() <= 32 * 1024);
                assert_eq!(
                    run.style.font.as_ref().unwrap().value.name,
                    expected["fontName"]
                );
                let size = run.style.font_size.as_ref().unwrap();
                assert_eq!(size.value.value, 18.0 + (index % 2) as f64);
                assert_eq!(size.value.unit, TextUnit::UnverifiedEngine);
                assert!(size.source.ends_with(".FontSize"));
                assert_eq!(run.style.fill_color.as_ref().unwrap().value.red, 0.125);
            }
            let paragraphs = text.paragraph_runs.as_ref().unwrap();
            assert_eq!(paragraphs.len(), 1);
            assert_eq!((paragraphs[0].start, paragraphs[0].end), (0, total));
            rebuilt.push_str(&text.text);
            page_count += 1;
            assert!(page_count <= total); // 防止续页游标不前进导致测试无限循环。
            match text.next_start {
                Some(next) => {
                    assert_eq!(next, text.end);
                    assert!(next > start && next < total);
                    start = next;
                }
                None => {
                    assert_eq!(text.end, total);
                    break;
                }
            }
        }
        assert!(page_count > 1);
        assert_eq!(rebuilt, expected["rawText"]);
    }
    close_and_check(service, lease);
}

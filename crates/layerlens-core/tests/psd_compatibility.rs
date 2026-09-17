//! 以独立编码的公开样本验证兼容性降级、组结构、合成透明度和原文保真。

use std::{fs, io::Cursor, path::PathBuf};

use layerlens_core::psd::{
    ExportBlocker, LayerKind, ParseLimits, PsdDocument, PsdErrorCode, Support,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/psd")
            .join(name),
    )
    .expect("读取公开合成样本")
}

fn expected(name: &str) -> Value {
    let manifest: Value =
        serde_json::from_str(include_str!("fixtures/psd/manifest.json")).expect("合法独立样本清单");
    manifest["samples"]
        .as_array()
        .expect("样本数组")
        .iter()
        .find(|sample| sample["file"] == name)
        .expect("样本有明确预期")
        .clone()
}

fn parse(name: &str) -> PsdDocument {
    PsdDocument::from_bytes(&fixture(name), ParseLimits::default())
        .expect("有界且结构有效的样本可读取")
}

fn assert_pixels(document: &PsdDocument, expected: &Value) {
    let png = document.preview_png().expect("保存时合成图可按需解码");
    let mut reader = png::Decoder::new(Cursor::new(png))
        .read_info()
        .expect("有效 PNG");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("解码完整 PNG");
    assert_eq!(u64::from(info.width), expected["width"].as_u64().unwrap());
    assert_eq!(u64::from(info.height), expected["height"].as_u64().unwrap());
    assert_eq!(info.color_type, png::ColorType::Rgba);
    let expected_pixels: Vec<u8> = expected["rgbaHex"]
        .as_str()
        .expect("人工定义的 RGBA")
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    assert_eq!(&pixels[..info.buffer_size()], expected_pixels);
}

#[test]
fn opaque_resources_preserve_following_dpi_and_report_their_limits() {
    let document = parse("resources-groups-alpha-raw.psd");
    let info = document.info();
    assert_eq!(info.resolution_dpi, Some([72.0, 72.0]));
    assert_eq!(info.layers.len(), 3);
    assert_eq!(info.resources.layer_records, 4);
    let profile = info.color_profile.as_ref().expect("记录存在的 ICC 载荷");
    assert_eq!(profile.byte_length, 15);
    assert_eq!(
        profile.sha256,
        format!("{:x}", Sha256::digest(b"ICC-unvalidated"))
    );
    assert_eq!(profile.conversion.status, Support::Unsupported);
    assert_ne!(info.preview.status, Support::Unsupported);
    assert_ne!(
        info.preview.status,
        Support::Supported,
        "未验证 ICC 不得声称完整颜色保真"
    );
    for resource_id in ["1028", "1039"] {
        assert!(
            info.diagnostics.iter().any(|diagnostic| {
                diagnostic.tag.contains(resource_id)
                    && diagnostic.offset > 0
                    && !diagnostic.message.is_empty()
            }),
            "资源 {resource_id} 必须留下可定位的诊断"
        );
    }
}

#[test]
fn hidden_parent_preserves_child_own_visibility_and_prevents_export() {
    let name = "resources-groups-alpha-raw.psd";
    let document = parse(name);
    let info = document.info();
    let expected = expected(name);
    for source in expected["layers"].as_array().unwrap() {
        let layer = info
            .layers
            .iter()
            .find(|layer| layer.name == source["name"])
            .unwrap();
        assert_eq!(
            serde_json::to_value(layer.bounds).unwrap(),
            source["bounds"]
        );
        assert_eq!(layer.visible, source["visible"].as_bool().unwrap());
        assert_eq!(
            layer.effective_visible,
            source["effectiveVisible"].as_bool().unwrap()
        );
        match source["parentName"].as_str() {
            Some(parent_name) => {
                let parent = info
                    .layers
                    .iter()
                    .find(|parent| parent.name == parent_name)
                    .unwrap();
                assert_eq!(layer.parent_id, Some(parent.id));
                assert_eq!(parent.kind, LayerKind::Group);
            }
            None => assert_eq!(layer.parent_id, None),
        }
        if !layer.effective_visible {
            assert!(layer.export_blockers.contains(&ExportBlocker::Hidden));
            assert_eq!(layer.export.status, Support::Unsupported);
            assert_eq!(
                document.layer_png(layer.id).unwrap_err().code,
                PsdErrorCode::ExportUnsupported
            );
        }
    }
}

#[test]
fn deferred_raw_and_rle_composites_keep_global_alpha_and_remove_white_matte() {
    for name in [
        "resources-groups-alpha-raw.psd",
        "resources-groups-alpha-rle.psd",
    ] {
        let document = parse(name);
        // 人工预期含 alpha 0/85/170/255；半透明 RGB 必须去白底 matte，非预乘输出。
        assert_pixels(&document, &expected(name)["composite"]);
    }
}

#[test]
fn recognized_and_unknown_semantic_tags_never_become_plain_bitmap_assets() {
    let document = parse("metadata-text.psd");
    let expected = expected("metadata-text.psd");
    let info = document.info();
    assert_eq!(info.layers.len(), 5);
    for source in expected["layers"].as_array().unwrap() {
        let layer = info
            .layers
            .iter()
            .find(|layer| layer.name == source["name"])
            .unwrap();
        assert_eq!(serde_json::to_value(layer.kind).unwrap(), source["kind"]);
        assert_ne!(layer.export.status, Support::Supported);
        assert_eq!(
            document.layer_png(layer.id).unwrap_err().code,
            PsdErrorCode::ExportUnsupported
        );
    }
    for (layer_name, tag) in [
        ("unsupported-gradient", "GdFl"),
        ("unknown-semantic-tag", "zzzz"),
    ] {
        let layer = info
            .layers
            .iter()
            .find(|layer| layer.name == layer_name)
            .unwrap();
        assert!(layer.diagnostics.iter().any(|diagnostic| {
            diagnostic.tag == tag && diagnostic.offset > 0 && !diagnostic.message.is_empty()
        }));
    }
    assert_pixels(&document, &expected["composite"]);
}

#[test]
fn text_preserves_original_utf16_content_and_transform_without_inventing_styles() {
    let document = parse("metadata-text.psd");
    let expected = expected("metadata-text.psd");
    let layer = document
        .info()
        .layers
        .iter()
        .find(|layer| layer.name == "literal-text")
        .unwrap();
    let text = layer.text.as_ref().expect("TySh 原文元数据");
    assert_eq!(text.raw_text, expected["text"]["rawText"].as_str().unwrap());
    assert_eq!(
        u64::from(text.utf16_length),
        expected["text"]["utf16Length"].as_u64().unwrap()
    );
    let transform = text.transform.expect("PSD 显式仿射矩阵");
    for (actual, expected) in transform
        .iter()
        .zip(expected["text"]["transform"].as_array().unwrap())
    {
        assert_eq!(*actual, expected.as_f64().unwrap());
    }
    assert!(
        text.normalization_changed,
        "候选把 CR 改为 LF 的差异必须被发现"
    );
    assert_eq!(text.styles.status, Support::Unsupported);
    assert_eq!(document.info().text.status, Support::Partial);
}

fn tag_payload(bytes: &[u8], tag: &[u8; 4]) -> usize {
    let marker: Vec<u8> = b"8BIM".iter().chain(tag).copied().collect();
    bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("样本声明的附加块")
        + 12
}

fn assert_invalid(bytes: &[u8], expected: PsdErrorCode) {
    let error = PsdDocument::from_bytes(bytes, ParseLimits::default())
        .err()
        .expect("损坏内容不能降级成成功");
    assert_eq!(error.code, expected, "{error}");
}

#[test]
fn global_mask_diagnostic_points_to_source_and_blocks_independent_assets() {
    let mut bytes = fixture("bitmap-raw.psd");
    let resources_len = u32::from_be_bytes(bytes[30..34].try_into().unwrap()) as usize;
    let section_offset = 34 + resources_len;
    let section_len = u32::from_be_bytes(
        bytes[section_offset..section_offset + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let mask_offset = section_offset + 4 + section_len;
    // 原样本的 Layer and Mask 末尾是空 global mask，替换为最小 13 字节载荷。
    bytes[mask_offset - 4..mask_offset].copy_from_slice(&13_u32.to_be_bytes());
    bytes[section_offset..section_offset + 4]
        .copy_from_slice(&((section_len + 13) as u32).to_be_bytes());
    bytes.splice(mask_offset..mask_offset, [0; 13]);
    let document = PsdDocument::from_bytes(&bytes, ParseLimits::default()).unwrap();
    let diagnostic = document
        .info()
        .diagnostics
        .iter()
        .find(|d| d.tag == "globalLayerMask")
        .expect("全局蒙版限制有来源");
    assert_eq!(diagnostic.offset, mask_offset as u64);
    for layer in &document.info().layers {
        assert!(layer.export_blockers.contains(&ExportBlocker::GlobalMask));
        assert!(
            !layer
                .export_blockers
                .contains(&ExportBlocker::AncestorVisualDependency)
        );
        assert_eq!(
            layer.export_blockers.contains(&ExportBlocker::Hidden),
            !layer.effective_visible
        );
        assert_eq!(
            document.layer_png(layer.id).unwrap_err().code,
            PsdErrorCode::ExportUnsupported
        );
    }
    assert_pixels(&document, &expected("bitmap-raw.psd")["composite"]);
}

#[test]
fn document_blocks_use_four_byte_padding_and_preserve_following_blocks_and_pixels() {
    let original = fixture("bitmap-raw.psd");
    // 固定样本：File Header、空 Color Mode Data、Image Resources 后是 Layer and Mask。
    let resources_len = u32::from_be_bytes(original[30..34].try_into().unwrap()) as usize;
    let section_offset = 34 + resources_len;
    let section_len = u32::from_be_bytes(
        original[section_offset..section_offset + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let blocks_offset = section_offset + 4 + section_len;
    for signature in [b"8BIM", b"8B64"] {
        for length in 1_usize..=4 {
            let mut blocks = signature.to_vec();
            blocks.extend_from_slice(b"zz01");
            if signature == b"8B64" {
                blocks.extend_from_slice(&0_u32.to_be_bytes());
            }
            blocks.extend_from_slice(&(length as u32).to_be_bytes());
            blocks.extend(std::iter::repeat_n(0x61, length));
            let padding_offset = blocks.len();
            blocks.resize(blocks.len() + (4 - length % 4) % 4, 0);
            let next_block_offset = blocks_offset + blocks.len();
            blocks.extend_from_slice(b"8BIMzz02\0\0\0\0");
            let mut bytes = original.clone();
            bytes[section_offset..section_offset + 4]
                .copy_from_slice(&((section_len + blocks.len()) as u32).to_be_bytes());
            bytes.splice(blocks_offset..blocks_offset, blocks);
            let document = PsdDocument::from_bytes(&bytes, ParseLimits::default()).unwrap();
            assert_eq!(
                document.info().resources.global_additional_blocks,
                ["zz01", "zz02"]
            );
            let diagnostics = &document.info().diagnostics;
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.tag == "zz02" && d.offset == next_block_offset as u64)
            );
            assert_pixels(&document, &expected("bitmap-raw.psd")["composite"]);

            if length % 4 != 0 {
                bytes[blocks_offset + padding_offset] = 1;
                assert_invalid(&bytes, PsdErrorCode::InvalidInput);
            }
        }
    }
}

#[test]
fn malformed_known_metadata_and_resource_lengths_remain_fatal() {
    let original = fixture("metadata-text.psd");
    let tysh = tag_payload(&original, b"TySh");
    let mut invalid_version = original.clone();
    invalid_version[tysh..tysh + 2].copy_from_slice(&2_i16.to_be_bytes());
    assert_invalid(&invalid_version, PsdErrorCode::ParseFailed);

    let mut invalid_descriptor_version = original.clone();
    invalid_descriptor_version[tysh + 52..tysh + 56].copy_from_slice(&17_u32.to_be_bytes());
    assert_invalid(&invalid_descriptor_version, PsdErrorCode::ParseFailed);

    let gradient = tag_payload(&original, b"GdFl");
    let mut unbounded_descriptor = original;
    unbounded_descriptor[gradient + 16..gradient + 20].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_invalid(&unbounded_descriptor, PsdErrorCode::ParseFailed);

    let mut resource_overrun = fixture("resources-groups-alpha-raw.psd");
    // File Header 26 + color-mode length 4 + resources length 4 + resource header 8。
    resource_overrun[42..46].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_invalid(&resource_overrun, PsdErrorCode::InvalidInput);
}

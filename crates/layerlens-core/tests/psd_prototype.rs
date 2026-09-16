//! 依据人工定义的合成样本验证公开解析行为，不以候选解析器生成期望。

use std::{fs, io::Cursor, path::PathBuf};

use layerlens_core::psd::{LayerId, LayerKind, ParseLimits, PsdDocument, PsdErrorCode, Support};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd")
        .join(name)
}

fn fixture(name: &str) -> Vec<u8> {
    fs::read(fixture_path(name)).expect("读取已提交的合成样本")
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("fixtures/psd/manifest.json")).expect("合法样本清单")
}

fn expected_sample(manifest: &Value, name: &str) -> Value {
    manifest["samples"]
        .as_array()
        .expect("样本数组")
        .iter()
        .find(|sample| sample["file"] == name)
        .expect("清单包含样本")
        .clone()
}

fn parse(bytes: &[u8]) -> PsdDocument {
    PsdDocument::from_bytes(bytes, ParseLimits::default()).expect("支持的合成样本可以解析")
}

fn assert_error(bytes: &[u8], limits: ParseLimits, expected: PsdErrorCode) {
    let error = PsdDocument::from_bytes(bytes, limits)
        .err()
        .expect("应明确拒绝输入");
    assert_eq!(error.code, expected, "{error}");
    assert!(!error.to_string().is_empty());
}

fn hex_bytes(value: &Value) -> Vec<u8> {
    let text = value.as_str().expect("RGBA 十六进制字符串");
    assert!(text.len().is_multiple_of(2));
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).expect("ASCII 十六进制");
            u8::from_str_radix(digits, 16).expect("合法十六进制")
        })
        .collect()
}

fn assert_png(bytes: &[u8], expected: &Value) {
    let mut reader = png::Decoder::new(Cursor::new(bytes))
        .read_info()
        .expect("合法 PNG 头");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("PNG 可以独立解码");
    assert_eq!(
        u64::from(info.width),
        expected["width"].as_u64().expect("预期宽度")
    );
    assert_eq!(
        u64::from(info.height),
        expected["height"].as_u64().expect("预期高度")
    );
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    assert_eq!(
        &pixels[..info.buffer_size()],
        hex_bytes(&expected["rgbaHex"])
    );
}

fn assert_visible_layer_png(document: &PsdDocument, expected: &Value) {
    let layer = document
        .info()
        .layers
        .iter()
        .find(|layer| layer.name == "negative-origin")
        .expect("负坐标可见图层");
    let source = expected["layers"]
        .as_array()
        .expect("预期图层")
        .iter()
        .find(|source| source["name"] == layer.name)
        .expect("图层像素预期");
    let mut pixels = source["bounds"].clone();
    pixels["rgbaHex"] = source["rgbaHex"].clone();
    assert_png(
        &document.layer_png(layer.id).expect("简单可见图层可导出"),
        &pixels,
    );
}

#[test]
fn fixture_fingerprints_match_the_independent_manifest() {
    for sample in manifest()["samples"].as_array().expect("样本数组") {
        let bytes = fixture(sample["file"].as_str().expect("文件名"));
        assert_eq!(
            bytes.len() as u64,
            sample["sizeBytes"].as_u64().expect("样本字节数")
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            sample["sha256"].as_str().expect("指纹")
        );
    }
}

#[test]
fn raw_and_rle_metadata_preserve_bounds_visibility_order_and_missing_profile() {
    let manifest = manifest();
    assert_eq!(manifest["layerOrder"], "bottom-to-top");
    for name in ["bitmap-raw.psd", "bitmap-rle.psd"] {
        let expected = expected_sample(&manifest, name);
        let bytes = fixture(name);
        let document = parse(&bytes);
        let info = document.info();
        assert_eq!(
            u64::from(info.width),
            expected["document"]["width"].as_u64().unwrap()
        );
        assert_eq!(
            u64::from(info.height),
            expected["document"]["height"].as_u64().unwrap()
        );
        assert_eq!(
            u64::from(info.bit_depth),
            expected["document"]["depth"].as_u64().unwrap()
        );
        assert_eq!(
            info.color_mode,
            expected["document"]["colorMode"].as_str().unwrap()
        );
        let dpi = expected["document"]["resolutionDpi"].as_f64().unwrap();
        assert_eq!(info.resolution_dpi, Some([dpi, dpi]));
        assert!(info.color_profile.is_none(), "缺失 ICC 不能补成 sRGB");
        assert_eq!(info.text.status, Support::Unsupported);
        assert!(!info.text.reason.is_empty());
        assert_eq!(info.preview.status, Support::Supported);
        assert_eq!(document.source_size_bytes(), bytes.len());
        assert_eq!(
            document.source_sha256(),
            expected["sha256"].as_str().unwrap()
        );

        let source_layers = expected["layers"].as_array().unwrap();
        assert_eq!(info.layers.len(), source_layers.len());
        for (layer, source) in info.layers.iter().zip(source_layers.iter().rev()) {
            assert_eq!(layer.name, source["name"].as_str().unwrap());
            assert_eq!(layer.kind, LayerKind::Bitmap);
            assert_eq!(layer.parent_id, None);
            assert_eq!(
                serde_json::to_value(layer.bounds).unwrap(),
                source["bounds"]
            );
            assert_eq!(layer.visible, source["visible"].as_bool().unwrap());
            assert_eq!(layer.effective_visible, layer.visible);
            assert_eq!(layer.opacity, 255);
        }
        assert_ne!(info.layers[0].id, info.layers[1].id);
    }
}

#[test]
fn raw_and_rle_pngs_match_exact_pixels_and_keep_off_canvas_transparency() {
    let manifest = manifest();
    for name in ["bitmap-raw.psd", "bitmap-rle.psd"] {
        let expected = expected_sample(&manifest, name);
        let document = parse(&fixture(name));
        assert_png(
            &document.preview_png().expect("保存时合成预览"),
            &expected["composite"],
        );
        // 3×2 的完整矩形含画布外红色像素、零 alpha 蓝色与 alpha=128；不能裁切或预乘。
        assert_visible_layer_png(&document, &expected);
    }
}

#[test]
fn hidden_layers_and_unknown_layer_ids_have_distinct_errors() {
    let document = parse(&fixture("bitmap-raw.psd"));
    let hidden = document
        .info()
        .layers
        .iter()
        .find(|layer| !layer.visible)
        .unwrap();
    assert_eq!(hidden.export.status, Support::Unsupported);
    assert!(!hidden.export.reason.is_empty());
    assert_eq!(
        document.layer_png(hidden.id).unwrap_err().code,
        PsdErrorCode::ExportUnsupported
    );
    assert_eq!(
        document.layer_png(LayerId(u32::MAX)).unwrap_err().code,
        PsdErrorCode::LayerNotFound
    );
}

#[test]
fn missing_composite_keeps_metadata_and_independent_layer_pixels_available() {
    let expected = expected_sample(&manifest(), "bitmap-no-composite.psd");
    let document = parse(&fixture("bitmap-no-composite.psd"));
    assert_eq!(document.info().layers.len(), 2);
    assert_eq!(document.info().preview.status, Support::Unsupported);
    assert!(!document.info().preview.reason.is_empty());
    assert_eq!(
        document.preview_png().unwrap_err().code,
        PsdErrorCode::PreviewUnavailable
    );
    assert_visible_layer_png(&document, &expected);
}

#[test]
fn modifying_the_callers_buffer_does_not_change_later_decoding_or_fingerprint() {
    let expected = expected_sample(&manifest(), "bitmap-rle.psd");
    let mut bytes = fixture("bitmap-rle.psd");
    let document = parse(&bytes);
    bytes.fill(0);
    drop(bytes);
    assert_eq!(
        document.source_sha256(),
        expected["sha256"].as_str().unwrap()
    );
    assert_png(&document.preview_png().unwrap(), &expected["composite"]);
    assert_visible_layer_png(&document, &expected);
}

// 只读取 PSD 规范的顶层长度字段，用于构造损坏输入；不调用生产预检实现。
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn layer_section_offset(bytes: &[u8]) -> usize {
    let resources = 26 + 4 + u32_at(bytes, 26) as usize;
    resources + 4 + u32_at(bytes, resources) as usize
}

fn composite_offset(bytes: &[u8]) -> usize {
    let layers = layer_section_offset(bytes);
    layers + 4 + u32_at(bytes, layers) as usize
}

#[test]
fn every_truncated_prefix_is_rejected_except_the_explicit_missing_composite_boundary() {
    for name in ["bitmap-raw.psd", "bitmap-rle.psd"] {
        let bytes = fixture(name);
        let image_start = composite_offset(&bytes);
        for length in 0..bytes.len() {
            match PsdDocument::from_bytes(&bytes[..length], ParseLimits::default()) {
                Ok(document) => {
                    assert_eq!(
                        length, image_start,
                        "{name} 在 {length} 字节处静默接纳截断输入"
                    );
                    assert_eq!(
                        document.preview_png().unwrap_err().code,
                        PsdErrorCode::PreviewUnavailable
                    );
                }
                Err(error) => {
                    assert_ne!(
                        length, image_start,
                        "完整元数据后 EOF 应保留元数据：{error}"
                    );
                    assert_eq!(
                        error.code,
                        PsdErrorCode::InvalidInput,
                        "{name} 截断于 {length}: {error}"
                    );
                }
            }
        }
    }
}

#[test]
fn unsupported_depth_and_document_modes_are_explicit() {
    assert_error(
        &fixture("rgb16.psd"),
        ParseLimits::default(),
        PsdErrorCode::Unsupported,
    );
    for (offset, value) in [(4, 2_u16), (22, 32), (24, 4)] {
        let mut bytes = fixture("bitmap-raw.psd");
        bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
        assert_error(&bytes, ParseLimits::default(), PsdErrorCode::Unsupported);
    }
}

#[test]
fn malformed_headers_sections_and_rle_cannot_become_successful_pixels() {
    let valid = fixture("bitmap-raw.psd");
    let mut signature = valid.clone();
    signature[0] = b'?';
    assert_error(
        &signature,
        ParseLimits::default(),
        PsdErrorCode::InvalidInput,
    );
    let mut section = valid.clone();
    section[26..30].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_error(&section, ParseLimits::default(), PsdErrorCode::InvalidInput);
    for dimension in [0_u32, u32::MAX] {
        let mut bytes = valid.clone();
        bytes[18..22].copy_from_slice(&dimension.to_be_bytes());
        assert_error(&bytes, ParseLimits::default(), PsdErrorCode::InvalidInput);
    }
    let mut rle = fixture("bitmap-rle.psd");
    let image = composite_offset(&rle);
    // 图像高度为 3、RGB 三通道，共 9 个两字节行长；首行 literal 伪称需要 128 字节。
    rle[image + 2 + 9 * 2] = 127;
    assert_error(&rle, ParseLimits::default(), PsdErrorCode::InvalidInput);
}

#[test]
fn resource_limits_include_layer_pixels_and_accept_the_exact_boundary() {
    let bytes = fixture("bitmap-raw.psd");
    // 画布 12 + 可见图层 6 + 隐藏图层 1，隐藏不免除内存预算。
    let limits = ParseLimits {
        max_file_bytes: bytes.len() as u64,
        max_total_pixels: 19,
        max_layers: 2,
        ..ParseLimits::default()
    };
    assert!(PsdDocument::from_bytes(&bytes, limits).is_ok());
    for limits in [
        ParseLimits {
            max_file_bytes: bytes.len() as u64 - 1,
            ..limits
        },
        ParseLimits {
            max_total_pixels: 18,
            ..limits
        },
        ParseLimits {
            max_layers: 1,
            ..limits
        },
    ] {
        assert_error(&bytes, limits, PsdErrorCode::ResourceLimit);
    }
    let mut large = bytes;
    large[14..18].copy_from_slice(&30_000_u32.to_be_bytes());
    large[18..22].copy_from_slice(&30_000_u32.to_be_bytes());
    assert_error(&large, ParseLimits::default(), PsdErrorCode::ResourceLimit);
}

#[test]
fn merged_alpha_preserves_pixels_while_extra_channels_remain_unverified() {
    let mut bytes = fixture("bitmap-raw.psd");
    let count = layer_section_offset(&bytes) + 8;
    bytes[count..count + 2].copy_from_slice(&(-2_i16).to_be_bytes());
    bytes[12..14].copy_from_slice(&4_u16.to_be_bytes());
    bytes.extend_from_slice(&[255; 12]);
    let document = parse(&bytes);
    assert_eq!(document.info().layers.len(), 2);
    assert_eq!(document.info().preview.status, Support::Supported);
    assert_png(
        &document.preview_png().unwrap(),
        &expected_sample(&manifest(), "bitmap-raw.psd")["composite"],
    );
    assert_visible_layer_png(&document, &expected_sample(&manifest(), "bitmap-raw.psd"));

    // 正层数的第 4 个通道也可能是额外 alpha 选区，不能直接冒充合成透明度。
    bytes[count..count + 2].copy_from_slice(&2_i16.to_be_bytes());
    let extra_alpha = parse(&bytes);
    assert_eq!(extra_alpha.info().preview.status, Support::Unsupported);
    assert_visible_layer_png(
        &extra_alpha,
        &expected_sample(&manifest(), "bitmap-raw.psd"),
    );
}

#[test]
fn deferred_decode_budget_does_not_block_metadata_and_small_layers() {
    let document = PsdDocument::from_bytes(
        &fixture("bitmap-rle.psd"),
        ParseLimits {
            // 小图层需 24 字节 RGBA 及 RLE 临时表；画布 RGBA 自身即需 48 字节。
            max_decoded_bytes: 47,
            ..ParseLimits::default()
        },
    )
    .unwrap();
    assert_eq!(document.info().layers.len(), 2);
    assert_eq!(
        document.preview_png().unwrap_err().code,
        PsdErrorCode::ResourceLimit
    );
    assert_visible_layer_png(&document, &expected_sample(&manifest(), "bitmap-rle.psd"));
}

#[cfg(windows)]
mod file_source {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            for _ in 0..100 {
                let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir()
                    .join(format!("layerlens-psd-api-{}-{id}", std::process::id()));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("无法创建隔离测试目录：{error}"),
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
    fn opening_and_exporting_are_read_only_and_later_source_changes_do_not_change_results() {
        let directory = TestDirectory::new();
        let path = directory.0.join("source.psd");
        let original = fixture("bitmap-raw.psd");
        fs::write(&path, &original).unwrap();
        let document = PsdDocument::open(&path, ParseLimits::default()).unwrap();
        let expected = expected_sample(&manifest(), "bitmap-raw.psd");
        assert_png(&document.preview_png().unwrap(), &expected["composite"]);
        assert_visible_layer_png(&document, &expected);
        assert_eq!(fs::read(&path).unwrap(), original, "解析和导出不回写 PSD");

        fs::write(&path, b"changed in place").unwrap();
        assert_png(&document.preview_png().unwrap(), &expected["composite"]);
        assert_visible_layer_png(&document, &expected);
        fs::rename(&path, directory.0.join("old.psd")).unwrap();
        fs::write(&path, fixture("rgb16.psd")).unwrap();
        assert_png(&document.preview_png().unwrap(), &expected["composite"]);
        assert_visible_layer_png(&document, &expected);
        assert_eq!(
            document.source_sha256(),
            expected["sha256"].as_str().unwrap()
        );
    }

    #[test]
    fn file_boundary_errors_are_classified_and_keep_the_io_cause() {
        use std::error::Error;

        let directory = TestDirectory::new();
        let missing = PsdDocument::open(&directory.0.join("missing.psd"), ParseLimits::default())
            .err()
            .expect("缺失文件失败");
        assert_eq!(missing.code, PsdErrorCode::Io);
        assert!(missing.source().is_some());
        let not_file = PsdDocument::open(&directory.0, ParseLimits::default())
            .err()
            .unwrap();
        assert_eq!(not_file.code, PsdErrorCode::InvalidInput);
        let path = directory.0.join("source.psd");
        fs::write(&path, fixture("bitmap-raw.psd")).unwrap();
        let over_budget = PsdDocument::open(
            &path,
            ParseLimits {
                max_file_bytes: 1,
                ..ParseLimits::default()
            },
        )
        .err()
        .unwrap();
        assert_eq!(over_budget.code, PsdErrorCode::ResourceLimit);

        let writer = fs::OpenOptions::new().write(true).open(&path).unwrap();
        let changing = PsdDocument::open(&path, ParseLimits::default())
            .err()
            .expect("不能在已有 writer 时回退为无保护读取");
        assert_eq!(changing.code, PsdErrorCode::SourceChanged);
        assert!(changing.source().is_some());
        drop(writer);
        assert!(PsdDocument::open(&path, ParseLimits::default()).is_ok());
    }
}

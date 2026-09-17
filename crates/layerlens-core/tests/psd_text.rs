//! 使用独立字节生成器的公开 PSD 核对原文、属性来源、区间降级与 DPI 不变量。

use std::{fs, path::PathBuf};

use layerlens_core::psd::{ParseLimits, PsdDocument, PsdErrorCode, Support};
use serde_json::Value;

fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/psd")
            .join(name),
    )
    .unwrap()
}

fn expected(name: &str) -> Value {
    let manifest: Value = serde_json::from_str(include_str!("fixtures/psd/manifest.json")).unwrap();
    manifest["samples"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["file"] == name)
        .unwrap()
        .clone()
}

// JSON number 不区分 2 与 2.0；保持对象键、null、数组顺序和所有数值的精确核对。
fn assert_json(actual: &Value, expected: &Value, context: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => assert_eq!(a.as_f64(), b.as_f64(), "{context}"),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{context}");
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                assert_json(a, b, &format!("{context}[{index}]"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{context}");
            for (key, b) in b {
                assert_json(a.get(key).unwrap(), b, &format!("{context}.{key}"));
            }
        }
        _ => assert_eq!(actual, expected, "{context}"),
    }
}

#[test]
fn explicit_text_properties_and_failures_match_independent_psd_expectations() {
    let name = "text-engine-72.psd";
    let document = PsdDocument::from_bytes(&fixture(name), ParseLimits::default()).unwrap();
    let sample = expected(name);
    for case in sample["textCases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let layer = document
            .info()
            .layers
            .iter()
            .find(|v| v.name == name)
            .unwrap();
        let expected = &case["expected"];
        if expected.get("text").is_some() {
            assert!(layer.text.is_none(), "{name}: 缺失 Txt 不能伪装为空字符串");
            continue;
        }
        let text = layer.text.as_ref().unwrap();
        let actual = serde_json::to_value(text).unwrap();
        assert_json(&actual["transform"], &sample["textTransform"], name);
        for key in [
            "rawText",
            "utf16Length",
            "indexMapping",
            "styleRuns",
            "paragraphRuns",
        ] {
            if let Some(value) = expected.get(key) {
                assert_json(&actual[key], value, &format!("{name}.{key}"));
            }
        }
        for (key, pointer) in [
            ("firstFont", "/styleRuns/0/style/font"),
            ("firstFontSize", "/styleRuns/0/style/fontSize"),
            ("firstColor", "/styleRuns/0/style/fillColor"),
            ("firstAlignment", "/paragraphRuns/0/style/alignment"),
        ] {
            if let Some(value) = expected.get(key) {
                assert_eq!(actual.pointer(pointer), Some(value), "{name}.{key}");
            }
        }
        for (flag, key) in [
            ("paragraphAvailable", "paragraphRuns"),
            ("characterAvailable", "styleRuns"),
        ] {
            if expected[flag] == true {
                assert!(!actual[key].as_array().unwrap().is_empty(), "{name}.{key}");
            }
        }
        let diagnostics = actual["diagnostics"].as_array().unwrap();
        if let Some(code) = expected.get("diagnostic") {
            assert!(
                diagnostics.iter().any(|v| &v["code"] == code),
                "{name}: {diagnostics:?}"
            );
        } else {
            assert_eq!(
                diagnostics.len(),
                1,
                "{name}: 有效样本只应报告待独立参考验证"
            );
            assert_eq!(diagnostics[0]["code"], "unverified");
        }
        assert_eq!(
            text.styles.status,
            if [
                "text-mismatch",
                "missing-engine-text",
                "invalid-engine-text"
            ]
            .contains(&name)
            {
                Support::Unsupported
            } else {
                Support::Partial
            }
        );
        if name == "invalid-local" {
            assert!(actual["styleRuns"][0]["style"]["fauxBold"].is_null());
            assert!(actual["styleRuns"][0]["style"]["horizontalScale"].is_null());
            assert!(actual["paragraphRuns"][0]["style"]["autoLeading"].is_null());
            assert_eq!(actual["styleRuns"][1]["style"]["font"]["value"]["index"], 2);
        }
        if name == "style-reference" {
            assert!(
                actual["styleRuns"][0]["style"]["font"].is_null(),
                "未展开的引用不能按默认字体伪装成功"
            );
        }
        if name == "unsupported-color" {
            assert!(diagnostics.iter().any(|v| v["code"] == "unsupported"
                && v["path"].as_str().unwrap().ends_with("FillColor")));
        }
    }
}

#[test]
fn dpi_never_rescales_unverified_engine_metrics_or_original_matrix() {
    let mut baseline = None;
    for name in [
        "text-engine-72.psd",
        "text-engine-300.psd",
        "text-engine-no-dpi.psd",
    ] {
        let document = PsdDocument::from_bytes(&fixture(name), ParseLimits::default()).unwrap();
        let sample = expected(name);
        let dpi = sample["document"]["resolutionDpi"].as_f64();
        assert_eq!(document.info().resolution_dpi, dpi.map(|v| [v, v]));
        let text = document
            .info()
            .layers
            .iter()
            .find(|v| v.name == "mixed")
            .unwrap()
            .text
            .as_ref()
            .unwrap();
        let actual = serde_json::to_value(text).unwrap();
        if let Some(previous) = &baseline {
            assert_eq!(&actual, previous, "{name}");
        } else {
            baseline = Some(actual);
        }
    }
}

#[test]
fn malformed_engine_strings_and_numbers_fail_without_losing_layer_boundaries() {
    let original = fixture("text-engine-300.psd");
    let marker = b"/Editor << /Text (";
    let start = original
        .windows(marker.len())
        .position(|v| v == marker)
        .unwrap()
        + marker.len();
    let mut invalid_utf16 = original.clone();
    // 用孤立高代理项替换第一个空格，保留全部 PSD/descriptor 长度。
    invalid_utf16[start + 2..start + 4].copy_from_slice(&[0xd8, 0]);
    let mut invalid_number = original;
    let marker = b"/FontSize 18";
    let end = invalid_number
        .windows(marker.len())
        .position(|v| v == marker)
        .unwrap()
        + marker.len();
    invalid_number[end - 1] = b'-';
    let mut invalid_tysh = fixture("metadata-text.psd");
    let marker = b"Txt TEXT";
    let start = invalid_tysh
        .windows(marker.len())
        .position(|v| v == marker)
        .unwrap()
        + marker.len()
        + 4;
    invalid_tysh[start..start + 2].copy_from_slice(&[0xd8, 0]);
    for bytes in [invalid_utf16, invalid_number, invalid_tysh] {
        let error = PsdDocument::from_bytes(&bytes, ParseLimits::default())
            .err()
            .unwrap();
        assert_eq!(error.code, PsdErrorCode::ParseFailed);
    }
}

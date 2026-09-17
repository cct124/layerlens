//! 样式归一化的资源上限和候选边界，不依赖文件 I/O 或第三方写入器。

use super::*;

fn dict(entries: Vec<(&str, EngineValue)>) -> EngineValue {
    EngineValue::Dict(
        entries
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect(),
    )
}

#[test]
fn run_limit_accepts_boundary_and_rejects_excess_without_partial_output() {
    for count in [MAX_STYLE_RUNS, MAX_STYLE_RUNS + 1] {
        let value = dict(vec![
            ("RunArray", EngineValue::Array(vec![dict(vec![]); count])),
            (
                "RunLengthArray",
                EngineValue::Array(vec![EngineValue::Number(1.0); count]),
            ),
        ]);
        let mut diagnostics = Vec::new();
        let result = runs(
            Some(&value),
            "StyleRun",
            &vec![65; count],
            count as u32,
            &mut diagnostics,
        );
        if count == MAX_STYLE_RUNS {
            assert_eq!(result.unwrap().len(), count);
            assert!(diagnostics.is_empty());
        } else {
            assert!(result.is_none());
            assert_eq!(diagnostics[0].code, TextDiagnosticCode::InvalidRange);
        }
    }
}

#[test]
fn utf16_boundaries_allow_combining_marks_but_never_split_surrogates() {
    let value = dict(vec![
        ("RunArray", EngineValue::Array(vec![dict(vec![]); 2])),
        (
            "RunLengthArray",
            EngineValue::Array(vec![EngineValue::Number(1.0); 2]),
        ),
    ]);
    assert!(runs(Some(&value), "StyleRun", &[101, 0x301], 2, &mut Vec::new()).is_some());
    assert!(
        runs(
            Some(&value),
            "StyleRun",
            &[0xd83d, 0xde00],
            2,
            &mut Vec::new()
        )
        .is_none()
    );
}

#[test]
fn nonfinite_candidate_metrics_are_null_without_hiding_valid_neighbors() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let style = dict(vec![(
            "StyleSheet",
            dict(vec![(
                "StyleSheetData",
                dict(vec![
                    ("FontSize", EngineValue::Number(value)),
                    ("Leading", EngineValue::Number(24.0)),
                ]),
            )]),
        )]);
        let mut diagnostics = Vec::new();
        let result = properties::character(&style, None, None, 0, &mut diagnostics);
        assert!(result.font_size.is_none());
        assert_eq!(result.leading.unwrap().value.value, 24.0);
        assert!(
            diagnostics
                .iter()
                .any(|v| v.code == TextDiagnosticCode::Invalid && v.path.ends_with("FontSize"))
        );
    }
}

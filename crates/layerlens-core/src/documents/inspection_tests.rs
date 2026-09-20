use super::*;
use crate::psd::{PsdDocument, TextData, TextDiagnosticCode};

fn sample_layer() -> LayerInfo {
    let doc = PsdDocument::from_bytes(
        include_bytes!("../../tests/fixtures/psd/text-engine-72.psd"),
        Default::default(),
    )
    .unwrap();
    doc.info()
        .layers
        .iter()
        .find(|layer| {
            layer.text.as_ref().is_some_and(|text| {
                text.style_runs
                    .as_ref()
                    .is_some_and(|runs| !runs.is_empty())
            })
        })
        .unwrap()
        .clone()
}

fn sample() -> TextData {
    sample_layer().text.unwrap()
}

#[test]
fn long_text_pages_reconstruct_unicode_without_splitting_surrogate_pairs() {
    let mut text = sample();
    text.raw_text = format!("{}😀e\u{301}\r\n尾  ", "a".repeat(2047));
    text.utf16_length = text.raw_text.encode_utf16().count() as u32;
    text.style_runs = None;
    text.paragraph_runs = None;
    let first = text_slice(&text, 0).unwrap();
    assert_eq!(first.end, 2047);
    let next = text_slice(&text, first.next_start.unwrap()).unwrap();
    assert_eq!(format!("{}{}", first.text, next.text), text.raw_text);
    assert_eq!(next.next_start, None);
    assert!(text_slice(&text, 2048).is_err());
    assert!(text_slice(&text, text.utf16_length + 1).is_err());
}

#[test]
fn run_budget_shortens_text_page_without_losing_style_ranges() {
    let mut text = sample();
    let run = text.style_runs.as_ref().unwrap()[0].clone();
    text.raw_text = "a".repeat(260);
    text.utf16_length = 260;
    text.paragraph_runs = None;
    text.style_runs = Some(
        (0..260)
            .map(|start| TextStyleRun {
                start,
                end: start + 1,
                style: run.style.clone(),
            })
            .collect(),
    );
    let first = text_slice(&text, 0).unwrap();
    assert_eq!(first.end, 128);
    assert_eq!(first.style_runs.unwrap().len(), 128);
    let second = text_slice(&text, 128).unwrap();
    assert_eq!(second.end, 256);
    assert_eq!(second.style_runs.unwrap()[0].start, 128);
    assert_eq!(text_slice(&text, 256).unwrap().end, 260);
}

#[test]
fn response_byte_budget_pages_large_styles_without_losing_text_or_absolute_ranges() {
    let mut layer = sample_layer();
    let text = layer.text.as_mut().unwrap();
    let mut style = text.style_runs.as_ref().unwrap()[0].style.clone();
    style.font.as_mut().unwrap().value.name = "字\"".repeat(512);
    text.raw_text = "😀ab".repeat(100);
    text.utf16_length = text.raw_text.encode_utf16().count() as u32;
    text.style_runs = Some(
        text.raw_text
            .chars()
            .scan(0, |start, character| {
                let end = *start + character.len_utf16() as u32;
                let run = TextStyleRun {
                    start: *start,
                    end,
                    style: style.clone(),
                };
                *start = end;
                Some(run)
            })
            .collect(),
    );
    let paragraph = text.paragraph_runs.as_ref().unwrap()[0].style.clone();
    text.paragraph_runs = Some(vec![ParagraphStyleRun {
        start: 0,
        end: text.utf16_length,
        style: paragraph,
    }]);
    let original = layer.text.as_ref().unwrap();
    let mut start = 0;
    let mut restored = String::new();
    loop {
        let details = inspect_layer(&layer, start).unwrap();
        assert!(serde_json::to_vec(&details).unwrap().len() <= RESPONSE_BYTES as usize);
        let page = details.text.unwrap();
        assert_eq!(page.start, start);
        assert!(page.end > start);
        assert_eq!(page.total_length, original.utf16_length);
        assert_eq!(page.transform, original.transform);
        assert_eq!(page.text.encode_utf16().count() as u32, page.end - start);
        let expected: Vec<_> = original
            .style_runs
            .as_ref()
            .unwrap()
            .iter()
            .filter(|run| run.end > start && run.start < page.end)
            .collect();
        assert_eq!(
            serde_json::to_value(page.style_runs).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(
            serde_json::to_value(page.paragraph_runs).unwrap(),
            serde_json::to_value(&original.paragraph_runs).unwrap()
        );
        restored.push_str(&page.text);
        match page.next_start {
            Some(next) => {
                assert_eq!(next, page.end);
                start = next;
            }
            None => {
                assert_eq!(page.end, original.utf16_length);
                break;
            }
        }
    }
    assert_eq!(restored, original.raw_text);
}

#[test]
fn oversized_properties_fail_before_clone_and_empty_text_is_available() {
    assert!(matches!(
        bounded_clone(&"a".repeat(32768)),
        Err(DocumentError::ResourceLimit { .. })
    ));
    let mut text = sample();
    text.raw_text.clear();
    text.utf16_length = 0;
    text.style_runs = Some(vec![]);
    text.paragraph_runs = Some(vec![]);
    let result = text_slice(&text, 0).unwrap();
    assert_eq!(result.text, "");
    assert!(result.style_runs.unwrap().is_empty());
    assert_eq!(result.next_start, None);
}

#[test]
fn indivisible_metadata_over_budget_fails_for_empty_text_and_one_surrogate_pair() {
    let mut layer = sample_layer();
    for raw in ["", "😀"] {
        let text = layer.text.as_mut().unwrap();
        text.raw_text = raw.into();
        text.utf16_length = raw.encode_utf16().count() as u32;
        text.style_runs = None;
        text.paragraph_runs = None;
        text.diagnostics = vec![
            TextDiagnostic {
                code: TextDiagnosticCode::Unverified,
                path: "EngineData".into(),
                message: "x".repeat(16 * 1024),
            };
            32
        ];
        assert!(matches!(
            inspect_layer(&layer, 0),
            Err(DocumentError::ResourceLimit {
                limit: RESPONSE_BYTES,
                ..
            })
        ));
    }
}

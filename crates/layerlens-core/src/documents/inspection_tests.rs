use super::*;
use crate::psd::{PsdDocument, TextData};

fn sample() -> TextData {
    let doc = PsdDocument::from_bytes(
        include_bytes!("../../tests/fixtures/psd/text-engine-72.psd"),
        Default::default(),
    )
    .unwrap();
    doc.info()
        .layers
        .iter()
        .filter_map(|layer| layer.text.as_ref())
        .find(|text| {
            text.style_runs
                .as_ref()
                .is_some_and(|runs| !runs.is_empty())
        })
        .unwrap()
        .clone()
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

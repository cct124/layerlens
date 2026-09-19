//! 查看器通过固定修订读取和选择，不依赖合成图可用性或前端状态。
#![cfg(windows)]

use layerlens_core::{
    documents::{DocumentError, DocumentService, OpenOutcome},
    psd::{LayerId, LayerKind},
};
use std::{path::Path, time::Duration};

fn open(service: &DocumentService, name: &str) -> layerlens_core::documents::DocumentLease {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd")
        .join(name);
    match service
        .open(path)
        .unwrap()
        .wait_timeout(Duration::from_secs(10))
        .unwrap()
        .outcome
    {
        OpenOutcome::Opened { document_id, .. } => service.lease(document_id).unwrap(),
        ref value => panic!("打开失败：{value:?}"),
    }
}

#[test]
fn pages_preserve_hierarchy_hidden_layers_and_original_geometry() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service, "resources-groups-alpha-raw.psd");
    let mut found = vec![];
    let mut offset = 0;
    loop {
        let page = lease.layers(offset, 2).unwrap();
        found.extend(page.layers);
        match page.next_offset {
            Some(next) => offset = next,
            None => break,
        }
    }
    assert_eq!(found.len(), lease.info().layers.len());
    assert!(found.iter().any(|layer| layer.kind == LayerKind::Group));
    assert!(
        found
            .iter()
            .any(|layer| layer.visible && !layer.effective_visible)
    );
    for (actual, original) in found.iter().zip(&lease.info().layers) {
        assert_eq!(actual.id, original.id);
        assert_eq!(actual.parent_id, original.parent_id);
        assert_eq!(actual.bounds, original.bounds);
    }
    assert!(matches!(
        lease.layers(0, 129),
        Err(DocumentError::InvalidQuery(_))
    ));
    assert!(matches!(
        lease.layers(9999, 1),
        Err(DocumentError::InvalidQuery(_))
    ));
    assert!(matches!(
        lease.layer_details(LayerId(9999), 0),
        Err(DocumentError::LayerNotFound(_))
    ));
}

#[test]
fn selection_is_per_document_and_cannot_cross_revisions_or_closed_entries() {
    let service = DocumentService::new(Default::default()).unwrap();
    let a = open(&service, "bitmap-raw.psd");
    let b = open(&service, "bitmap-no-composite.psd");
    service.select_layer(&a, Some(LayerId(0))).unwrap();
    service.select_layer(&b, Some(LayerId(1))).unwrap();
    service.activate(a.document_id()).unwrap();
    let state = service.snapshot();
    assert_eq!(state.documents[0].selected_layer, Some(LayerId(0)));
    assert_eq!(state.documents[1].selected_layer, Some(LayerId(1)));
    assert!(b.layer_details(LayerId(0), 0).is_ok());
    assert!(service.select_layer(&a, Some(LayerId(9999))).is_err());
    assert_eq!(
        service.snapshot().documents[0].selected_layer,
        Some(LayerId(0))
    );
    service.reload(a.document_id()).unwrap().wait();
    assert_eq!(service.snapshot().documents[0].selected_layer, None);
    assert!(matches!(
        service.select_layer(&a, Some(LayerId(0))),
        Err(DocumentError::StaleRevision)
    ));
    service.close(b.document_id()).unwrap();
    assert!(matches!(
        service.select_layer(&b, None),
        Err(DocumentError::NotFound(_))
    ));
    assert!(b.layer_details(LayerId(0), 0).is_ok());
}

#[test]
fn original_text_styles_and_utf16_boundaries_survive_inspection() {
    let service = DocumentService::new(Default::default()).unwrap();
    let lease = open(&service, "text-engine-72.psd");
    let layer = lease
        .info()
        .layers
        .iter()
        .find(|layer| {
            layer
                .text
                .as_ref()
                .is_some_and(|text| text.raw_text.contains('😀') && text.style_runs.is_some())
        })
        .unwrap();
    let original = layer.text.as_ref().unwrap();
    let details = lease.layer_details(layer.id, 0).unwrap();
    let text = details.text.unwrap();
    assert_eq!(text.text, original.raw_text);
    assert_eq!(text.total_length, original.utf16_length);
    assert_eq!(text.transform, original.transform);
    assert_eq!(
        serde_json::to_value(text.style_runs).unwrap(),
        serde_json::to_value(&original.style_runs).unwrap()
    );
    let emoji = original
        .raw_text
        .split('😀')
        .next()
        .unwrap()
        .encode_utf16()
        .count() as u32;
    assert!(matches!(
        lease.layer_details(layer.id, emoji + 1),
        Err(DocumentError::InvalidQuery(_))
    ));
    assert_eq!(
        lease
            .layer_details(layer.id, emoji + 2)
            .unwrap()
            .text
            .unwrap()
            .start,
        emoji + 2
    );
}

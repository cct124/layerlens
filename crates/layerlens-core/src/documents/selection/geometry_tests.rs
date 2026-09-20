//! 合成规范化层树验证授权与几何规则，不需要改写 PSD 或不可变修订。

use super::*;
use crate::{
    documents::RevisionId,
    psd::{Bounds, DocumentInfo, ExportBlocker, LayerKind},
};

fn tree() -> DocumentInfo {
    let document = PsdDocument::from_bytes(
        include_bytes!("../../../tests/fixtures/psd/resources-groups-alpha-raw.psd"),
        Default::default(),
    )
    .unwrap();
    let mut info = document.info().clone();
    info.width = 100;
    info.height = 100;
    let template = info.layers[0].clone();
    // 两层可见组、两个分离目标，以及隐藏组中自身可见但最终不可见的后代。
    info.layers = (0..6)
        .map(|id| {
            let mut layer = template.clone();
            layer.id = LayerId(id);
            layer.name = format!("node-{id}");
            layer.parent_id = match id {
                0 => None,
                1 | 4 => Some(LayerId(0)),
                2 | 3 => Some(LayerId(1)),
                _ => Some(LayerId(4)),
            };
            layer.kind = if [0, 1, 4].contains(&id) {
                LayerKind::Group
            } else {
                LayerKind::Bitmap
            };
            layer.bounds = match id {
                2 => Bounds {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 10,
                },
                3 => Bounds {
                    x: 80,
                    y: 80,
                    width: 10,
                    height: 10,
                },
                _ => Bounds {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 100,
                },
            };
            layer.visible = id != 4;
            layer.effective_visible = id < 4;
            layer.export_blockers.clear();
            layer.text = None;
            layer
        })
        .collect();
    info
}

fn refs(ids: &[u32]) -> SelectionInput {
    SelectionInput::Layers(
        ids.iter()
            .map(|id| LayerReference {
                document_id: DocumentId(1),
                revision_id: RevisionId(1),
                layer_id: LayerId(*id),
            })
            .collect(),
    )
}

fn normalize(info: &DocumentInfo, input: SelectionInput) -> SelectionScope {
    super::super::normalize::normalize(
        info,
        DocumentId(1),
        RevisionId(1),
        input,
        Default::default(),
    )
    .unwrap()
}

#[test]
fn explicit_groups_expand_visible_descendants_once_and_retain_authorization() {
    let info = tree();
    let scope = normalize(&info, refs(&[2, 0, 2, 1]));
    assert_eq!(
        scope.items,
        vec![
            SelectionItem::Layer {
                layer_id: LayerId(0)
            },
            SelectionItem::Layer {
                layer_id: LayerId(1)
            },
            SelectionItem::Layer {
                layer_id: LayerId(2)
            },
        ]
    );
    assert_eq!(scope.selected_group_ids, vec![LayerId(0), LayerId(1)]);
    assert_eq!(scope.target_layer_ids, vec![LayerId(2), LayerId(3)]);
    assert_eq!(scope.group_refs, vec![LayerId(0), LayerId(1)]);
    assert_eq!(scope.records.len(), 4);
    assert_eq!(
        scope.reference_bounds,
        Some(SelectionBounds {
            x: 0.0,
            y: 0.0,
            width: 90.0,
            height: 90.0
        })
    );
    assert!(scope.dependency_layer_ids.is_empty());
}

#[test]
fn structural_groups_never_expand_a_region_to_unselected_siblings() {
    let info = tree();
    let scope = normalize(&info, region(0.5, 0.5, 2.0, 2.0));
    assert_eq!(scope.target_layer_ids, vec![LayerId(2)]);
    assert_eq!(scope.group_refs, vec![LayerId(0), LayerId(1)]);
    assert!(scope.selected_group_ids.is_empty());
    assert_eq!(
        scope
            .records
            .iter()
            .map(|record| (record.layer_id, record.role))
            .collect::<Vec<_>>(),
        vec![
            (LayerId(0), LayerRole::Structure),
            (LayerId(1), LayerRole::Structure),
            (LayerId(2), LayerRole::Target)
        ]
    );
    assert!(
        scope
            .records
            .iter()
            .all(|record| record.intersection.unwrap().kind == IntersectionKind::Partial)
    );
    let child = normalize(&info, refs(&[2]));
    assert_eq!(child.target_layer_ids, vec![LayerId(2)]);
    assert_eq!(child.group_refs, scope.group_refs);
    assert!(child.selected_group_ids.is_empty());
}

#[test]
fn empty_geometry_can_be_explicit_but_has_no_reference_or_region_hit() {
    let mut info = tree();
    info.layers[2].bounds.width = 0;
    let scope = normalize(&info, refs(&[2]));
    assert_eq!(scope.target_layer_ids, vec![LayerId(2)]);
    assert!(scope.reference_bounds.is_none());
    assert!(
        scope
            .warnings
            .contains(&SelectionWarning::NoReferenceBounds)
    );
    assert!(
        normalize(&info, region(0.0, 0.0, 10.0, 10.0))
            .target_layer_ids
            .is_empty()
    );
}

#[test]
fn negative_bounds_keep_original_extent_and_only_positive_intersections_count() {
    let mut info = tree();
    let bounds = Bounds {
        x: -4,
        y: -2,
        width: 10,
        height: 8,
    };
    info.layers[2].bounds = bounds;
    info.layers[2]
        .export_blockers
        .push(ExportBlocker::LayerVisualDependency);
    let scope = normalize(&info, refs(&[2]));
    assert_eq!(
        scope.reference_bounds,
        Some(SelectionBounds {
            x: -4.0,
            y: -2.0,
            width: 10.0,
            height: 8.0
        })
    );
    assert!(
        scope
            .warnings
            .contains(&SelectionWarning::PixelDependenciesUnresolved)
    );
    assert!(scope.dependency_layer_ids.is_empty());
    let scope = normalize(&info, region(0.25, 0.5, 6.0, 7.0));
    let hit = scope
        .records
        .iter()
        .find(|record| record.layer_id == LayerId(2))
        .unwrap()
        .intersection
        .unwrap();
    assert_eq!(
        hit.bounds,
        SelectionBounds {
            x: 0.25,
            y: 0.5,
            width: 5.75,
            height: 5.5
        }
    );
    assert_eq!(hit.kind, IntersectionKind::Partial);
    assert_eq!(info.layers[2].bounds, bounds);
    assert!(
        normalize(&info, region(6.0, 0.0, 1.0, 1.0))
            .target_layer_ids
            .is_empty()
    );
}

#[test]
fn structural_records_share_the_scope_limit_and_invalid_parents_are_rejected() {
    let mut info = tree();
    assert!(matches!(
        super::super::normalize::normalize(
            &info,
            DocumentId(1),
            RevisionId(1),
            refs(&[2]),
            SelectionConfig {
                max_scope_layers: 2,
                ..Default::default()
            }
        ),
        Err(SelectionError::Document(
            DocumentError::ResourceLimit { .. }
        ))
    ));
    for parent in [LayerId(99), LayerId(3), LayerId(2)] {
        info.layers[2].parent_id = Some(parent);
        assert!(matches!(
            super::super::normalize::normalize(
                &info,
                DocumentId(1),
                RevisionId(1),
                refs(&[2]),
                Default::default()
            ),
            Err(SelectionError::InvalidInput(_))
        ));
    }
    info.layers[2].parent_id = Some(LayerId(1));
    info.layers[0].parent_id = Some(LayerId(1));
    assert!(matches!(
        super::super::normalize::normalize(
            &info,
            DocumentId(1),
            RevisionId(1),
            refs(&[2]),
            Default::default()
        ),
        Err(SelectionError::InvalidInput(_))
    ));
}

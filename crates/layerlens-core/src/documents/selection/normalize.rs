//! 无副作用的范围规范化。只使用固定修订事实，不推测视觉边界或区域文字片段。

use std::collections::{BTreeMap, BTreeSet};

use crate::documents::{DocumentError, DocumentId, RevisionId};
use crate::psd::{Bounds, DocumentInfo, ExportBlocker, LayerInfo, LayerKind};

use super::{
    IntersectionKind, LayerIntersection, LayerRole, SelectionBounds, SelectionConfig,
    SelectionError, SelectionInput, SelectionItem, SelectionScope, SelectionWarning,
    model::{ContentRecord, limit},
};

pub(super) fn normalize(
    info: &DocumentInfo,
    document_id: DocumentId,
    revision_id: RevisionId,
    input: SelectionInput,
    config: SelectionConfig,
) -> Result<SelectionScope, SelectionError> {
    let positions: BTreeMap<_, _> = info
        .layers
        .iter()
        .enumerate()
        .map(|(index, layer)| (layer.id.0, index))
        .collect();
    let mut explicit = BTreeSet::new();
    let mut targets = BTreeSet::new();
    let mut groups = BTreeSet::new();
    let mut selected_groups = BTreeSet::new();
    let region = match input {
        SelectionInput::Layers(refs) => {
            if refs.is_empty() {
                return Err(SelectionError::InvalidInput("空图层列表不能代替明确清空"));
            }
            if refs.len() > config.max_items {
                return Err(limit("原始选择项", config.max_items as u64));
            }
            for reference in refs {
                if reference.document_id != document_id || reference.revision_id != revision_id {
                    return Err(SelectionError::InvalidInput("图层引用必须属于同一文档修订"));
                }
                let index = positions
                    .get(&reference.layer_id.0)
                    .ok_or(DocumentError::LayerNotFound(reference.layer_id))?;
                explicit.insert(*index);
            }
            // 从显式选择根向下遍历；visited 防止组与后代重复选择造成重复目标或平方展开。
            let mut children: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
            for (index, layer) in info.layers.iter().enumerate() {
                if let Some(parent) = layer.parent_id {
                    children.entry(parent.0).or_default().push(index);
                }
            }
            let mut pending: Vec<_> = explicit.iter().copied().collect();
            let mut visited = BTreeSet::new();
            while let Some(index) = pending.pop() {
                if !visited.insert(index) {
                    continue;
                }
                let layer = &info.layers[index];
                if layer.kind == LayerKind::Group && explicit.contains(&index) {
                    selected_groups.insert(index);
                }
                if !layer.effective_visible {
                    continue;
                }
                if layer.kind == LayerKind::Group {
                    groups.insert(index);
                    if let Some(descendants) = children.get(&layer.id.0) {
                        pending.extend(descendants);
                    }
                } else {
                    targets.insert(index);
                }
            }
            None
        }
        SelectionInput::Region(bounds) => {
            validate_region(bounds, info)?;
            for (index, layer) in info.layers.iter().enumerate() {
                if layer.effective_visible && intersection(bounds, layer.bounds).is_some() {
                    if layer.kind == LayerKind::Group {
                        groups.insert(index);
                    } else {
                        targets.insert(index);
                    }
                }
            }
            Some(bounds)
        }
    };
    // 父组仅提供结构说明；不由结构引用向下加入未选中的兄弟节点。
    let roots: Vec<_> = targets.iter().chain(groups.iter()).copied().collect();
    for index in roots {
        add_ancestors(&info.layers[index], info, &positions, &mut groups)?;
    }
    if targets.len() + groups.len() > config.max_scope_layers {
        return Err(limit("选区投影图层", config.max_scope_layers as u64));
    }
    let items = match region {
        Some(bounds) => vec![SelectionItem::Region { bounds }],
        None => explicit
            .iter()
            .map(|index| SelectionItem::Layer {
                layer_id: info.layers[*index].id,
            })
            .collect(),
    };
    let reference_bounds =
        region.or_else(|| union(targets.iter().map(|index| info.layers[*index].bounds)));
    let mut warnings = vec![SelectionWarning::GeometricBoundsOnly];
    if reference_bounds.is_none() {
        warnings.push(SelectionWarning::NoReferenceBounds);
    }
    if targets.iter().any(|index| {
        info.layers[*index].export_blockers.iter().any(|blocker| {
            matches!(
                blocker,
                ExportBlocker::GlobalMask
                    | ExportBlocker::AncestorVisualDependency
                    | ExportBlocker::LayerVisualDependency
            )
        })
    }) {
        warnings.push(SelectionWarning::PixelDependenciesUnresolved);
    }
    let records: Vec<_> = info
        .layers
        .iter()
        .enumerate()
        .filter_map(|(index, layer)| {
            let role = if targets.contains(&index) {
                LayerRole::Target
            } else if groups.contains(&index) {
                LayerRole::Structure
            } else {
                return None;
            };
            Some(ContentRecord {
                layer_id: layer.id,
                stack_index: index as u32,
                role,
                intersection: region.and_then(|bounds| intersection(bounds, layer.bounds)),
            })
        })
        .collect();
    let text_layer_count = targets
        .iter()
        .filter(|index| info.layers[**index].kind == LayerKind::Text)
        .count() as u32;
    let ids = |indices: BTreeSet<usize>| {
        indices
            .into_iter()
            .map(|index| info.layers[index].id)
            .collect()
    };
    Ok(SelectionScope {
        items,
        target_layer_ids: ids(targets),
        selected_group_ids: ids(selected_groups),
        group_refs: ids(groups),
        dependency_layer_ids: Vec::new(),
        reference_bounds,
        warnings,
        records,
        text_layer_count,
    })
}

fn validate_region(bounds: SelectionBounds, info: &DocumentInfo) -> Result<(), SelectionError> {
    let right = bounds.x + bounds.width;
    let bottom = bounds.y + bounds.height;
    if ![
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        right,
        bottom,
    ]
    .iter()
    .all(|v| v.is_finite())
        || bounds.x < 0.0
        || bounds.y < 0.0
        || bounds.width <= 0.0
        || bounds.height <= 0.0
        || right <= bounds.x
        || bottom <= bounds.y
        || right > f64::from(info.width)
        || bottom > f64::from(info.height)
    {
        return Err(SelectionError::InvalidInput(
            "区域必须有限、具有正面积且完整位于画布内",
        ));
    }
    Ok(())
}

fn add_ancestors(
    layer: &LayerInfo,
    info: &DocumentInfo,
    positions: &BTreeMap<u32, usize>,
    groups: &mut BTreeSet<usize>,
) -> Result<(), SelectionError> {
    let mut parent = layer.parent_id;
    let mut visited = BTreeSet::new();
    while let Some(id) = parent {
        let index = *positions
            .get(&id.0)
            .ok_or(SelectionError::InvalidInput("缺失父组结构"))?;
        if !visited.insert(index) || info.layers[index].kind != LayerKind::Group {
            return Err(SelectionError::InvalidInput("父组结构无效"));
        }
        groups.insert(index);
        parent = info.layers[index].parent_id;
    }
    Ok(())
}

fn geometry(bounds: Bounds) -> SelectionBounds {
    SelectionBounds {
        x: f64::from(bounds.x),
        y: f64::from(bounds.y),
        width: f64::from(bounds.width),
        height: f64::from(bounds.height),
    }
}

pub(super) fn intersection(region: SelectionBounds, bounds: Bounds) -> Option<LayerIntersection> {
    let layer = geometry(bounds);
    let x = region.x.max(layer.x);
    let y = region.y.max(layer.y);
    let right = (region.x + region.width).min(layer.x + layer.width);
    let bottom = (region.y + region.height).min(layer.y + layer.height);
    if right <= x || bottom <= y {
        return None;
    }
    Some(LayerIntersection {
        kind: if layer.x >= region.x
            && layer.y >= region.y
            && layer.x + layer.width <= region.x + region.width
            && layer.y + layer.height <= region.y + region.height
        {
            IntersectionKind::Contained
        } else {
            IntersectionKind::Partial
        },
        bounds: SelectionBounds {
            x,
            y,
            width: right - x,
            height: bottom - y,
        },
    })
}

fn union(bounds: impl Iterator<Item = Bounds>) -> Option<SelectionBounds> {
    bounds
        .filter(|bounds| bounds.width > 0 && bounds.height > 0)
        .map(geometry)
        .reduce(|a, b| {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            SelectionBounds {
                x,
                y,
                width: (a.x + a.width).max(b.x + b.width) - x,
                height: (a.y + a.height).max(b.y + b.height) - y,
            }
        })
}

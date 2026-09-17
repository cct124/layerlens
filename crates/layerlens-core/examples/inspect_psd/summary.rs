//! 实验报告的汇总，不包含图层名称、文字原文或属性载荷。

use std::collections::BTreeMap;

use layerlens_core::psd::{
    Capability, DocumentInfo, ExportBlocker, LayerKind, ResourceUsage, Support,
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DocumentSummary {
    width: u32,
    height: u32,
    nodes: usize,
    text_layers: usize,
    character_runs: usize,
    paragraph_runs: usize,
    resources: ResourceUsage,
    preview: Capability,
    layer_kinds: Vec<KindCount>,
    export: ExportSummary,
}

#[derive(Serialize)]
struct KindCount {
    kind: LayerKind,
    count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportSummary {
    supported: usize,
    partial: usize,
    unsupported: usize,
    /// 多项拒绝可重叠，不能将计数相加作为节点总数。
    blockers: BTreeMap<ExportBlocker, usize>,
    bitmap_blockers: BTreeMap<ExportBlocker, usize>,
}

impl DocumentSummary {
    pub(super) fn from_info(info: &DocumentInfo) -> Self {
        let mut export = ExportSummary {
            supported: 0,
            partial: 0,
            unsupported: 0,
            blockers: BTreeMap::new(),
            bitmap_blockers: BTreeMap::new(),
        };
        for layer in &info.layers {
            match layer.export.status {
                Support::Supported => export.supported += 1,
                Support::Partial => export.partial += 1,
                Support::Unsupported => export.unsupported += 1,
            }
            for blocker in &layer.export_blockers {
                *export.blockers.entry(*blocker).or_default() += 1;
                if layer.kind == LayerKind::Bitmap {
                    *export.bitmap_blockers.entry(*blocker).or_default() += 1;
                }
            }
        }
        let texts: Vec<_> = info
            .layers
            .iter()
            .filter_map(|layer| layer.text.as_ref())
            .collect();
        Self {
            width: info.width,
            height: info.height,
            nodes: info.layers.len(),
            text_layers: texts.len(),
            character_runs: texts
                .iter()
                .filter_map(|text| text.style_runs.as_ref())
                .map(Vec::len)
                .sum(),
            paragraph_runs: texts
                .iter()
                .filter_map(|text| text.paragraph_runs.as_ref())
                .map(Vec::len)
                .sum(),
            resources: info.resources.clone(),
            preview: info.preview.clone(),
            layer_kinds: [
                LayerKind::Group,
                LayerKind::Bitmap,
                LayerKind::Text,
                LayerKind::Shape,
                LayerKind::SmartObject,
                LayerKind::Adjustment,
                LayerKind::Unknown,
            ]
            .into_iter()
            .map(|kind| KindCount {
                kind,
                count: info
                    .layers
                    .iter()
                    .filter(|layer| layer.kind == kind)
                    .count(),
            })
            .collect(),
            export,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layerlens_core::psd::{ParseLimits, PsdDocument};

    #[test]
    fn summary_counts_capabilities_without_exposing_design_content() {
        let document = PsdDocument::from_bytes(
            include_bytes!("../../tests/fixtures/psd/text-engine-72.psd"),
            ParseLimits::default(),
        )
        .unwrap();
        let info = document.info();
        let summary = DocumentSummary::from_info(info);
        assert_eq!(summary.nodes, info.layers.len());
        assert_eq!(
            summary.export.supported + summary.export.partial + summary.export.unsupported,
            summary.nodes
        );
        assert!(summary.character_runs > summary.text_layers);
        assert_eq!(summary.export.supported, 0);
        let json = serde_json::to_string(&summary).unwrap();
        for layer in &info.layers {
            assert!(!json.contains(&serde_json::to_string(&layer.name).unwrap()));
            if let Some(text) = &layer.text {
                assert!(!json.contains(&serde_json::to_string(&text.raw_text).unwrap()));
            }
        }
    }
}

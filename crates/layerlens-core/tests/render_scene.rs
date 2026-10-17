#![cfg(feature = "render-lab")]
use layerlens_core::psd::{ParseLimits, PsdDocument, PsdErrorCode, RenderContent};

#[test]
fn scene_opacity_does_not_grant_independent_export_or_multiply_alpha_twice() {
    let mut bytes = include_bytes!("fixtures/psd/bitmap-raw.psd").to_vec();
    let at = bytes.windows(8).position(|b| b == b"8BIMnorm").unwrap();
    bytes[at + 8] = 128;
    let document = PsdDocument::from_bytes(&bytes, ParseLimits::default()).unwrap();
    let layer = document
        .info()
        .layers
        .iter()
        .find(|l| l.opacity == 128)
        .unwrap();
    assert!(document.layer_png(layer.id).is_err());
    let scene = document.render_scene().unwrap();
    let rendered = scene.layers.iter().find(|l| l.id == layer.id.0).unwrap();
    assert_eq!(rendered.opacity, 128);
    let reference = PsdDocument::from_bytes(
        include_bytes!("fixtures/psd/bitmap-raw.psd"),
        ParseLimits::default(),
    )
    .unwrap()
    .render_scene()
    .unwrap();
    let RenderContent::Bitmap { png: actual } = &rendered.content else {
        panic!("expected bitmap");
    };
    let RenderContent::Bitmap { png: expected } = &reference
        .layers
        .iter()
        .find(|l| l.id == layer.id.0)
        .unwrap()
        .content
    else {
        panic!("expected reference bitmap");
    };
    assert_eq!(actual, expected, "源 alpha 不应预先乘图层 opacity");
}

#[test]
fn real_psd_supplies_geometry_text_pixels_and_hidden_ancestry() {
    let document = PsdDocument::from_bytes(
        include_bytes!("fixtures/psd/render-simple.psd"),
        ParseLimits::default(),
    )
    .unwrap();
    let scene = document.render_scene().unwrap();
    assert_eq!(
        (scene.width, scene.height, scene.layers.len()),
        (480, 320, 11)
    );
    let find = |name: &str| scene.layers.iter().find(|l| l.name == name).unwrap();
    assert!(!find("hidden-child").visible);
    assert!(find("hidden-child").parent_id.is_some());
    let RenderContent::Path { paths, color } = &find("blue-card").content else {
        panic!("expected source vector: {:?}", find("blue-card").content);
    };
    assert_eq!(*color, [40.0 / 255.0, 115.0 / 255.0, 210.0 / 255.0, 1.0]);
    assert!((paths[0].knots[0][2] - 24.0).abs() < 0.0001);
    assert!((paths[0].knots[0][3] - 170.0).abs() < 0.0001);
    let RenderContent::Text { data, .. } = &find("mixed").content else {
        panic!("expected source text");
    };
    assert_eq!(data.raw_text, "Same PSD / three engines");
    assert_eq!(data.transform, Some([1.0, 0.0, 0.0, 1.0, 24.0, 91.0]));
    assert_eq!(data.style_runs.as_ref().unwrap().len(), 2);
    let RenderContent::Bitmap { png } = &find("alpha-window").content else {
        panic!("expected alpha pixels");
    };
    let mut reader = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .unwrap();
    let mut bytes = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut bytes).unwrap();
    assert_eq!((info.width, info.height), (80, 64));
    assert_eq!(
        &bytes[(30 * 80 + 30) * 4..(30 * 80 + 30) * 4 + 4],
        &[240, 105, 50, 0]
    );
    assert_eq!(&bytes[0..4], &[240, 105, 50, 255]);
    assert_eq!(scene.source_sha256, document.source_sha256());
    assert!(
        scene
            .layers
            .iter()
            .filter(|l| l.visible)
            .all(|l| !matches!(l.content, RenderContent::Unsupported { .. }))
    );
}

#[test]
fn experimental_budget_does_not_expand_normal_document_support() {
    let document = PsdDocument::from_bytes(
        include_bytes!("fixtures/psd/viewer-scale.psd"),
        ParseLimits::default(),
    )
    .unwrap();
    assert_eq!(document.info().layers.len(), 512);
    assert_eq!(
        document.render_scene().unwrap_err().code,
        PsdErrorCode::ResourceLimit
    );
    let scene = document
        .render_scene_with_profile(layerlens_core::psd::RenderProfile::Bounded)
        .unwrap();
    assert_eq!(scene.layers.len(), 512);
    assert_eq!(scene.source_sha256, document.source_sha256());
}

#[test]
fn large_manifest_defers_pixels_and_keeps_source_projection_explicit() {
    use layerlens_core::psd::{LayerId, RenderProfile};
    let document = PsdDocument::from_bytes(
        include_bytes!("fixtures/psd/render-simple.psd"),
        ParseLimits::default(),
    )
    .unwrap();
    let small = document.render_scene().unwrap();
    let large = document
        .render_scene_with_profile(RenderProfile::Large)
        .unwrap();
    assert!(large.reference_png.is_empty());
    let bitmap = large
        .layers
        .iter()
        .find(|l| l.name == "alpha-window")
        .unwrap();
    assert!(matches!(&bitmap.content, RenderContent::Bitmap { png } if png.is_empty()));
    assert!(bitmap.composition_warning.is_none());
    let expected = small.layers.iter().find(|l| l.id == bitmap.id).unwrap();
    let RenderContent::Bitmap { png } = &expected.content else {
        panic!("expected bitmap");
    };
    assert_eq!(
        &document
            .render_resource(Some(LayerId(bitmap.id)), RenderProfile::Large)
            .unwrap(),
        png
    );
    assert!(
        document
            .render_resource(Some(LayerId(u32::MAX)), RenderProfile::Large)
            .is_err()
    );
    let projection = document
        .render_scene_with_profile(RenderProfile::LargeLayers)
        .unwrap();
    assert!(
        projection
            .layers
            .iter()
            .all(|l| l.composition_warning.is_some())
    );
    assert!(projection.reference_png.is_empty());
    assert!(
        document
            .layer_png(LayerId(
                small.layers.iter().find(|l| l.name == "mixed").unwrap().id
            ))
            .is_err(),
        "诊断不能开放文字独立素材导出"
    );
}

#[test]
fn vector_mask_registered_key_and_alias_preserve_the_same_source_geometry() {
    let original = include_bytes!("fixtures/psd/render-simple.psd");
    let mut alias = original.to_vec();
    let positions: Vec<_> = original
        .windows(8)
        .enumerate()
        .filter_map(|(i, b)| (b == b"8BIMvmsk").then_some(i + 4))
        .collect();
    assert_eq!(positions.len(), 6);
    for position in positions {
        alias[position..position + 4].copy_from_slice(b"vsms");
    }
    let first = PsdDocument::from_bytes(original, ParseLimits::default())
        .unwrap()
        .render_scene()
        .unwrap();
    let second = PsdDocument::from_bytes(&alias, ParseLimits::default())
        .unwrap()
        .render_scene()
        .unwrap();
    assert_eq!(
        serde_json::to_value(first.layers).unwrap(),
        serde_json::to_value(second.layers).unwrap()
    );
}

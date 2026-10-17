//! 显式本地样本的渲染诊断；先经核心预检，只输出属性与场景类型，不记录像素或完整文字。
use ag_psd::psd::{Layer, ReadOptions};
use layerlens_core::psd::{ParseLimits, PsdDocument, RenderContent, RenderProfile};
use serde_json::{Value, json};

fn layers(input: &[Layer]) -> Vec<Value> {
    input.iter().map(|l| {
        let a = &l.additional_info;
        json!({
            "name": a.name, "opacity": l.opacity, "blend": format!("{:?}", l.blend_mode),
            "fillOpacity": a.fill_opacity, "mask": format!("{:?}", a.mask),
            "realMask": format!("{:?}", a.real_mask), "diagnostics": format!("{:?}", a.diagnostics),
            "vector": format!("{:?}", a.vector_mask),
            "textBox": a.text.as_ref().map(|t| format!("{:?} {:?}", t.shape_type, t.box_bounds)),
            "children": layers(l.children.as_deref().unwrap_or_default())
        })
    }).collect()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut reports = Vec::new();
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let large = args.first().is_some_and(|a| a == "--large");
    if large {
        args.remove(0);
    }
    for file in args {
        let limits = if large {
            ParseLimits {
                max_file_bytes: 512 * 1024 * 1024,
                max_total_pixels: 128 * 1024 * 1024,
                ..Default::default()
            }
        } else {
            ParseLimits::default()
        };
        if std::fs::metadata(&file)?.len() > limits.max_file_bytes {
            return Err("源文件超过显式诊断额度".into());
        }
        let bytes = std::fs::read(&file)?;
        let document = match PsdDocument::from_bytes(&bytes, limits) {
            Ok(document) => document,
            Err(error) => {
                reports.push(json!({"file": file, "error":error.to_string()}));
                continue;
            }
        };
        if large {
            for profile in [RenderProfile::Large, RenderProfile::LargeLayers] {
                let scene = document.render_scene_with_profile(profile)?;
                reports.push(json!({"file":file,"profile":profile,"sha256":document.source_sha256(),"width":scene.width,"height":scene.height,"layers":scene.layers.iter().zip(&document.info().layers).map(|(l, source)| json!({"id":l.id,"sourceRecordIndex":source.source_record_index,"visible":l.visible,"opacity":l.opacity,"bounds":l.bounds,"warning":l.composition_warning,"content":match &l.content {RenderContent::Bitmap{..}=>"bitmap",RenderContent::Text{..}=>"text",RenderContent::Path{..}=>"path",RenderContent::Group=>"group",RenderContent::Unsupported{reason}=>reason}})).collect::<Vec<_>>() }));
            }
            continue;
        }
        let raw = ag_psd::read_psd(
            &bytes,
            &ReadOptions {
                use_raw_data: Some(true),
                strict: Some(true),
                throw_for_missing_features: Some(true),
                skip_thumbnail: Some(true),
                skip_linked_files_data: Some(true),
                allow_unsupported_features: true,
                total_memory_limit: Some(128 * 1024 * 1024),
                ..Default::default()
            },
        )?;
        let scene = document.render_scene_with_profile(RenderProfile::Bounded)?;
        reports.push(json!({
            "file": file, "sha256": document.source_sha256(), "raw": layers(raw.children.as_deref().unwrap_or_default()),
            "layers": scene.layers.iter().map(|l| json!({"id":l.id, "name":l.name, "content":match &l.content { RenderContent::Bitmap { .. } => "bitmap", RenderContent::Text { .. } => "text", RenderContent::Path { .. } => "path", RenderContent::Group => "group", RenderContent::Unsupported {reason} => reason } })).collect::<Vec<_>>()
        }));
    }
    println!("{}", serde_json::to_string_pretty(&reports)?);
    Ok(())
}

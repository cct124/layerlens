//! 实验 DTO 与正式类型同源，单独生成避免把实验契约引入正式前端。
use layerlens_core::psd::{
    RenderContent, RenderLayer, RenderPath, RenderProfile, RenderScene, RenderTextBox, TextData,
};
use std::{env, fs, io, path::PathBuf};
use ts_rs::{Config, TS};
fn main() -> io::Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let check = match args.as_slice() {
        [] => false,
        [flag] if flag == "--check" => true,
        _ => return Err(io::Error::other("仅支持 --check")),
    };
    let config = Config::default();
    let mut output = String::from(
        "// 由 export_render_bindings 从 Rust 自动生成，请勿手工编辑。\nimport type { Bounds, Capability, TextIndexMapping, TextStyleRun, ParagraphStyleRun, TextDiagnostic } from '../shared/api/generated';\n\n",
    );
    for declaration in [
        TextData::decl(&config),
        RenderPath::decl(&config),
        RenderContent::decl(&config),
        RenderLayer::decl(&config),
        RenderProfile::decl(&config),
        RenderTextBox::decl(&config),
        RenderScene::decl(&config),
    ] {
        output.push_str("export ");
        output.push_str(&declaration);
        output.push_str("\n\n");
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src/render-lab/generated.ts");
    if check {
        if fs::read_to_string(path)?.replace("\r\n", "\n") != output {
            return Err(io::Error::other(
                "实验 DTO 过期，请运行 bindings:render:generate",
            ));
        }
    } else {
        fs::write(path, output)?;
    }
    Ok(())
}

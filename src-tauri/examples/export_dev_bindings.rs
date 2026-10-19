//! 从桌面开发观测契约生成 TypeScript，--check 只读比较。
#[path = "../src/dev_mcp/ui/model.rs"]
#[allow(dead_code)] // 同一契约文件还包含仅由 Rust MCP 路由消费的输入类型。
mod model;
use model::*;
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
    let mut output = String::from("// 由 export_dev_bindings 从 Rust 自动生成，请勿手工编辑。\n\n");
    for declaration in [
        UiRect::decl(&config),
        UiDocument::decl(&config),
        UiSnapTarget::decl(&config),
        UiCanvas::decl(&config),
        UiTask::decl(&config),
        UiCleanup::decl(&config),
        UiContent::decl(&config),
        UiInputEvidence::decl(&config),
        UiWindowControl::decl(&config),
        UiWindow::decl(&config),
        UiRenderSurface::decl(&config),
        UiRenderLab::decl(&config),
        UiRenderSample::decl(&config),
        UiObservation::decl(&config),
    ] {
        output.push_str("export ");
        for line in declaration.lines() {
            output.push_str(line.trim_end());
            output.push('\n');
        }
        output.push('\n');
    }
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/shared/api/devGenerated.ts");
    if check {
        if fs::read_to_string(file)?.replace("\r\n", "\n") != output {
            return Err(io::Error::other(
                "开发观测 DTO 过期，请运行 npm run bindings:dev:generate",
            ));
        }
    } else {
        fs::write(file, output)?;
    }
    Ok(())
}

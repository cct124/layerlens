//! 从 Rust 边界类型生成前端声明；`--check` 只比较文件，不修改工作区。

use std::{env, error::Error, fs, io, path::PathBuf};

use layerlens_core::documents::{LayerDetails, LayerPage, LayerSummary, TextSlice};
use layerlens_core::psd::*;
use layerlens_core::workspace_contract::*;
use layerlens_core::{
    AppInfo, AppInfoRequest, CommandError, CommandErrorCode, IPC_PROTOCOL_VERSION,
};
use ts_rs::{Config, TS};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let check_only = match args.as_slice() {
        [] => false,
        [flag] if flag == "--check" => true,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "用法：cargo run -p layerlens-core --example export_bindings [-- --check]",
            )
            .into());
        }
    };

    // 从 crate 位置定位，允许在任意工作目录通过 --manifest-path 调用。
    let output =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src/shared/api/generated.ts");
    let config = Config::default();
    let declarations = [
        AppInfoRequest::decl(&config),
        AppInfo::decl(&config),
        CommandErrorCode::decl(&config),
        CommandError::decl(&config),
        WorkspaceAction::decl(&config),
        WorkspaceRequest::decl(&config),
        PreviewRequest::decl(&config),
        WorkspaceErrorCode::decl(&config),
        WorkspaceError::decl(&config),
        WorkspaceDocument::decl(&config),
        WorkspaceJobPhase::decl(&config),
        WorkspaceJob::decl(&config),
        WorkspacePreviewState::decl(&config),
        WorkspacePreview::decl(&config),
        WorkspaceNotice::decl(&config),
        WorkspaceResources::decl(&config),
        WorkspaceSnapshot::decl(&config),
        LayerId::decl(&config),
        Bounds::decl(&config),
        LayerKind::decl(&config),
        ExportBlocker::decl(&config),
        Support::decl(&config),
        Capability::decl(&config),
        TextIndexMapping::decl(&config),
        TextProperty::<TextMetric>::decl(&config),
        TextUnit::decl(&config),
        TextMetric::decl(&config),
        TextFont::decl(&config),
        TextColor::decl(&config),
        CharacterStyle::decl(&config),
        TextStyleRun::decl(&config),
        ParagraphAlignment::decl(&config),
        ParagraphStyle::decl(&config),
        ParagraphStyleRun::decl(&config),
        TextDiagnosticCode::decl(&config),
        TextDiagnostic::decl(&config),
        LayerSummary::decl(&config),
        LayerPage::decl(&config),
        TextSlice::decl(&config),
        LayerDetails::decl(&config),
        LayerQuery::decl(&config),
        LayerRequest::decl(&config),
        LayerResult::decl(&config),
        LayerResponse::decl(&config),
    ];
    let mut expected = String::from(
        "// 此文件由 Rust DTO 自动生成，请勿手工编辑。\n\
         // 更新：cargo run -p layerlens-core --example export_bindings\n\n",
    );
    expected.push_str(&format!(
        "export const IPC_PROTOCOL_VERSION = {IPC_PROTOCOL_VERSION};\n\n"
    ));
    for declaration in declarations {
        expected.push_str("export ");
        // ts-rs 的多行声明可能保留行尾空格；在生成源统一规范化。
        for line in declaration.lines() {
            expected.push_str(line.trim_end());
            expected.push('\n');
        }
        expected.push('\n');
    }

    let expected = format!("{}\n", expected.trim_end());

    if check_only {
        let actual = fs::read_to_string(&output)?;
        // Git 在 Windows 上可能检出 CRLF；只忽略换行编码，不忽略声明变化。
        if actual.replace("\r\n", "\n") != expected {
            return Err(io::Error::other(
                "前端类型已过期，请运行 cargo run -p layerlens-core --example export_bindings。",
            )
            .into());
        }
        println!("Rust 与 TypeScript 边界类型一致。");
    } else {
        let parent = output
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "生成文件缺少父目录"))?;
        fs::create_dir_all(parent)?;
        fs::write(&output, expected)?;
        println!("已生成 {}", output.display());
    }

    Ok(())
}

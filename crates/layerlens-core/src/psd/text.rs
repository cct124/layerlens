//! 从候选保留的原始 EngineData 读取文字；不使用其裁剪、默认值或去重后的样式。
//! 区间必须覆盖引擎原文且可映射到 TySh 原文；失败只关闭对应文字能力。

mod properties;

#[cfg(test)]
mod tests;

use ag_psd::{EngineValue, psd::LayerTextData};

use super::{
    capability::Capability,
    error::{PsdError, PsdErrorCode},
    text_model::*,
};

// 防止一个文字层生成无界属性、来源路径和诊断；不是进程内存上限。
const MAX_STYLE_RUNS: usize = 16_384;

pub(super) fn normalize(input: &LayerTextData) -> Result<Option<TextData>, PsdError> {
    let Some(raw) = &input.raw_text else {
        return Ok(None);
    };
    let utf16_length = u32::try_from(raw.encode_utf16().count())
        .map_err(|_| invalid_candidate("文字 UTF-16 长度超出范围"))?;
    let transform = input
        .transform
        .as_ref()
        .map(|values| {
            <[f64; 6]>::try_from(values.as_slice())
                .ok()
                .filter(|matrix| matrix.iter().all(|v| v.is_finite()))
                .ok_or_else(|| invalid_candidate("文字矩阵必须有六个有限值"))
        })
        .transpose()?;
    let mut text = TextData {
        raw_text: raw.clone(),
        utf16_length,
        transform,
        normalization_changed: *raw != input.text,
        styles: Capability::unsupported("没有可映射到 TySh 原文的 EngineData 样式"),
        index_mapping: None,
        style_runs: None,
        paragraph_runs: None,
        diagnostics: Vec::new(),
    };
    let Some(engine) = &input.raw_engine_data else {
        report(
            &mut text.diagnostics,
            TextDiagnosticCode::Missing,
            "EngineData",
            "没有 EngineData；不补默认样式",
        );
        return Ok(Some(text));
    };
    let engine_dict = get(engine, "EngineDict");
    let editor_value = engine_dict
        .and_then(|v| get(v, "Editor"))
        .and_then(|v| get(v, "Text"));
    let Some(editor) = editor_value.and_then(string) else {
        report(
            &mut text.diagnostics,
            if editor_value.is_some() {
                TextDiagnosticCode::Invalid
            } else {
                TextDiagnosticCode::Missing
            },
            "EngineDict.Editor.Text",
            "引擎原文缺失或类型无效，不能映射区间",
        );
        return Ok(Some(text));
    };
    // Photoshop 的引擎可能额外保存一个段落结束 CR。仅接受明确的一字符后缀；
    // 不 trim，不折叠 CRLF，也不让引擎结束符吞掉 TySh 自身的尾随换行。
    text.index_mapping = if editor == raw {
        Some(TextIndexMapping::Exact)
    } else if editor.strip_suffix('\r') == Some(raw.as_str()) {
        Some(TextIndexMapping::TrailingParagraphTerminator)
    } else {
        report(
            &mut text.diagnostics,
            TextDiagnosticCode::TextMismatch,
            "EngineDict.Editor.Text",
            "引擎与 TySh 原文不一致，样式区间不可直接映射",
        );
        return Ok(Some(text));
    };
    let units: Vec<_> = editor.encode_utf16().collect();
    let fonts = get(engine, "ResourceDict")
        .and_then(|v| get(v, "FontSet"))
        .and_then(array);
    let character = engine_dict.and_then(|v| get(v, "StyleRun"));
    let paragraph = engine_dict.and_then(|v| get(v, "ParagraphRun"));
    text.style_runs = runs(
        character,
        "StyleRun",
        &units,
        utf16_length,
        &mut text.diagnostics,
    )
    .map(|runs| {
        let default = character.and_then(|v| get(v, "DefaultRunData"));
        runs.into_iter()
            .map(|run| TextStyleRun {
                start: run.start,
                end: run.end,
                style: properties::character(
                    run.data,
                    default,
                    fonts,
                    run.index,
                    &mut text.diagnostics,
                ),
            })
            .collect()
    });
    text.paragraph_runs = runs(
        paragraph,
        "ParagraphRun",
        &units,
        utf16_length,
        &mut text.diagnostics,
    )
    .map(|runs| {
        let default = paragraph.and_then(|v| get(v, "DefaultRunData"));
        runs.into_iter()
            .map(|run| ParagraphStyleRun {
                start: run.start,
                end: run.end,
                style: properties::paragraph(run.data, default, run.index, &mut text.diagnostics),
            })
            .collect()
    });
    if text.style_runs.is_some() || text.paragraph_runs.is_some() {
        text.styles = Capability::partial(
            "已校验原文区间并读取显式字符／段落属性；度量单位、字体与颜色保真仍待独立参考，未读取属性见诊断",
        );
        report(
            &mut text.diagnostics,
            TextDiagnosticCode::Unverified,
            "EngineData",
            "度量保留原始引擎数值与未验证单位，不按 DPI、矩阵或 CSS 比例换算；RGB 未做 ICC 转换，字体可用性与字重未推断",
        );
    }
    Ok(Some(text))
}

struct Run<'a> {
    start: u32,
    end: u32,
    index: usize,
    data: &'a EngineValue,
}

fn runs<'a>(
    value: Option<&'a EngineValue>,
    kind: &str,
    units: &[u16],
    raw_length: u32,
    diagnostics: &mut Vec<TextDiagnostic>,
) -> Option<Vec<Run<'a>>> {
    let path = format!("EngineDict.{kind}");
    let Some(value) = value else {
        report(
            diagnostics,
            TextDiagnosticCode::Missing,
            &path,
            "缺少样式区间",
        );
        return None;
    };
    let arrays = get(value, "RunArray")
        .and_then(array)
        .zip(get(value, "RunLengthArray").and_then(array));
    let Some((records, lengths)) = arrays else {
        report(
            diagnostics,
            TextDiagnosticCode::InvalidRange,
            &path,
            "区间与长度必须为数组",
        );
        return None;
    };
    if records.len() != lengths.len() || records.len() > MAX_STYLE_RUNS {
        report(
            diagnostics,
            TextDiagnosticCode::InvalidRange,
            &path,
            "区间数量不匹配或超过每类 16384 段上限",
        );
        return None;
    }
    let mut position = 0u32;
    let mut output = Vec::with_capacity(records.len());
    for (index, (record, length)) in records.iter().zip(lengths).enumerate() {
        let end = integer(length)
            .filter(|length| *length > 0)
            .and_then(|length| position.checked_add(length));
        let Some(end) =
            end.filter(|end| (*end as usize) <= units.len() && boundary(units, *end as usize))
        else {
            report(
                diagnostics,
                TextDiagnosticCode::InvalidRange,
                &format!("{path}.RunLengthArray[{index}]"),
                "长度须为正整数且不越界、不溢出、不拆分 UTF-16 代理对",
            );
            return None;
        };
        if !matches!(record, EngineValue::Dict(_)) {
            report(
                diagnostics,
                TextDiagnosticCode::InvalidRange,
                &format!("{path}.RunArray[{index}]"),
                "样式项必须为字典",
            );
            return None;
        }
        if position < raw_length {
            output.push(Run {
                start: position,
                end: end.min(raw_length),
                index,
                data: record,
            });
        }
        position = end;
    }
    if position as usize != units.len() {
        report(
            diagnostics,
            TextDiagnosticCode::InvalidRange,
            &path,
            "区间总长必须完整覆盖引擎原文",
        );
        return None;
    }
    Some(output)
}

fn boundary(units: &[u16], index: usize) -> bool {
    index == 0
        || index == units.len()
        || !((0xd800..=0xdbff).contains(&units[index - 1])
            && (0xdc00..=0xdfff).contains(&units[index]))
}

fn get<'a>(value: &'a EngineValue, key: &str) -> Option<&'a EngineValue> {
    match value {
        EngineValue::Dict(entries) => entries.iter().find(|(name, _)| name == key).map(|(_, v)| v),
        _ => None,
    }
}

fn array(value: &EngineValue) -> Option<&[EngineValue]> {
    match value {
        EngineValue::Array(values) => Some(values),
        _ => None,
    }
}

fn string(value: &EngineValue) -> Option<&str> {
    match value {
        EngineValue::Str(value) => Some(value),
        _ => None,
    }
}

fn number(value: &EngineValue) -> Option<f64> {
    match value {
        EngineValue::Number(value) if value.is_finite() => Some(*value),
        _ => None,
    }
}

fn integer(value: &EngineValue) -> Option<u32> {
    number(value)
        .filter(|v| *v >= 0.0 && *v <= f64::from(u32::MAX) && v.fract() == 0.0)
        .map(|v| v as u32)
}

fn report(
    diagnostics: &mut Vec<TextDiagnostic>,
    code: TextDiagnosticCode,
    path: &str,
    message: &str,
) {
    diagnostics.push(TextDiagnostic {
        code,
        path: path.into(),
        message: message.into(),
    });
}

fn invalid_candidate(message: &str) -> PsdError {
    PsdError::new(PsdErrorCode::ParseFailed, message)
}

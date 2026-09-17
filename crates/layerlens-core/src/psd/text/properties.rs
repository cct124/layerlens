//! 读取原始样式字典中的显式属性；仅继承同一 Run 的 DefaultRunData，绝不猜测默认字体。

use super::{array, get, integer, number, report, string};
use crate::psd::text_model::*;
use ag_psd::EngineValue;

pub(super) fn character(
    run: &EngineValue,
    default: Option<&EngineValue>,
    fonts: Option<&[EngineValue]>,
    index: usize,
    diagnostics: &mut Vec<TextDiagnostic>,
) -> CharacterStyle {
    let mut fields = Fields::new(
        run,
        default,
        "StyleRun",
        ["StyleSheet", "StyleSheetData"],
        index,
        diagnostics,
    );
    fields.unknown(&[
        "Font",
        "FontSize",
        "FillColor",
        "FillFlag",
        "StrokeFlag",
        "FauxBold",
        "FauxItalic",
        "AutoLeading",
        "Leading",
        "Tracking",
        "BaselineShift",
        "HorizontalScale",
        "VerticalScale",
    ]);
    CharacterStyle {
        font: fields.property("Font", true, |value| {
            let index = integer(value)?;
            let font = fonts?.get(index as usize)?;
            let name = get(font, "Name")
                .and_then(string)
                .filter(|name| !name.is_empty())?;
            Some(TextFont {
                index,
                name: name.into(),
                family: optional_name(font, "FamilyName").ok()?,
                style: optional_name(font, "StyleName").ok()?,
            })
        }),
        font_size: fields.property("FontSize", true, |value| metric(value, |v| v > 0.0)),
        fill_color: fields.decoded_property("FillColor", true, color),
        fill_enabled: fields.property("FillFlag", false, boolean),
        stroke_enabled: fields.property("StrokeFlag", false, boolean),
        faux_bold: fields.property("FauxBold", false, boolean),
        faux_italic: fields.property("FauxItalic", false, boolean),
        auto_leading: fields.property("AutoLeading", false, boolean),
        leading: fields.property("Leading", false, |value| metric(value, |v| v >= 0.0)),
        tracking: fields.property("Tracking", false, |value| metric(value, |_| true)),
        baseline_shift: fields.property("BaselineShift", false, |value| metric(value, |_| true)),
        horizontal_scale: fields.property("HorizontalScale", false, positive),
        vertical_scale: fields.property("VerticalScale", false, positive),
    }
}

pub(super) fn paragraph(
    run: &EngineValue,
    default: Option<&EngineValue>,
    index: usize,
    diagnostics: &mut Vec<TextDiagnostic>,
) -> ParagraphStyle {
    let mut fields = Fields::new(
        run,
        default,
        "ParagraphRun",
        ["ParagraphSheet", "Properties"],
        index,
        diagnostics,
    );
    fields.unknown(&[
        "Justification",
        "AutoLeading",
        "FirstLineIndent",
        "StartIndent",
        "EndIndent",
        "SpaceBefore",
        "SpaceAfter",
    ]);
    ParagraphStyle {
        alignment: fields.property("Justification", true, |v| match integer(v)? {
            0 => Some(ParagraphAlignment::Left),
            1 => Some(ParagraphAlignment::Right),
            2 => Some(ParagraphAlignment::Center),
            3 => Some(ParagraphAlignment::JustifyLeft),
            4 => Some(ParagraphAlignment::JustifyRight),
            5 => Some(ParagraphAlignment::JustifyCenter),
            6 => Some(ParagraphAlignment::JustifyAll),
            _ => None,
        }),
        auto_leading: fields.property("AutoLeading", false, positive),
        first_line_indent: fields.property("FirstLineIndent", false, |v| metric(v, |_| true)),
        start_indent: fields.property("StartIndent", false, |v| metric(v, |_| true)),
        end_indent: fields.property("EndIndent", false, |v| metric(v, |_| true)),
        space_before: fields.property("SpaceBefore", false, |v| metric(v, |n| n >= 0.0)),
        space_after: fields.property("SpaceAfter", false, |v| metric(v, |n| n >= 0.0)),
    }
}

struct Fields<'a, 'd> {
    local: Option<&'a EngineValue>,
    default: Option<&'a EngineValue>,
    path: String,
    default_path: String,
    diagnostics: &'d mut Vec<TextDiagnostic>,
}

impl<'a, 'd> Fields<'a, 'd> {
    fn new(
        run: &'a EngineValue,
        default: Option<&'a EngineValue>,
        kind: &str,
        keys: [&str; 2],
        index: usize,
        diagnostics: &'d mut Vec<TextDiagnostic>,
    ) -> Self {
        let path = format!(
            "EngineDict.{kind}.RunArray[{index}].{}.{}",
            keys[0], keys[1]
        );
        let default_path = format!("EngineDict.{kind}.DefaultRunData.{}.{}", keys[0], keys[1]);
        let local = dictionary(run, keys, &path, diagnostics);
        // 显式非法容器不按“缺失”继承，以免掩盖源损坏。
        let default = if local.is_err() || has_reference(run, keys) {
            None
        } else {
            default.and_then(|v| {
                dictionary(v, keys, &default_path, diagnostics)
                    .ok()
                    .flatten()
            })
        };
        Self {
            local: local.ok().flatten(),
            default,
            path,
            default_path,
            diagnostics,
        }
    }

    fn unknown(&mut self, supported: &[&str]) {
        for (value, path) in [(self.local, &self.path), (self.default, &self.default_path)] {
            if let Some(EngineValue::Dict(entries)) = value
                && entries
                    .iter()
                    .any(|(key, _)| !supported.contains(&key.as_str()))
            {
                report(
                    self.diagnostics,
                    TextDiagnosticCode::Unsupported,
                    path,
                    "存在本轮未读取的样式属性或样式表引用；不展开隐式继承、不声明完整排版样式",
                );
            }
        }
    }

    fn property<T>(
        &mut self,
        key: &str,
        required: bool,
        decode: impl FnOnce(&EngineValue) -> Option<T>,
    ) -> Option<TextProperty<T>> {
        self.decoded_property(key, required, |value| {
            decode(value).ok_or(TextDiagnosticCode::Invalid)
        })
    }

    fn decoded_property<T>(
        &mut self,
        key: &str,
        required: bool,
        decode: impl FnOnce(&EngineValue) -> Result<T, TextDiagnosticCode>,
    ) -> Option<TextProperty<T>> {
        let entry = self
            .local
            .and_then(|v| get(v, key))
            .map(|v| (v, &self.path))
            .or_else(|| {
                self.default
                    .and_then(|v| get(v, key))
                    .map(|v| (v, &self.default_path))
            });
        let Some((value, path)) = entry else {
            if required {
                report(
                    self.diagnostics,
                    TextDiagnosticCode::Missing,
                    &format!("{}.{key}", self.path),
                    "源中没有显式属性或可用的 DefaultRunData 属性；不补默认值",
                );
            }
            return None;
        };
        let source = format!("{path}.{key}");
        match decode(value) {
            Ok(value) => Some(TextProperty { value, source }),
            Err(code) => {
                let message = if code == TextDiagnosticCode::Unsupported {
                    "该颜色模型尚未支持；不按 RGB 解释或猜测转换"
                } else {
                    "属性类型、有限数值、枚举范围或字体引用无效；仅该属性返回 null"
                };
                report(self.diagnostics, code, &source, message);
                None
            }
        }
    }
}

fn dictionary<'a>(
    run: &'a EngineValue,
    keys: [&str; 2],
    path: &str,
    diagnostics: &mut Vec<TextDiagnostic>,
) -> Result<Option<&'a EngineValue>, ()> {
    let mut current = run;
    for key in keys {
        if !matches!(current, EngineValue::Dict(_)) {
            report(
                diagnostics,
                TextDiagnosticCode::Invalid,
                path,
                "样式容器必须为字典",
            );
            return Err(());
        }
        if let EngineValue::Dict(entries) = current
            && entries.iter().any(|(name, _)| name != key)
        {
            report(
                diagnostics,
                TextDiagnosticCode::Unsupported,
                path,
                "存在未读取的样式容器属性或引用；引用不参与隐式继承",
            );
        }
        match get(current, key) {
            Some(value) => current = value,
            None => return Ok(None),
        }
    }
    if !matches!(current, EngineValue::Dict(_)) {
        report(
            diagnostics,
            TextDiagnosticCode::Invalid,
            path,
            "样式属性必须为字典",
        );
        return Err(());
    }
    Ok(Some(current))
}

fn metric(value: &EngineValue, valid: impl FnOnce(f64) -> bool) -> Option<TextMetric> {
    number(value).filter(|v| valid(*v)).map(|value| TextMetric {
        value,
        unit: TextUnit::UnverifiedEngine,
    })
}

fn positive(value: &EngineValue) -> Option<f64> {
    number(value).filter(|v| *v > 0.0)
}

fn boolean(value: &EngineValue) -> Option<bool> {
    match value {
        EngineValue::Bool(v) => Some(*v),
        _ => None,
    }
}

fn has_reference(run: &EngineValue, keys: [&str; 2]) -> bool {
    let sheet = get(run, keys[0]);
    let data = sheet.and_then(|v| get(v, keys[1]));
    [sheet, data]
        .into_iter()
        .flatten()
        .any(|v| get(v, "StyleSheet").is_some())
        || [Some(run), sheet, data].into_iter().flatten().any(|v| {
            ["DefaultStyleSheet", "BaseParent"]
                .iter()
                .any(|key| get(v, key).is_some())
        })
}

fn optional_name(font: &EngineValue, key: &str) -> Result<Option<String>, ()> {
    get(font, key)
        .map(|v| {
            string(v)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .ok_or(())
        })
        .transpose()
}

fn color(value: &EngineValue) -> Result<TextColor, TextDiagnosticCode> {
    let kind = get(value, "Type")
        .and_then(integer)
        .ok_or(TextDiagnosticCode::Invalid)?;
    if kind != 1 {
        return Err(TextDiagnosticCode::Unsupported);
    }
    rgb_color(value).ok_or(TextDiagnosticCode::Invalid)
}

fn rgb_color(value: &EngineValue) -> Option<TextColor> {
    let values = get(value, "Values").and_then(array)?;
    let [alpha, red, green, blue] = values else {
        return None;
    };
    let component = |v| number(v).filter(|v| (0.0..=1.0).contains(v));
    Some(TextColor {
        alpha: component(alpha)?,
        red: component(red)?,
        green: component(green)?,
        blue: component(blue)?,
    })
}

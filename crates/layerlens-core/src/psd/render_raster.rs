//! 实验场景的像素读取与简单蒙版合成；不授予独立素材导出权限。
use super::*;
use ag_psd::psd::LayerMaskData;

/// 仅诊断原始图层通道；不应用蒙版、效果、剪贴或组语义，不授予素材导出能力。
pub(super) fn source_bitmap(
    document: &PsdDocument,
    info: &LayerInfo,
    raw: &Layer,
    deferred: bool,
) -> Result<RenderContent, PsdError> {
    let record = document
        .render_records
        .get(info.source_record_index as usize)
        .ok_or_else(layer_not_found)?;
    if !record.has_rgb || info.bounds.width == 0 || info.bounds.height == 0 {
        return Ok(unsupported("源图层没有可诊断的 RGB 像素"));
    }
    document.check_decode_budget(info.bounds.width, info.bounds.height)?;
    if deferred {
        return Ok(RenderContent::Bitmap { png: Vec::new() });
    }
    let Some(pixels) =
        ag_psd::get_layer_image_data(raw).map_err(|e| parser_error("读取诊断图层通道", e))?
    else {
        return Ok(unsupported("源图层没有可诊断的像素"));
    };
    Ok(RenderContent::Bitmap {
        png: crate::psd::png_output::encode(
            pixels,
            info.bounds.width,
            info.bounds.height,
            32 * 1024 * 1024,
        )?,
    })
}

pub(super) fn bitmap(
    document: &PsdDocument,
    info: &LayerInfo,
    raw: &Layer,
    record: &preflight::LayerRecord,
    defer_pixels: bool,
) -> Result<RenderContent, PsdError> {
    if info.bounds.width == 0 || info.bounds.height == 0 {
        return Ok(unsupported("空位图没有绘制像素"));
    }
    let mask = raw.additional_info.mask.as_ref();
    if let Some(mask) = mask
        && (record.mask_flags.is_none_or(|f| f & !3 != 0)
            || mask.from_vector_data == Some(true)
            || mask.user_mask_density.is_some_and(|v| v != 1.0)
            || mask.user_mask_feather.is_some_and(|v| v != 0.0)
            || mask.vector_mask_density.is_some()
            || mask.vector_mask_feather.is_some())
    {
        return Ok(unsupported("蒙版反向、羽化、密度或矢量来源尚未支持"));
    }
    document.check_decode_budget(info.bounds.width, info.bounds.height)?;
    if defer_pixels {
        return Ok(RenderContent::Bitmap { png: Vec::new() });
    }
    let Some(mut pixels) =
        ag_psd::get_layer_image_data(raw).map_err(|e| parser_error("读取实验图层像素", e))?
    else {
        return Ok(unsupported("图层没有独立像素"));
    };
    if let Some(mask) = mask.filter(|m| m.disabled != Some(true)) {
        let bounds = mask_bounds(mask)?;
        let total =
            u64::from(bounds.width) * u64::from(bounds.height) * 4 + pixels.data.len() as u64;
        if total > document.max_decoded_bytes {
            return Err(limited());
        }
        let Some(alpha) = ag_psd::get_layer_mask_image_data(raw)
            .map_err(|e| parser_error("读取实验蒙版像素", e))?
        else {
            return Ok(unsupported("蒙版没有可读取通道"));
        };
        apply_mask(&mut pixels, info.bounds, &alpha, bounds, mask)?;
    }
    Ok(RenderContent::Bitmap {
        png: crate::psd::png_output::encode(
            pixels,
            info.bounds.width,
            info.bounds.height,
            32 * 1024 * 1024,
        )?,
    })
}
fn mask_bounds(mask: &LayerMaskData) -> Result<Bounds, PsdError> {
    let values = [mask.left, mask.top, mask.right, mask.bottom];
    if values.iter().any(|v| {
        v.is_none_or(|n| !n.is_finite() || n.fract() != 0.0 || n.abs() > f64::from(i32::MAX))
    }) {
        return Err(limited());
    }
    let [left, top, right, bottom] = values.map(|v| v.unwrap_or_default() as i32);
    Ok(Bounds {
        x: left,
        y: top,
        width: u32::try_from(i64::from(right) - i64::from(left)).map_err(|_| limited())?,
        height: u32::try_from(i64::from(bottom) - i64::from(top)).map_err(|_| limited())?,
    })
}
fn apply_mask(
    pixels: &mut ag_psd::PixelData,
    layer: Bounds,
    alpha: &ag_psd::PixelData,
    mut bounds: Bounds,
    mask: &LayerMaskData,
) -> Result<(), PsdError> {
    if pixels.width != layer.width
        || pixels.height != layer.height
        || pixels.data.len() as u64 != u64::from(layer.width) * u64::from(layer.height) * 4
        || alpha.width != bounds.width
        || alpha.height != bounds.height
        || alpha.data.len() as u64 != u64::from(bounds.width) * u64::from(bounds.height) * 4
    {
        return Err(limited());
    }
    let default = mask
        .default_color
        .filter(|v| v.is_finite() && v.fract() == 0.0 && (0.0..=255.0).contains(v))
        .ok_or_else(limited)? as u32;
    if mask.position_relative_to_layer == Some(true) {
        bounds.x = bounds.x.checked_add(layer.x).ok_or_else(limited)?;
        bounds.y = bounds.y.checked_add(layer.y).ok_or_else(limited)?;
    }
    // PSD 蒙版矩形外使用 default color，矩形内灰度乘源 alpha；图层 opacity 留给渲染器一次应用。
    for (i, rgba) in pixels.data.chunks_exact_mut(4).enumerate() {
        let x =
            i64::from(layer.x) + (i as u64 % u64::from(layer.width)) as i64 - i64::from(bounds.x);
        let y =
            i64::from(layer.y) + (i as u64 / u64::from(layer.width)) as i64 - i64::from(bounds.y);
        let value =
            if x >= 0 && y >= 0 && x < i64::from(bounds.width) && y < i64::from(bounds.height) {
                u32::from(alpha.data[(y as usize * bounds.width as usize + x as usize) * 4])
            } else {
                default
            };
        rgba[3] = ((u32::from(rgba[3]) * value + 127) / 255) as u8;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pixels(width: u32, data: &[u8]) -> ag_psd::PixelData {
        ag_psd::PixelData {
            width,
            height: 1,
            data: data.into(),
        }
    }
    #[test]
    fn mask_uses_source_alpha_default_color_and_document_coordinates() {
        let mut source = pixels(3, &[20, 30, 40, 128, 20, 30, 40, 128, 20, 30, 40, 255]);
        let mask = LayerMaskData {
            default_color: Some(0.0),
            ..Default::default()
        };
        apply_mask(
            &mut source,
            Bounds {
                x: -2,
                y: 4,
                width: 3,
                height: 1,
            },
            &pixels(1, &[128, 128, 128, 255]),
            Bounds {
                x: -1,
                y: 4,
                width: 1,
                height: 1,
            },
            &mask,
        )
        .unwrap();
        assert_eq!(
            source.data,
            vec![20, 30, 40, 0, 20, 30, 40, 64, 20, 30, 40, 0]
        );
    }
    #[test]
    fn relative_mask_and_white_default_preserve_outside_pixels() {
        let mut source = pixels(2, &[20, 30, 40, 255, 20, 30, 40, 128]);
        let mask = LayerMaskData {
            default_color: Some(255.0),
            position_relative_to_layer: Some(true),
            ..Default::default()
        };
        apply_mask(
            &mut source,
            Bounds {
                x: 10,
                y: 20,
                width: 2,
                height: 1,
            },
            &pixels(1, &[0, 0, 0, 255]),
            Bounds {
                x: 1,
                y: 0,
                width: 1,
                height: 1,
            },
            &mask,
        )
        .unwrap();
        assert_eq!(source.data, vec![20, 30, 40, 255, 20, 30, 40, 0]);
    }
    #[test]
    fn malformed_dimensions_and_missing_default_cannot_be_used_as_mask() {
        let mut source = pixels(1, &[20, 30, 40, 255]);
        let bounds = Bounds {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        assert!(
            apply_mask(
                &mut source,
                bounds,
                &pixels(1, &[0, 0, 0, 255]),
                bounds,
                &LayerMaskData::default()
            )
            .is_err()
        );
        assert!(
            apply_mask(
                &mut source,
                bounds,
                &pixels(2, &[0]),
                bounds,
                &LayerMaskData {
                    default_color: Some(0.0),
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}

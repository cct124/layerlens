//! 在调用候选解析器前校验有界的 PSD v1 子集；借用输入切片，不按未核验的长度分配。
//! 只允许已经审查的资源和图层标签，复杂 descriptor、压缩及蒙版留待独立样本验证。

use super::{
    error::{PsdError, PsdErrorCode},
    model::ParseLimits,
};

const MAX_DIMENSION: u32 = 30_000;
const MAX_GROUP_DEPTH: u32 = 64;

#[derive(Debug)]
pub(super) struct Header {
    pub width: u32,
    pub height: u32,
    pub has_composite: bool,
    pub global_alpha: bool,
}

fn invalid(message: &str) -> PsdError {
    PsdError::new(PsdErrorCode::InvalidInput, message)
}

fn unsupported(message: &str) -> PsdError {
    PsdError::new(PsdErrorCode::Unsupported, message)
}

fn limited(message: &str) -> PsdError {
    PsdError::new(PsdErrorCode::ResourceLimit, message)
}

struct Cursor<'a> {
    remaining: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { remaining: bytes }
    }

    fn take(&mut self, size: usize) -> Result<&'a [u8], PsdError> {
        let bytes = self
            .remaining
            .get(..size)
            .ok_or_else(|| invalid("PSD 区段或通道数据截断"))?;
        self.remaining = &self.remaining[size..];
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, PsdError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, PsdError> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, PsdError> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn section(&mut self) -> Result<Cursor<'a>, PsdError> {
        let size = self.u32()? as usize;
        Ok(Cursor::new(self.take(size)?))
    }

    fn signature(&mut self, expected: &[u8; 4]) -> Result<(), PsdError> {
        if self.take(4)? != expected {
            return Err(invalid("PSD 区段签名无效"));
        }
        Ok(())
    }

    fn padding(&mut self, size: usize) -> Result<(), PsdError> {
        if self.take(size)?.iter().any(|byte| *byte != 0) {
            return Err(invalid("PSD 对齐填充必须为零"));
        }
        Ok(())
    }

    fn pascal_name(&mut self, alignment: usize) -> Result<(), PsdError> {
        let length = usize::from(self.u8()?);
        self.take(length)?;
        self.padding((alignment - (length + 1) % alignment) % alignment)
    }
}

/// 限定所有外部长度和像素预算后，才允许候选库构造模型及按需解码所需的通道副本。
pub(super) fn inspect(bytes: &[u8], limits: ParseLimits) -> Result<Header, PsdError> {
    if bytes.len() as u64 > limits.max_file_bytes {
        return Err(limited("PSD 文件超过字节预算"));
    }
    let mut input = Cursor::new(bytes);
    input.signature(b"8BPS")?;
    if input.u16()? != 1 {
        return Err(unsupported("本轮只验证 PSD v1，尚不支持 PSB"));
    }
    input.padding(6)?;
    let channels = input.u16()?;
    let height = input.u32()?;
    let width = input.u32()?;
    let depth = input.u16()?;
    let mode = input.u16()?;
    if !(3..=4).contains(&channels) || depth != 8 || mode != 3 {
        return Err(unsupported("本轮只验证具有 3 或 4 个通道的 RGB/8 位 PSD"));
    }
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(invalid("PSD v1 画布尺寸必须在 1–30000 像素内"));
    }
    let mut pixels = 0;
    add_pixels(&mut pixels, width, height, limits)?;
    if !input.section()?.remaining.is_empty() {
        return Err(unsupported("本轮不处理 RGB 文件的附加 Color Mode Data"));
    }
    resources(input.section()?)?;
    let global_alpha = layer_and_mask(input.section()?, &mut pixels, limits)?;
    let has_composite = !input.remaining.is_empty();
    if has_composite {
        image_data(&mut input, width, height, channels)?;
    }
    Ok(Header {
        width,
        height,
        has_composite,
        global_alpha,
    })
}

fn add_pixels(
    total: &mut u64,
    width: u32,
    height: u32,
    limits: ParseLimits,
) -> Result<(), PsdError> {
    *total = total
        .checked_add(u64::from(width) * u64::from(height))
        .filter(|total| *total <= limits.max_total_pixels)
        .ok_or_else(|| limited("PSD 画布与图层总像素超过预算"))?;
    Ok(())
}

fn resources(mut resources: Cursor<'_>) -> Result<(), PsdError> {
    while !resources.remaining.is_empty() {
        resources.signature(b"8BIM")?;
        let id = resources.u16()?;
        resources.pascal_name(2)?;
        let mut data = resources.section()?;
        let length = data.remaining.len();
        match id {
            1005 if length == 16 => {
                let horizontal = data.u32()? as i32;
                data.take(4)?; // 水平分辨率单位与宽度显示单位。
                let vertical = data.u32()? as i32;
                if horizontal <= 0 || vertical <= 0 {
                    return Err(invalid(
                        "ResolutionInfo 的水平与垂直 16.16 fixed 分辨率必须为正",
                    ));
                }
            }
            1005 => return Err(invalid("ResolutionInfo 必须为 16 字节")),
            1039 => return Err(unsupported("候选严格读取尚不能处理 ICC 图像资源")),
            _ => return Err(unsupported("本轮仅允许 ResolutionInfo 图像资源")),
        }
        resources.padding(length % 2)?;
    }
    Ok(())
}

struct LayerLayout {
    width: u32,
    height: u32,
    channel_lengths: [u32; 4],
    channel_count: usize,
}

fn layer_and_mask(
    mut section: Cursor<'_>,
    pixels: &mut u64,
    limits: ParseLimits,
) -> Result<bool, PsdError> {
    if section.remaining.is_empty() {
        return Ok(false);
    }
    let mut info = section.section()?;
    let mut global_alpha = false;
    if !info.remaining.is_empty() {
        let signed_count = info.u16()? as i16;
        global_alpha = signed_count < 0;
        let count = u32::from(signed_count.unsigned_abs());
        if count > limits.max_layers {
            return Err(limited("PSD 图层记录数量超过预算"));
        }
        // 每条记录先完整检查，再存储固定大小的通道布局，避免直接预分配声明长度。
        let mut layers = Vec::new();
        let mut group_depth = 0;
        for _ in 0..count {
            layers.push(layer_record(&mut info, pixels, limits, &mut group_depth)?);
        }
        if group_depth != 0 {
            return Err(invalid("PSD 组边界没有配对"));
        }
        for layer in layers {
            for length in &layer.channel_lengths[..layer.channel_count] {
                let mut channel = Cursor::new(info.take(*length as usize)?);
                image_data(&mut channel, layer.width, layer.height, 1)?;
            }
        }
        if info.remaining.len() > 1 {
            return Err(invalid("PSD 图层信息区包含未声明的数据"));
        }
        info.padding(info.remaining.len())?;
    }
    if !section.section()?.remaining.is_empty() {
        return Err(unsupported("本轮不支持全局图层蒙版"));
    }
    if !section.remaining.is_empty() {
        return Err(unsupported("本轮不支持文档级附加图层信息"));
    }
    Ok(global_alpha)
}

fn layer_record(
    input: &mut Cursor<'_>,
    pixels: &mut u64,
    limits: ParseLimits,
    group_depth: &mut u32,
) -> Result<LayerLayout, PsdError> {
    let top = i64::from(input.u32()? as i32);
    let left = i64::from(input.u32()? as i32);
    let bottom = i64::from(input.u32()? as i32);
    let right = i64::from(input.u32()? as i32);
    let width = u32::try_from(right - left).map_err(|_| invalid("PSD 图层横向边界无效"))?;
    let height = u32::try_from(bottom - top).map_err(|_| invalid("PSD 图层纵向边界无效"))?;
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(unsupported("本轮图层边长上限为 30000 像素"));
    }
    add_pixels(pixels, width, height, limits)?;
    let channel_count = usize::from(input.u16()?);
    if channel_count > 4 {
        return Err(unsupported("本轮每个图层最多支持 RGBA 四个通道"));
    }
    let mut channel_lengths = [0; 4];
    let mut seen = 0_u8;
    for length in &mut channel_lengths[..channel_count] {
        let id = input.u16()? as i16;
        if !(-1..=2).contains(&id) {
            return Err(unsupported("本轮不支持图层蒙版或其他附加通道"));
        }
        let bit = 1_u8 << (id + 1);
        if seen & bit != 0 {
            return Err(invalid("PSD 图层通道 ID 重复"));
        }
        seen |= bit;
        *length = input.u32()?;
    }
    input.signature(b"8BIM")?;
    blend_mode(input)?;
    input.u8()?; // 不透明度保留为来源值，由适配器判断导出能力。
    if input.u8()? > 1 {
        return Err(invalid("PSD clipping 标记只能为 0 或 1"));
    }
    // 仅接纳透明度锁定、隐藏和 obsolete 低三位；未验证的像素无关/效果标志不得冒充位图。
    if input.u8()? & 0xf8 != 0 {
        return Err(unsupported("本轮尚未验证该图层 flags 声明的像素或效果语义"));
    }
    input.padding(1)?;
    let mut extra = input.section()?;
    if !extra.section()?.remaining.is_empty() {
        return Err(unsupported("本轮不支持图层蒙版"));
    }
    let ranges = extra.section()?;
    // 灰度复合与至多 RGBA 四通道各占 8 字节；不允许重复默认范围放大候选的 Vec 分配。
    if ranges.remaining.len() > 40
        || ranges.remaining.len() % 8 != 0
        || ranges
            .remaining
            .chunks(4)
            .any(|range| range != [0, 0, 255, 255])
    {
        return Err(unsupported("本轮只支持空或默认的图层混合范围"));
    }
    extra.pascal_name(4)?;
    let structural = additional_info(extra, group_depth)?;
    if width > 0 && height > 0 && !structural && seen & 0b1110 != 0b1110 {
        return Err(unsupported(
            "正面积位图必须包含完整的 RGB 通道，不能用默认颜色补齐",
        ));
    }
    Ok(LayerLayout {
        width,
        height,
        channel_lengths,
        channel_count,
    })
}

fn blend_mode(input: &mut Cursor<'_>) -> Result<(), PsdError> {
    if !matches!(input.take(4)?, b"norm" | b"pass") {
        return Err(unsupported("本轮只接纳 normal 和 pass-through 混合模式"));
    }
    Ok(())
}

fn additional_info(mut input: Cursor<'_>, group_depth: &mut u32) -> Result<bool, PsdError> {
    let mut seen_group = false;
    let mut structural = false;
    while !input.remaining.is_empty() {
        input.signature(b"8BIM")?;
        let key = input.take(4)?;
        let mut data = input.section()?;
        let length = data.remaining.len();
        match key {
            b"lyid" if length == 4 => {}
            b"lyid" => return Err(invalid("lyid 长度必须为 4 字节")),
            b"luni" => {
                let count = data.u32()? as usize;
                let bytes = count
                    .checked_mul(2)
                    .ok_or_else(|| invalid("luni 长度溢出"))?;
                data.take(bytes)?;
                if data.remaining.len() > 3 {
                    return Err(invalid("luni 包含未声明的数据"));
                }
                data.padding(data.remaining.len())?;
            }
            b"lsct" | b"lsdk" => {
                if seen_group || ![4, 12, 16].contains(&length) {
                    return Err(invalid("组分隔标签重复或长度无效"));
                }
                seen_group = true;
                let kind = data.u32()?;
                structural = (1..=3).contains(&kind);
                if length >= 12 {
                    data.signature(b"8BIM")?;
                    blend_mode(&mut data)?;
                }
                if length == 16 && data.u32()? > 1 {
                    return Err(unsupported("本轮不支持该组分隔子类型"));
                }
                // 文件记录从底到顶：bounding divider 入组，open/closed folder 结束该组。
                match kind {
                    0 => {}
                    3 if *group_depth < MAX_GROUP_DEPTH => *group_depth += 1,
                    3 => return Err(limited("PSD 组嵌套超过 64 层")),
                    1 | 2 if *group_depth > 0 => *group_depth -= 1,
                    1 | 2 => return Err(invalid("PSD 组结束记录没有对应边界")),
                    _ => return Err(invalid("PSD 组分隔类型无效")),
                }
            }
            _ => return Err(unsupported("本轮仅允许 lyid、luni、lsct 和 lsdk 图层标签")),
        }
        input.padding(length % 2)?;
    }
    Ok(structural)
}

fn image_data(
    input: &mut Cursor<'_>,
    width: u32,
    height: u32,
    channels: u16,
) -> Result<(), PsdError> {
    match input.u16()? {
        0 => {
            let size = u64::from(width) * u64::from(height) * u64::from(channels);
            if input.remaining.len() as u64 != size {
                return Err(invalid("PSD RAW 通道长度与像素尺寸不匹配"));
            }
            input.take(input.remaining.len())?;
        }
        1 => {
            let rows = u64::from(height) * u64::from(channels);
            let table_size = usize::try_from(rows * 2).map_err(|_| invalid("RLE 行长表溢出"))?;
            let mut table = Cursor::new(input.take(table_size)?);
            for _ in 0..rows {
                let size = usize::from(table.u16()?);
                packbits_row(input.take(size)?, width)?;
            }
            if !input.remaining.is_empty() {
                return Err(invalid("PSD RLE 通道包含未声明的数据"));
            }
        }
        _ => return Err(unsupported("本轮只验证 RAW 和 RLE 像素压缩")),
    }
    Ok(())
}

fn packbits_row(bytes: &[u8], width: u32) -> Result<(), PsdError> {
    let mut row = Cursor::new(bytes);
    let mut decoded = 0_u32;
    while !row.remaining.is_empty() {
        let marker = row.u8()? as i8;
        let count = match marker {
            0..=127 => {
                let count = marker as u32 + 1;
                row.take(count as usize)?;
                count
            }
            -127..=-1 => {
                row.take(1)?;
                (1 - i32::from(marker)) as u32
            }
            -128 => 0, // PackBits 标准允许无操作字节。
        };
        decoded = decoded
            .checked_add(count)
            .filter(|decoded| *decoded <= width)
            .ok_or_else(|| invalid("PSD RLE 行解压后超过声明宽度"))?;
    }
    if decoded != width {
        return Err(invalid("PSD RLE 行解压后未达到声明宽度"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &[u8] = include_bytes!("../../tests/fixtures/psd/bitmap-raw.psd");
    const RLE: &[u8] = include_bytes!("../../tests/fixtures/psd/bitmap-rle.psd");

    #[test]
    fn accepts_verified_raw_rle_and_missing_composite() {
        for bytes in [RAW, RLE] {
            let header = inspect(bytes, ParseLimits::default()).unwrap();
            assert_eq!((header.width, header.height), (4, 3));
            assert!(header.has_composite);
            assert!(!header.global_alpha);
        }
        let missing = include_bytes!("../../tests/fixtures/psd/bitmap-no-composite.psd");
        assert!(
            !inspect(missing, ParseLimits::default())
                .unwrap()
                .has_composite
        );
    }

    #[test]
    fn budgets_include_layers_and_are_checked_before_allocations() {
        for limits in [
            ParseLimits {
                max_file_bytes: 100,
                ..ParseLimits::default()
            },
            ParseLimits {
                max_total_pixels: 18,
                ..ParseLimits::default()
            },
            ParseLimits {
                max_layers: 1,
                ..ParseLimits::default()
            },
        ] {
            assert_eq!(
                inspect(RAW, limits).unwrap_err().code,
                PsdErrorCode::ResourceLimit
            );
        }
        inspect(
            RAW,
            ParseLimits {
                max_total_pixels: 19,
                ..ParseLimits::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn rejects_truncation_lengths_and_unverified_depth() {
        for size in [0, 25, 65, RAW.len() - 1] {
            assert_eq!(
                inspect(&RAW[..size], ParseLimits::default())
                    .unwrap_err()
                    .code,
                PsdErrorCode::InvalidInput
            );
        }
        let mut huge_section = RAW.to_vec();
        huge_section[30..34].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(
            inspect(&huge_section, ParseLimits::default())
                .unwrap_err()
                .code,
            PsdErrorCode::InvalidInput
        );
        let rgb16 = include_bytes!("../../tests/fixtures/psd/rgb16.psd");
        assert_eq!(
            inspect(rgb16, ParseLimits::default()).unwrap_err().code,
            PsdErrorCode::Unsupported
        );
    }

    #[test]
    fn validates_packbits_runs_including_repetition_and_noop() {
        packbits_row(&[128, 254, 9, 0, 4], 4).unwrap();
        for bytes in [&[3, 1][..], &[252, 1], &[0, 7]] {
            assert_eq!(
                packbits_row(bytes, 4).unwrap_err().code,
                PsdErrorCode::InvalidInput
            );
        }
    }

    #[test]
    fn rejects_unbounded_unicode_and_unknown_tags() {
        let mut depth = 0;
        let unicode = b"8BIMluni\0\0\0\x04\xff\xff\xff\xff";
        assert_eq!(
            additional_info(Cursor::new(unicode), &mut depth)
                .unwrap_err()
                .code,
            PsdErrorCode::InvalidInput
        );
        let descriptor = b"8BIMTySh\0\0\0\0";
        assert_eq!(
            additional_info(Cursor::new(descriptor), &mut depth)
                .unwrap_err()
                .code,
            PsdErrorCode::Unsupported
        );
    }

    #[test]
    fn group_nesting_is_paired_and_limited() {
        let divider = b"8BIMlsct\0\0\0\x04\0\0\0\x03";
        let folder = b"8BIMlsct\0\0\0\x04\0\0\0\x01";
        let mut depth = 0;
        for _ in 0..MAX_GROUP_DEPTH {
            additional_info(Cursor::new(divider), &mut depth).unwrap();
        }
        assert_eq!(
            additional_info(Cursor::new(divider), &mut depth)
                .unwrap_err()
                .code,
            PsdErrorCode::ResourceLimit
        );
        for _ in 0..MAX_GROUP_DEPTH {
            additional_info(Cursor::new(folder), &mut depth).unwrap();
        }
        assert_eq!(depth, 0);
        assert_eq!(
            additional_info(Cursor::new(folder), &mut depth)
                .unwrap_err()
                .code,
            PsdErrorCode::InvalidInput
        );
    }

    #[test]
    fn rejects_icc_invalid_clipping_and_unverified_blend_keys() {
        // 基础样本的 Resource ID 为字节 38..40；首层 blend/clipping 为 118..122 / 123。
        let mut icc = RAW.to_vec();
        icc[38..40].copy_from_slice(&1039_u16.to_be_bytes());
        assert_eq!(
            inspect(&icc, ParseLimits::default()).unwrap_err().code,
            PsdErrorCode::Unsupported
        );
        let mut clipping = RAW.to_vec();
        clipping[123] = 2;
        assert_eq!(
            inspect(&clipping, ParseLimits::default()).unwrap_err().code,
            PsdErrorCode::InvalidInput
        );
        for key in [b"xxxx", b"mul "] {
            let mut blend = RAW.to_vec();
            blend[118..122].copy_from_slice(key);
            assert_eq!(
                inspect(&blend, ParseLimits::default()).unwrap_err().code,
                PsdErrorCode::Unsupported
            );
            let mut divider = b"8BIMlsct\0\0\0\x0c\0\0\0\x038BIMnorm".to_vec();
            divider[20..24].copy_from_slice(key);
            assert_eq!(
                additional_info(Cursor::new(&divider), &mut 0)
                    .unwrap_err()
                    .code,
                PsdErrorCode::Unsupported
            );
        }
    }

    #[test]
    fn requires_rgb_pixels_but_allows_empty_or_structural_records() {
        let check = |bytes: &[u8], group_depth: &mut u32| {
            layer_record(
                &mut Cursor::new(bytes),
                &mut 0,
                ParseLimits::default(),
                group_depth,
            )
            .map(|_| ())
        };
        // 首层 record 位于 72..154，前 18 字节是矩形和通道数量；随后每通道占 6 字节。
        let mut alpha_only = RAW[72..154].to_vec();
        alpha_only[16..18].copy_from_slice(&1_u16.to_be_bytes());
        alpha_only.drain(24..42);
        assert_eq!(
            check(&alpha_only, &mut 0).unwrap_err().code,
            PsdErrorCode::Unsupported
        );
        alpha_only[8..12].copy_from_slice(&1_i32.to_be_bytes()); // bottom == top，合法零面积。
        check(&alpha_only, &mut 0).unwrap();

        let mut group = RAW[72..154].to_vec();
        group[16..18].copy_from_slice(&0_u16.to_be_bytes());
        group.drain(18..42);
        assert_eq!(
            check(&group, &mut 0).unwrap_err().code,
            PsdErrorCode::Unsupported
        );
        group[30..34].copy_from_slice(&40_u32.to_be_bytes()); // 原 extra 24 字节加 16 字节 lsct。
        group.extend_from_slice(b"8BIMlsct\0\0\0\x04\0\0\0\x03");
        let mut depth = 0;
        check(&group, &mut depth).unwrap();
        assert_eq!(depth, 1);
        *group.last_mut().unwrap() = 1;
        check(&group, &mut depth).unwrap();
        assert_eq!(depth, 0);
    }

    #[test]
    fn rejects_nonpositive_resolution_and_unverified_pixel_flags() {
        // Resource 1005 的水平/垂直 fixed 分别位于 46 / 54；第一层 flags 位于 124。
        for offset in [46, 54] {
            for resolution in [0_i32, -65536] {
                let mut bytes = RAW.to_vec();
                bytes[offset..offset + 4].copy_from_slice(&resolution.to_be_bytes());
                assert_eq!(
                    inspect(&bytes, ParseLimits::default()).unwrap_err().code,
                    PsdErrorCode::InvalidInput
                );
            }
        }
        for flags in [0x08, 0x10, 0x20, 0x40, 0x80] {
            let mut bytes = RAW.to_vec();
            bytes[124] = flags;
            assert_eq!(
                inspect(&bytes, ParseLimits::default()).unwrap_err().code,
                PsdErrorCode::Unsupported
            );
        }
    }
}

//! 校验 PSD v1 容器、通道布局与预算，借用输入切片，不复制资源或嵌入对象。
//! 资源和附加块仅盘点来源；descriptor 语义由解析适配器负责，不能据标签存在宣称完整支持。

use super::{
    error::{PsdError, PsdErrorCode},
    model::{Bounds, ParseLimits},
};

const MAX_DIMENSION: u32 = 30_000;
const MAX_GROUP_DEPTH: u32 = 64;
const MAX_METADATA_RECORDS: usize = 65_536;
// PSD 最多 56 个通道，混合范围还包含一组 composite gray。
const MAX_BLENDING_RANGES: usize = 57;

#[derive(Debug)]
pub(super) struct Header {
    pub width: u32,
    pub height: u32,
    pub has_composite: bool,
    pub global_alpha: bool,
    pub resources: Vec<ResourceRecord>,
    pub layers: Vec<LayerRecord>,
    pub global_tags: Vec<[u8; 4]>,
    pub global_mask_offset: Option<u64>,
    pub total_pixels: u64,
}

#[derive(Debug)]
pub(super) struct ResourceRecord {
    pub id: u16,
    /// 有效载荷相对固定源数据起点的字节偏移。
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug)]
pub(super) struct LayerRecord {
    /// PSD 原始记录顺序，从零计数；不将名称或可重复的 lyid 当作身份。
    pub ordinal: u32,
    pub source_id: Option<u32>,
    pub bounds: Bounds,
    pub tags: Vec<[u8; 4]>,
    pub blend_mode: [u8; 4],
    pub clipping: bool,
    pub flags: u8,
    pub has_rgb: bool,
    pub has_mask: bool,
    pub has_nondefault_blending_ranges: bool,
    pub section_kind: Option<u32>,
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
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            remaining: bytes,
            offset: 0,
        }
    }

    fn take(&mut self, size: usize) -> Result<&'a [u8], PsdError> {
        let bytes = self
            .remaining
            .get(..size)
            .ok_or_else(|| invalid("PSD 区段或通道数据截断"))?;
        self.remaining = &self.remaining[size..];
        self.offset += size;
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
        self.subsection(size)
    }

    fn subsection(&mut self, size: usize) -> Result<Cursor<'a>, PsdError> {
        let offset = self.offset;
        Ok(Cursor {
            remaining: self.take(size)?,
            offset,
        })
    }

    fn key(&mut self) -> Result<[u8; 4], PsdError> {
        let bytes = self.take(4)?;
        Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
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
    let mut metadata_count = 0;
    let resources = resources(input.section()?, &mut metadata_count)?;
    let layer_info = layer_and_mask(input.section()?, &mut pixels, limits, &mut metadata_count)?;
    let has_composite = !input.remaining.is_empty();
    if has_composite {
        image_data(&mut input, width, height, channels)?;
    }
    Ok(Header {
        width,
        height,
        has_composite,
        global_alpha: layer_info.global_alpha,
        resources,
        layers: layer_info.layers,
        global_tags: layer_info.global_tags,
        global_mask_offset: layer_info.global_mask_offset,
        total_pixels: pixels,
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
        .ok_or_else(|| limited("PSD 画布、图层与蒙版总像素超过预算"))?;
    Ok(())
}

fn count_metadata(count: &mut usize) -> Result<(), PsdError> {
    if *count >= MAX_METADATA_RECORDS {
        return Err(limited("PSD 资源与附加块合计超过 65536 条"));
    }
    *count += 1;
    Ok(())
}

fn resources(
    mut resources: Cursor<'_>,
    count: &mut usize,
) -> Result<Vec<ResourceRecord>, PsdError> {
    let mut records = Vec::new();
    while !resources.remaining.is_empty() {
        count_metadata(count)?;
        resources.signature(b"8BIM")?;
        let id = resources.u16()?;
        resources.pascal_name(2)?;
        let mut data = resources.section()?;
        let length = data.remaining.len();
        records.push(ResourceRecord {
            id,
            offset: data.offset as u64,
            length: length as u64,
        });
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
            // 包括 ICC 在内的其他资源只验证容器边界；读取能力和颜色限制另行报告。
            _ => {}
        }
        resources.padding(length % 2)?;
    }
    Ok(records)
}

#[derive(Debug, Clone, Copy, Default)]
struct ChannelLayout {
    id: i16,
    length: u32,
}

#[derive(Debug)]
struct LayerLayout {
    record: LayerRecord,
    mask_bounds: Option<Bounds>,
    real_mask_bounds: Option<Bounds>,
    channels: [ChannelLayout; 6],
    channel_count: usize,
}

#[derive(Debug, Default)]
struct LayerSection {
    global_alpha: bool,
    layers: Vec<LayerRecord>,
    global_tags: Vec<[u8; 4]>,
    global_mask_offset: Option<u64>,
}

fn layer_and_mask(
    mut section: Cursor<'_>,
    pixels: &mut u64,
    limits: ParseLimits,
    metadata_count: &mut usize,
) -> Result<LayerSection, PsdError> {
    let mut result = LayerSection::default();
    if section.remaining.is_empty() {
        return Ok(result);
    }
    let mut info = section.section()?;
    if !info.remaining.is_empty() {
        let signed_count = info.u16()? as i16;
        result.global_alpha = signed_count < 0;
        let count = u32::from(signed_count.unsigned_abs());
        if count > limits.max_layers {
            return Err(limited("PSD 图层记录数量超过预算"));
        }
        // 每条记录先完整检查，再存储固定大小的通道布局，避免直接预分配声明长度。
        let mut layers = Vec::new();
        let mut group_depth = 0;
        for ordinal in 0..count {
            layers.push(layer_record(
                &mut info,
                pixels,
                limits,
                &mut group_depth,
                ordinal,
                metadata_count,
            )?);
        }
        if group_depth != 0 {
            return Err(invalid("PSD 组边界没有配对"));
        }
        for layer in layers {
            layer_channels(&mut info, &layer)?;
            result.layers.push(layer.record);
        }
        if info.remaining.len() > 1 {
            return Err(invalid("PSD 图层信息区包含未声明的数据"));
        }
        info.padding(info.remaining.len())?;
    }
    let global_mask = section.section()?;
    if !global_mask.remaining.is_empty() {
        if global_mask.remaining.len() < 13 {
            return Err(invalid("PSD 全局蒙版数据截断"));
        }
        result.global_mask_offset = Some(global_mask.offset as u64);
    }
    if !section.remaining.is_empty() {
        result.global_tags = global_additional_info(section, metadata_count)?;
    }
    Ok(result)
}

fn layer_channels(info: &mut Cursor<'_>, layer: &LayerLayout) -> Result<(), PsdError> {
    for layout in &layer.channels[..layer.channel_count] {
        let bounds = match layout.id {
            -2 => layer
                .mask_bounds
                .ok_or_else(|| invalid("PSD 蒙版通道没有对应蒙版矩形"))?,
            -3 => layer
                .real_mask_bounds
                .ok_or_else(|| invalid("PSD 实际蒙版通道没有对应矩形"))?,
            _ => layer.record.bounds,
        };
        let mut channel = info.subsection(layout.length as usize)?;
        image_data(&mut channel, bounds.width, bounds.height, 1)?;
    }
    Ok(())
}

fn layer_record(
    input: &mut Cursor<'_>,
    pixels: &mut u64,
    limits: ParseLimits,
    group_depth: &mut u32,
    ordinal: u32,
    metadata_count: &mut usize,
) -> Result<LayerLayout, PsdError> {
    let bounds = rectangle(input)?;
    let Bounds { width, height, .. } = bounds;
    add_pixels(pixels, width, height, limits)?;
    let channel_count = usize::from(input.u16()?);
    if channel_count > 6 {
        return Err(unsupported("本轮支持 RGBA 与用户/实际蒙版六个图层通道"));
    }
    let mut channels = [ChannelLayout::default(); 6];
    let mut seen = 0_u8;
    for channel in &mut channels[..channel_count] {
        let id = input.u16()? as i16;
        if !(-3..=2).contains(&id) {
            return Err(unsupported("本轮不支持该图层附加通道"));
        }
        let bit = 1_u8 << (id + 3);
        if seen & bit != 0 {
            return Err(invalid("PSD 图层通道 ID 重复"));
        }
        seen |= bit;
        *channel = ChannelLayout {
            id,
            length: input.u32()?,
        };
    }
    input.signature(b"8BIM")?;
    let blend_mode = blend_mode(input)?;
    input.u8()?; // 不透明度保留为来源值，由适配器判断导出能力。
    let clipping = input.u8()?;
    if clipping > 1 {
        return Err(invalid("PSD clipping 标记只能为 0 或 1"));
    }
    let flags = input.u8()?;
    input.padding(1)?;
    let mut extra = input.section()?;
    let (mask_bounds, real_mask_bounds) = mask_data(extra.section()?, pixels, limits)?;
    let ranges = extra.section()?;
    // 每个范围包含源、目标各四字节；非默认范围阻止普通素材导出，由适配器报告。
    if ranges.remaining.len() % 8 != 0 || ranges.remaining.len() > 8 * MAX_BLENDING_RANGES {
        return Err(invalid("PSD 图层混合范围数量或长度无效"));
    }
    let has_nondefault_blending_ranges = ranges
        .remaining
        .chunks(4)
        .any(|range| range != [0, 0, 255, 255]);
    extra.pascal_name(4)?;
    let tags = additional_info(extra, group_depth, metadata_count)?;
    let structural = tags
        .section_kind
        .is_some_and(|kind| (1..=3).contains(&kind));
    // 具有其他内容标签的空通道记录保留元数据；普通位图仍不能以缺省 RGB 伪装有效数据。
    let has_content_metadata = tags
        .keys
        .iter()
        .any(|key| !matches!(key, b"lyid" | b"luni" | b"lsct" | b"lsdk"));
    if width > 0
        && height > 0
        && !structural
        && !has_content_metadata
        && seen & 0b111000 != 0b111000
    {
        return Err(unsupported(
            "正面积位图必须包含完整的 RGB 通道，不能用默认颜色补齐",
        ));
    }
    Ok(LayerLayout {
        record: LayerRecord {
            ordinal,
            source_id: tags.source_id,
            bounds,
            tags: tags.keys,
            blend_mode,
            clipping: clipping == 1,
            flags,
            has_rgb: seen & 0b111000 == 0b111000,
            has_mask: mask_bounds.is_some(),
            has_nondefault_blending_ranges,
            section_kind: tags.section_kind,
        },
        mask_bounds,
        real_mask_bounds,
        channels,
        channel_count,
    })
}

fn rectangle(input: &mut Cursor<'_>) -> Result<Bounds, PsdError> {
    let top = i64::from(input.u32()? as i32);
    let left = i64::from(input.u32()? as i32);
    let bottom = i64::from(input.u32()? as i32);
    let right = i64::from(input.u32()? as i32);
    let width = u32::try_from(right - left).map_err(|_| invalid("PSD 图层横向边界无效"))?;
    let height = u32::try_from(bottom - top).map_err(|_| invalid("PSD 图层纵向边界无效"))?;
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(unsupported("本轮图层边长上限为 30000 像素"));
    }
    Ok(Bounds {
        x: left as i32,
        y: top as i32,
        width,
        height,
    })
}

fn mask_data(
    mut input: Cursor<'_>,
    pixels: &mut u64,
    limits: ParseLimits,
) -> Result<(Option<Bounds>, Option<Bounds>), PsdError> {
    if input.remaining.is_empty() {
        return Ok((None, None));
    }
    let bounds = rectangle(&mut input)?;
    add_pixels(pixels, bounds.width, bounds.height, limits)?;
    input.u8()?; // 缺省蒙版颜色。
    let flags = input.u8()?;
    // Adobe Layer mask / adjustment layer data：参数紧跟 flags，之后才是实际蒙版矩形。
    if flags & 0x10 != 0 {
        let parameters = input.u8()?;
        if parameters & 0xf0 != 0 {
            return Err(unsupported("PSD 蒙版包含未知参数标记"));
        }
        for (flag, length) in [(1, 1), (2, 8), (4, 1), (8, 8)] {
            if parameters & flag != 0 {
                input.take(length)?;
            }
        }
    }
    let real = match input.remaining.len() {
        0 | 2 => {
            input.padding(input.remaining.len())?;
            None
        }
        18 => {
            input.take(2)?; // 实际蒙版 flags 和缺省颜色。
            let bounds = rectangle(&mut input)?;
            add_pixels(pixels, bounds.width, bounds.height, limits)?;
            Some(bounds)
        }
        _ => return Err(invalid("PSD 蒙版剩余长度与实际蒙版矩形不匹配")),
    };
    Ok((Some(bounds), real))
}

fn blend_mode(input: &mut Cursor<'_>) -> Result<[u8; 4], PsdError> {
    let key = input.key()?;
    if !matches!(
        &key,
        b"norm"
            | b"pass"
            | b"diss"
            | b"dark"
            | b"mul "
            | b"idiv"
            | b"lbrn"
            | b"dkCl"
            | b"lite"
            | b"scrn"
            | b"div "
            | b"lddg"
            | b"lgCl"
            | b"over"
            | b"sLit"
            | b"hLit"
            | b"vLit"
            | b"lLit"
            | b"pLit"
            | b"hMix"
            | b"diff"
            | b"smud"
            | b"fsub"
            | b"fdiv"
            | b"hue "
            | b"sat "
            | b"colr"
            | b"lum "
    ) {
        return Err(unsupported("PSD 使用未识别的混合模式"));
    }
    Ok(key)
}

#[derive(Debug, Default)]
struct AdditionalInfo {
    keys: Vec<[u8; 4]>,
    source_id: Option<u32>,
    section_kind: Option<u32>,
}

fn additional_block<'a>(
    input: &mut Cursor<'a>,
    count: &mut usize,
    alignment: usize,
) -> Result<([u8; 4], Cursor<'a>), PsdError> {
    count_metadata(count)?;
    let signature = input.key()?;
    let key = input.key()?;
    let mut data = match &signature {
        b"8BIM" => input.section()?,
        b"8B64" => {
            let high = input.u32()?;
            let low = input.u32()?;
            let length = (u64::from(high) << 32) | u64::from(low);
            let length = usize::try_from(length).map_err(|_| invalid("PSD 附加块长度溢出"))?;
            input.subsection(length)?
        }
        _ => return Err(invalid("PSD 附加块签名无效")),
    };
    // data 借用 payload，不为 lnk2 等大块复制内存。
    let length = data.remaining.len();
    input.padding((alignment - length % alignment) % alignment)?;
    // 候选库对 luni 的 UTF-16 长度分配前必须先验证其容器；不读取名字内容。
    if &key == b"luni" {
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
    Ok((key, data))
}

fn global_additional_info(
    mut input: Cursor<'_>,
    count: &mut usize,
) -> Result<Vec<[u8; 4]>, PsdError> {
    let mut tags = Vec::new();
    while !input.remaining.is_empty() {
        // Layer and Mask 区末尾允许至多三个零字节作四字节对齐。
        if input.remaining.len() <= 3 {
            input.padding(input.remaining.len())?;
            break;
        }
        // 文档级附加块按四字节补齐，图层 extra data 内的块按两字节补齐。
        // 不扫描或猜测下一个签名，避免把损坏长度当成可恢复的填充。
        let (key, _) = additional_block(&mut input, count, 4)?;
        // RGB8 的权威图层来自普通 Layer Info；不允许附加块递归替换已核验的布局。
        if matches!(&key, b"Layr" | b"Lr16" | b"Lr32") {
            return Err(unsupported("RGB8 附加图层信息不允许替换主图层记录"));
        }
        tags.push(key);
    }
    Ok(tags)
}

fn additional_info(
    mut input: Cursor<'_>,
    group_depth: &mut u32,
    count: &mut usize,
) -> Result<AdditionalInfo, PsdError> {
    let mut result = AdditionalInfo::default();
    while !input.remaining.is_empty() {
        let (key, mut data) = additional_block(&mut input, count, 2)?;
        let length = data.remaining.len();
        match &key {
            b"lyid" if length == 4 && result.source_id.is_none() => {
                result.source_id = Some(data.u32()?);
            }
            b"lyid" => return Err(invalid("lyid 重复或长度不是 4 字节")),
            b"lsct" | b"lsdk" => {
                if result.section_kind.is_some() || ![4, 12, 16].contains(&length) {
                    return Err(invalid("组分隔标签重复或长度无效"));
                }
                let kind = data.u32()?;
                result.section_kind = Some(kind);
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
            _ => {}
        }
        result.keys.push(key);
    }
    Ok(result)
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
    fn rejects_unbounded_unicode_and_inventories_unknown_tags() {
        let mut depth = 0;
        let unicode = b"8BIMluni\0\0\0\x04\xff\xff\xff\xff";
        assert_eq!(
            additional_info(Cursor::new(unicode), &mut depth, &mut 0)
                .unwrap_err()
                .code,
            PsdErrorCode::InvalidInput
        );
        let descriptor = b"8BIMTySh\0\0\0\0";
        let result = additional_info(Cursor::new(descriptor), &mut depth, &mut 0).unwrap();
        assert_eq!(result.keys, vec![*b"TySh"]);
        for length in 1..descriptor.len() {
            assert_eq!(
                additional_info(Cursor::new(&descriptor[..length]), &mut 0, &mut 0)
                    .unwrap_err()
                    .code,
                PsdErrorCode::InvalidInput
            );
        }
    }

    #[test]
    fn group_nesting_is_paired_and_limited() {
        let divider = b"8BIMlsct\0\0\0\x04\0\0\0\x03";
        let folder = b"8BIMlsct\0\0\0\x04\0\0\0\x01";
        let mut depth = 0;
        for _ in 0..MAX_GROUP_DEPTH {
            additional_info(Cursor::new(divider), &mut depth, &mut 0).unwrap();
        }
        assert_eq!(
            additional_info(Cursor::new(divider), &mut depth, &mut 0)
                .unwrap_err()
                .code,
            PsdErrorCode::ResourceLimit
        );
        for _ in 0..MAX_GROUP_DEPTH {
            additional_info(Cursor::new(folder), &mut depth, &mut 0).unwrap();
        }
        assert_eq!(depth, 0);
        assert_eq!(
            additional_info(Cursor::new(folder), &mut depth, &mut 0)
                .unwrap_err()
                .code,
            PsdErrorCode::InvalidInput
        );
    }

    #[test]
    fn inventories_icc_and_known_blends_but_rejects_invalid_clipping_and_unknown_blends() {
        // 基础样本的 Resource ID 为字节 38..40；首层 blend/clipping 为 118..122 / 123。
        let mut icc = RAW.to_vec();
        icc[38..40].copy_from_slice(&1039_u16.to_be_bytes());
        let header = inspect(&icc, ParseLimits::default()).unwrap();
        assert_eq!(header.resources[0].id, 1039);
        assert_eq!(header.resources[0].offset, 46);
        assert_eq!(header.resources[0].length, 16);
        let mut clipping = RAW.to_vec();
        clipping[123] = 2;
        assert_eq!(
            inspect(&clipping, ParseLimits::default()).unwrap_err().code,
            PsdErrorCode::InvalidInput
        );
        let mut blend = RAW.to_vec();
        blend[118..122].copy_from_slice(b"xxxx");
        assert_eq!(
            inspect(&blend, ParseLimits::default()).unwrap_err().code,
            PsdErrorCode::Unsupported
        );
        let mut divider = b"8BIMlsct\0\0\0\x0c\0\0\0\x038BIMnorm".to_vec();
        divider[20..24].copy_from_slice(b"xxxx");
        assert_eq!(
            additional_info(Cursor::new(&divider), &mut 0, &mut 0)
                .unwrap_err()
                .code,
            PsdErrorCode::Unsupported
        );
        for key in [b"mul ", b"diff", b"scrn", b"fdiv"] {
            let mut blend = RAW.to_vec();
            blend[118..122].copy_from_slice(key);
            let header = inspect(&blend, ParseLimits::default()).unwrap();
            assert_eq!(&header.layers[0].blend_mode, key);
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
                0,
                &mut 0,
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
    fn rejects_nonpositive_resolution_and_preserves_pixel_flags() {
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
            let header = inspect(&bytes, ParseLimits::default()).unwrap();
            assert_eq!(header.layers[0].flags, flags);
        }
    }

    #[test]
    fn global_blocks_are_bounded_and_cannot_replace_rgb8_layers() {
        let bytes = b"8B64lnk2\0\0\0\0\0\0\0\x03abc\0\0\0";
        assert_eq!(
            global_additional_info(Cursor::new(bytes), &mut 0).unwrap(),
            vec![*b"lnk2"]
        );
        for end in 1..19 {
            // 1..3 zero-only padding is accepted, but this prefix starts with 8B64.
            assert_eq!(
                global_additional_info(Cursor::new(&bytes[..end]), &mut 0)
                    .unwrap_err()
                    .code,
                PsdErrorCode::InvalidInput
            );
        }
        for key in [b"Layr", b"Lr16", b"Lr32"] {
            let mut bytes = b"8BIMxxxx\0\0\0\0".to_vec();
            bytes[4..8].copy_from_slice(key);
            assert_eq!(
                global_additional_info(Cursor::new(&bytes), &mut 0)
                    .unwrap_err()
                    .code,
                PsdErrorCode::Unsupported
            );
        }
        let mut count = MAX_METADATA_RECORDS;
        assert_eq!(
            global_additional_info(Cursor::new(bytes), &mut count)
                .unwrap_err()
                .code,
            PsdErrorCode::ResourceLimit
        );
    }

    fn section_bytes(bytes: &[u8]) -> Vec<u8> {
        let mut result = u32::try_from(bytes.len()).unwrap().to_be_bytes().to_vec();
        result.extend_from_slice(bytes);
        result
    }

    fn mask_with_parameters() -> Vec<u8> {
        let mut bytes = Vec::new();
        // 用户蒙版 2×1，参数包含两份 density/feather，实际蒙版 1×3。
        for coordinate in [0_i32, 0, 1, 2] {
            bytes.extend_from_slice(&coordinate.to_be_bytes());
        }
        bytes.extend_from_slice(&[255, 0x10, 0x0f, 192]);
        bytes.extend_from_slice(&1.25_f64.to_be_bytes());
        bytes.push(128);
        bytes.extend_from_slice(&2.5_f64.to_be_bytes());
        bytes.extend_from_slice(&[0, 0]);
        for coordinate in [0_i32, 0, 3, 1] {
            bytes.extend_from_slice(&coordinate.to_be_bytes());
        }
        bytes
    }

    fn masked_psd(mask: &[u8], mask_channel_lengths: [u32; 2]) -> Vec<u8> {
        let mut record = Vec::new();
        for coordinate in [0_i32, 0, 1, 1] {
            record.extend_from_slice(&coordinate.to_be_bytes());
        }
        record.extend_from_slice(&6_u16.to_be_bytes());
        let channels = [
            (-1_i16, 1_u32),
            (0, 1),
            (1, 1),
            (2, 1),
            (-2, mask_channel_lengths[0]),
            (-3, mask_channel_lengths[1]),
        ];
        for (id, pixels) in channels {
            record.extend_from_slice(&id.to_be_bytes());
            record.extend_from_slice(&(pixels + 2).to_be_bytes());
        }
        record.extend_from_slice(b"8BIMnorm\xff\0\0\0");
        let mut extra = section_bytes(mask);
        extra.extend_from_slice(&0_u32.to_be_bytes());
        extra.extend_from_slice(&[0; 4]);
        record.extend_from_slice(&section_bytes(&extra));
        let mut info = 1_i16.to_be_bytes().to_vec();
        info.extend_from_slice(&record);
        for (_, pixels) in channels {
            info.extend_from_slice(&0_u16.to_be_bytes());
            info.extend(std::iter::repeat_n(255, pixels as usize));
        }
        if !info.len().is_multiple_of(2) {
            info.push(0);
        }
        let mut layer_section = section_bytes(&info);
        layer_section.extend_from_slice(&0_u32.to_be_bytes());
        let mut bytes = RAW[..26].to_vec();
        bytes.extend_from_slice(&[0; 8]); // 空 Color Mode Data 与 Image Resources。
        bytes.extend_from_slice(&section_bytes(&layer_section));
        bytes
    }

    #[test]
    fn mask_channels_use_each_declared_rectangle_and_budget() {
        let mask = mask_with_parameters();
        let bytes = masked_psd(&mask, [2, 3]);
        let result = inspect(&bytes, ParseLimits::default()).unwrap();
        assert!(result.layers[0].has_mask);
        assert_eq!(result.total_pixels, 18); // canvas12 + layer1 + mask2 + real3。
        assert_eq!(
            inspect(
                &bytes,
                ParseLimits {
                    max_total_pixels: 17,
                    ..ParseLimits::default()
                }
            )
            .unwrap_err()
            .code,
            PsdErrorCode::ResourceLimit
        );
        for lengths in [[1, 3], [2, 1], [3, 2]] {
            assert_eq!(
                inspect(&masked_psd(&mask, lengths), ParseLimits::default())
                    .unwrap_err()
                    .code,
                PsdErrorCode::InvalidInput
            );
        }
        // 删除实际矩形后 -3 通道必须失败，不能借用用户蒙版或图层矩形。
        assert_eq!(
            inspect(
                &masked_psd(&mask[..mask.len() - 18], [2, 3]),
                ParseLimits::default()
            )
            .unwrap_err()
            .code,
            PsdErrorCode::InvalidInput
        );
        for end in [1, 17, 18, 19, 25, mask.len() - 1] {
            assert!(mask_data(Cursor::new(&mask[..end]), &mut 0, ParseLimits::default()).is_err());
        }
    }

    #[test]
    fn candidate_uses_the_same_mask_parameter_order_as_preflight() {
        let bytes = masked_psd(&mask_with_parameters(), [2, 3]);
        inspect(&bytes, ParseLimits::default()).unwrap();
        let parsed = ag_psd::read_psd(
            &bytes,
            &ag_psd::psd::ReadOptions {
                use_raw_data: Some(true),
                strict: Some(true),
                ..ag_psd::psd::ReadOptions::default()
            },
        )
        .unwrap();
        let layer = &parsed.children.as_ref().unwrap()[0];
        let mask = layer.additional_info.mask.as_ref().unwrap();
        let real = layer.additional_info.real_mask.as_ref().unwrap();
        assert_eq!(mask.right, Some(2.0));
        assert_eq!(mask.bottom, Some(1.0));
        assert_eq!(mask.user_mask_feather, Some(1.25));
        assert_eq!(mask.vector_mask_feather, Some(2.5));
        assert_eq!(real.right, Some(1.0));
        assert_eq!(real.bottom, Some(3.0));
    }
}

//! 有界 PNG 输出。流式编码避免先生成整幅压缩副本；编解码器内部临时内存仍非 RSS 硬限。

use std::io::{self, Write};

use super::{PsdError, PsdErrorCode};

struct Output {
    bytes: Vec<u8>,
    limit: usize,
    exhausted: bool,
}

impl Write for Output {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.exhausted || buffer.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.exhausted = true;
            return Err(io::Error::other("PNG 输出超过字节上限"));
        }
        let required = self.bytes.len() + buffer.len();
        if required > self.bytes.capacity() {
            let capacity = required.max(
                self.bytes
                    .capacity()
                    .saturating_mul(2)
                    .max(4096)
                    .min(self.limit),
            );
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(io::Error::other)?;
            // Vec 允许分配器提供更多容量；不可将超出预留的容量作为成功结果发布。
            if self.bytes.capacity() > self.limit {
                self.exhausted = true;
                return Err(io::Error::other("PNG 分配容量超过字节上限"));
            }
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn encode(
    pixels: ag_psd::PixelData,
    width: u32,
    height: u32,
    limit: usize,
) -> Result<Vec<u8>, PsdError> {
    let length = u64::from(width) * u64::from(height) * 4;
    if pixels.width != width || pixels.height != height || pixels.data.len() as u64 != length {
        return Err(PsdError::new(
            PsdErrorCode::ParseFailed,
            "解码像素尺寸与原始矩形不一致",
        ));
    }
    let mut output = Output {
        bytes: Vec::new(),
        limit,
        exhausted: false,
    };
    let result = (|| -> Result<(), png::EncodingError> {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        {
            let mut stream = writer.stream_writer()?;
            stream.write_all(&pixels.data)?;
            stream.finish()?;
        }
        writer.finish()
    })();
    result.map_err(|cause| {
        PsdError::with_cause(
            if output.exhausted {
                PsdErrorCode::ResourceLimit
            } else {
                PsdErrorCode::EncodeFailed
            },
            format!("编码 PNG 失败：{cause}"),
            cause,
        )
    })?;
    Ok(output.bytes)
}

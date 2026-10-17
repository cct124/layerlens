//! 有界真实截图快照：一次捕获、256×256 物理像素 PNG 分块、RGBA 指纹校验。
//! 每个驱动只保留一个快照；显式释放、替换、60 秒到期或驱动销毁都会回收。
use super::model::ScreenshotResult;
use crate::workspace::error;
use base64::Engine as _;
use layerlens_core::workspace_contract::{WorkspaceError, WorkspaceErrorCode as Code};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Cursor,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const TILE: u32 = 256;
const PIXEL_BYTES: usize = 64 * 1024 * 1024;
const TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CaptureRequest {
    pub frontend_id: String,
    pub operation: CaptureOperation,
}
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum CaptureOperation {
    Create {},
    Tile { capture_id: String, index: u32 },
    Release { capture_id: String },
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CaptureInfo {
    pub capture_id: String,
    pub width: u32,
    pub height: u32,
    pub tile_size: u32,
    pub tile_count: u32,
    pub rgba_sha256: String,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum CaptureResult {
    Created {
        capture: CaptureInfo,
        screenshot: Box<ScreenshotResult>,
    },
    Tile {
        capture_id: String,
        index: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    Released {
        capture_id: String,
    },
}
pub(crate) struct Pixels {
    pub width: u32,
    pub height: u32,
    rgba: Vec<u8>,
}
impl Pixels {
    pub(crate) fn decode(base64: &str) -> Result<Self, WorkspaceError> {
        if base64.len() > (8usize * 1024 * 1024).div_ceil(3) * 4 {
            return Err(error(Code::ResourceLimit, "源截图超过 8 MiB PNG 额度"));
        }
        let png = base64::engine::general_purpose::STANDARD
            .decode(base64)
            .map_err(|e| error(Code::InvalidInput, format!("截图 base64 无效：{e}")))?;
        let mut decoder = png::Decoder::new(Cursor::new(png));
        decoder.set_limits(png::Limits { bytes: PIXEL_BYTES });
        let mut reader = decoder
            .read_info()
            .map_err(|e| error(Code::InvalidInput, format!("截图 PNG 无效：{e}")))?;
        let info = reader.info();
        if info.width == 0
            || info.height == 0
            || info.width > 8192
            || info.height > 8192
            || u64::from(info.width) * u64::from(info.height) * 4 > PIXEL_BYTES as u64
            || info.bit_depth != png::BitDepth::Eight
            || !matches!(info.color_type, png::ColorType::Rgb | png::ColorType::Rgba)
        {
            return Err(error(
                Code::ResourceLimit,
                "截图须为 8 位 RGB(A)，RGBA 不超过 64 MiB",
            ));
        }
        let mut bytes = vec![0; reader.output_buffer_size()];
        let frame = reader
            .next_frame(&mut bytes)
            .map_err(|e| error(Code::InvalidInput, format!("截图解码失败：{e}")))?;
        bytes.truncate(frame.buffer_size());
        let rgba = if frame.color_type == png::ColorType::Rgba {
            bytes
        } else {
            bytes
                .chunks_exact(3)
                .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
                .collect()
        };
        Ok(Self {
            width: frame.width,
            height: frame.height,
            rgba,
        })
    }
    fn tile(&self, id: &str, index: u32) -> Result<(CaptureResult, String), WorkspaceError> {
        let columns = self.width.div_ceil(TILE);
        if index >= columns * self.height.div_ceil(TILE) {
            return Err(error(Code::InvalidInput, "截图分块索引越界"));
        }
        let x = index % columns * TILE;
        let y = index / columns * TILE;
        let width = TILE.min(self.width - x);
        let height = TILE.min(self.height - y);
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for row in y..y + height {
            let start = ((row * self.width + x) * 4) as usize;
            rgba.extend_from_slice(&self.rgba[start..start + (width * 4) as usize]);
        }
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(encoding_error)?;
            writer.write_image_data(&rgba).map_err(encoding_error)?;
            writer.finish().map_err(encoding_error)?;
        }
        if bytes.len() > crate::dev_mcp::tools::PNG_BYTES {
            return Err(error(Code::ResourceLimit, "PNG 分块超过既有图像额度"));
        }
        Ok((
            CaptureResult::Tile {
                capture_id: id.into(),
                index,
                x,
                y,
                width,
                height,
            },
            base64::engine::general_purpose::STANDARD.encode(bytes),
        ))
    }
}
fn encoding_error(e: png::EncodingError) -> WorkspaceError {
    error(Code::Internal, format!("编码截图分块失败：{e}"))
}
struct Capture {
    info: CaptureInfo,
    frontend: String,
    created: Instant,
    pixels: Pixels,
}
#[derive(Default)]
struct Slot {
    capture: Option<Arc<Capture>>,
    timer: Option<tokio::task::JoinHandle<()>>,
}
#[derive(Default)]
pub(crate) struct CaptureStore {
    slot: Mutex<Slot>,
}
impl CaptureStore {
    pub(crate) fn put(self: &Arc<Self>, pixels: Pixels, frontend: String) -> CaptureInfo {
        let info = CaptureInfo {
            capture_id: uuid::Uuid::new_v4().to_string(),
            width: pixels.width,
            height: pixels.height,
            tile_size: TILE,
            tile_count: pixels.width.div_ceil(TILE) * pixels.height.div_ceil(TILE),
            rgba_sha256: format!("{:x}", Sha256::digest(&pixels.rgba)),
        };
        let capture = Arc::new(Capture {
            info: info.clone(),
            frontend,
            created: Instant::now(),
            pixels,
        });
        let weak = Arc::downgrade(self);
        let id = info.capture_id.clone();
        let timer = tokio::spawn(async move {
            tokio::time::sleep(TTL).await;
            if let Some(store) = weak.upgrade() {
                let removed = {
                    let mut slot = store.slot.lock().unwrap_or_else(|p| p.into_inner());
                    if slot
                        .capture
                        .as_ref()
                        .is_some_and(|c| c.info.capture_id == id)
                    {
                        slot.capture.take()
                    } else {
                        None
                    }
                };
                drop(removed);
            }
        });
        let (old, timer) = {
            let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
            (slot.capture.replace(capture), slot.timer.replace(timer))
        };
        if let Some(timer) = timer {
            timer.abort();
        }
        drop(old);
        info
    }
    fn get(&self, id: &str, frontend: &str, now: Instant) -> Result<Arc<Capture>, WorkspaceError> {
        self.slot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .capture
            .as_ref()
            .filter(|c| {
                c.info.capture_id == id
                    && c.frontend == frontend
                    && now
                        .checked_duration_since(c.created)
                        .is_some_and(|age| age < TTL)
            })
            .cloned()
            .ok_or_else(|| error(Code::StaleRevision, "截图已失效、被替换或属于另一前端"))
    }
    pub(crate) fn tile(
        &self,
        id: &str,
        frontend: &str,
        index: u32,
    ) -> Result<(CaptureResult, String), WorkspaceError> {
        self.get(id, frontend, Instant::now())?
            .pixels
            .tile(id, index)
    }
    pub(crate) fn release(
        &self,
        id: &str,
        frontend: &str,
    ) -> Result<CaptureResult, WorkspaceError> {
        let removed = {
            let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
            if slot
                .capture
                .as_ref()
                .is_some_and(|c| c.info.capture_id != id || c.frontend != frontend)
            {
                return Err(error(Code::StaleRevision, "不能释放另一截图"));
            }
            if let Some(timer) = slot.timer.take() {
                timer.abort();
            }
            slot.capture.take()
        };
        drop(removed);
        Ok(CaptureResult::Released {
            capture_id: id.into(),
        })
    }
}
impl Drop for CaptureStore {
    fn drop(&mut self) {
        if let Some(timer) = self
            .slot
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .timer
            .take()
        {
            timer.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn replacement_from_blocking_worker_invalidates_previous_snapshot() {
        let store = Arc::new(CaptureStore::default());
        let worker = store.clone();
        let first = tokio::task::spawn_blocking(move || {
            worker.put(
                Pixels {
                    width: 1,
                    height: 1,
                    rgba: vec![20, 30, 40, 128],
                },
                "f".into(),
            )
        })
        .await
        .unwrap();
        assert!(store.get(&first.capture_id, "f", Instant::now()).is_ok());
        let second = store.put(
            Pixels {
                width: 1,
                height: 1,
                rgba: vec![0; 4],
            },
            "f".into(),
        );
        assert!(store.get(&first.capture_id, "f", Instant::now()).is_err());
        store.release(&second.capture_id, "f").unwrap();
        assert!(store.slot.lock().unwrap().capture.is_none());
        assert!(store.slot.lock().unwrap().timer.is_none());
    }
    #[test]
    fn tiles_preserve_edge_dimensions_and_exact_rgba() {
        let pixels = Pixels {
            width: 300,
            height: 260,
            rgba: (0..300 * 260 * 4).map(|i| (i % 251) as u8).collect(),
        };
        let mut combined = vec![0; pixels.rgba.len()];
        for index in 0..4 {
            let (result, png) = pixels.tile("id", index).unwrap();
            let CaptureResult::Tile {
                x,
                y,
                width,
                height,
                ..
            } = result
            else {
                panic!("tile")
            };
            let decoded = Pixels::decode(&png).unwrap();
            assert_eq!((decoded.width, decoded.height), (width, height));
            for row in 0..height {
                let start = (((y + row) * 300 + x) * 4) as usize;
                let from = (row * width * 4) as usize;
                combined[start..start + (width * 4) as usize]
                    .copy_from_slice(&decoded.rgba[from..from + (width * 4) as usize]);
            }
        }
        assert_eq!(combined, pixels.rgba);
        assert!(pixels.tile("id", 4).is_err());
        assert!(Pixels::decode("invalid").is_err());
    }
    #[test]
    fn snapshot_identity_expiry_and_release_are_enforced() {
        let store = CaptureStore::default();
        let now = Instant::now();
        store.slot.lock().unwrap().capture = Some(Arc::new(Capture {
            info: CaptureInfo {
                capture_id: "a".into(),
                width: 1,
                height: 1,
                tile_size: TILE,
                tile_count: 1,
                rgba_sha256: String::new(),
            },
            frontend: "f".into(),
            created: now,
            pixels: Pixels {
                width: 1,
                height: 1,
                rgba: vec![0; 4],
            },
        }));
        assert!(store.get("a", "f", now).is_ok());
        assert!(store.get("a", "other", now).is_err());
        assert!(store.get("a", "f", now + TTL).is_err());
        assert!(store.release("other", "f").is_err());
        store.release("a", "f").unwrap();
        store.release("a", "f").unwrap();
        assert!(store.get("a", "f", now).is_err());
    }
}

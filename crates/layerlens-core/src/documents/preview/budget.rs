//! 解码预留和共享输出容量的独立 RAII 账本；缓存为输出的子集。

use super::super::{DocumentError, model::lock};
use super::{PreviewAccounting, PreviewConfig};
use std::sync::{Arc, Mutex};

pub(in super::super) struct Budget {
    config: PreviewConfig,
    usage: Mutex<PreviewAccounting>,
}
impl Budget {
    pub fn new(config: PreviewConfig) -> Arc<Self> {
        Arc::new(Self {
            config,
            usage: Mutex::new(PreviewAccounting::default()),
        })
    }
    pub fn usage(&self) -> PreviewAccounting {
        *lock(&self.usage)
    }
    pub(super) fn reserve(
        self: &Arc<Self>,
        decode: u64,
    ) -> Result<(DecodePermit, OutputPermit), DocumentError> {
        let mut usage = lock(&self.usage);
        for (used, requested, limit, resource) in [
            (
                usage.decoded_bytes,
                decode,
                self.config.max_decoded_bytes,
                "预览解码字节",
            ),
            (
                usage.output_bytes,
                self.config.max_png_bytes as u64,
                self.config.max_output_bytes,
                "预览输出字节",
            ),
        ] {
            if requested > limit.saturating_sub(used) {
                return Err(DocumentError::ResourceLimit { resource, limit });
            }
        }
        usage.decoded_bytes += decode;
        usage.output_bytes += self.config.max_png_bytes as u64;
        Ok((
            DecodePermit {
                budget: Arc::clone(self),
                charge: decode,
            },
            OutputPermit {
                budget: Arc::clone(self),
                charge: self.config.max_png_bytes as u64,
            },
        ))
    }
}
pub(super) struct DecodePermit {
    budget: Arc<Budget>,
    charge: u64,
}
impl Drop for DecodePermit {
    fn drop(&mut self) {
        lock(&self.budget.usage).decoded_bytes -= self.charge;
    }
}
pub(super) struct OutputPermit {
    budget: Arc<Budget>,
    charge: u64,
}
impl OutputPermit {
    pub fn settle(&mut self, capacity: u64) -> Result<(), DocumentError> {
        if capacity > self.charge {
            return Err(DocumentError::ResourceLimit {
                resource: "单次 PNG 容量",
                limit: self.charge,
            });
        }
        lock(&self.budget.usage).output_bytes -= self.charge - capacity;
        self.charge = capacity;
        Ok(())
    }
}
impl Drop for OutputPermit {
    fn drop(&mut self) {
        lock(&self.budget.usage).output_bytes -= self.charge;
    }
}

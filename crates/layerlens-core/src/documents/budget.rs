//! 全局源字节、声明像素和修订槽位的 RAII 计费，不估算进程 RSS。

use std::sync::{Arc, Mutex};

use super::{DocumentError, DocumentServiceConfig, ResourceAccounting, model::lock};

pub(super) struct Budget {
    config: DocumentServiceConfig,
    usage: Mutex<ResourceAccounting>,
}

impl Budget {
    pub fn new(config: DocumentServiceConfig) -> Arc<Self> {
        Arc::new(Self {
            config,
            usage: Mutex::new(ResourceAccounting::default()),
        })
    }
    pub fn usage(&self) -> ResourceAccounting {
        *lock(&self.usage)
    }
    pub fn reserve(self: &Arc<Self>) -> Result<Permit, DocumentError> {
        let mut usage = lock(&self.usage);
        let charge = ResourceAccounting {
            live_revisions: 1,
            source_bytes: self.config.parse_limits.max_file_bytes,
            declared_pixels: self.config.parse_limits.max_total_pixels,
        };
        // 差值检查避免用户配置为 u64::MAX 时的加法溢出，全部检查通过才修改账本。
        for (used, requested, limit, resource) in [
            (
                usage.live_revisions,
                1,
                self.config.max_live_revisions,
                "修订数量",
            ),
            (
                usage.source_bytes,
                charge.source_bytes,
                self.config.max_source_bytes,
                "源字节",
            ),
            (
                usage.declared_pixels,
                charge.declared_pixels,
                self.config.max_declared_pixels,
                "声明像素",
            ),
        ] {
            if requested > limit.saturating_sub(used) {
                return Err(DocumentError::ResourceLimit { resource, limit });
            }
        }
        usage.live_revisions += 1;
        usage.source_bytes += charge.source_bytes;
        usage.declared_pixels += charge.declared_pixels;
        Ok(Permit {
            budget: Arc::clone(self),
            charge,
        })
    }
}

pub(super) struct Permit {
    budget: Arc<Budget>,
    charge: ResourceAccounting,
}

impl Permit {
    pub fn settle(&mut self, source_bytes: u64, pixels: u64) -> Result<(), DocumentError> {
        for (actual, reserved, resource) in [
            (source_bytes, self.charge.source_bytes, "源字节"),
            (pixels, self.charge.declared_pixels, "声明像素"),
        ] {
            if actual > reserved {
                return Err(DocumentError::ResourceLimit {
                    resource,
                    limit: reserved,
                });
            }
        }
        let mut usage = lock(&self.budget.usage);
        usage.source_bytes -= self.charge.source_bytes - source_bytes;
        usage.declared_pixels -= self.charge.declared_pixels - pixels;
        self.charge.source_bytes = source_bytes;
        self.charge.declared_pixels = pixels;
        Ok(())
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut usage = lock(&self.budget.usage);
        usage.live_revisions -= self.charge.live_revisions;
        usage.source_bytes -= self.charge.source_bytes;
        usage.declared_pixels -= self.charge.declared_pixels;
    }
}

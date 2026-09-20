//! 不可变快照的 RAII 额度；只计范围对象容量，不重复计源数据，不估算进程 RSS。

use std::sync::{Arc, Mutex};

use super::{SelectionAccounting, SelectionConfig, SelectionError, model::limit};
use crate::documents::model::lock;

pub(in crate::documents) struct Budget {
    config: SelectionConfig,
    usage: Mutex<SelectionAccounting>,
}

impl Budget {
    pub fn new(config: SelectionConfig) -> Arc<Self> {
        Arc::new(Self {
            config,
            usage: Mutex::new(SelectionAccounting::default()),
        })
    }
    pub fn usage(&self) -> SelectionAccounting {
        *lock(&self.usage)
    }
    pub(super) fn reserve(self: &Arc<Self>, bytes: u64) -> Result<Permit, SelectionError> {
        let mut usage = lock(&self.usage);
        if usage.live_snapshots >= self.config.max_live_snapshots {
            return Err(limit("选区快照数量", self.config.max_live_snapshots));
        }
        if bytes
            > self
                .config
                .max_scope_bytes
                .saturating_sub(usage.scope_bytes)
        {
            return Err(limit("选区范围字节", self.config.max_scope_bytes));
        }
        usage.live_snapshots += 1;
        usage.scope_bytes += bytes;
        Ok(Permit {
            budget: Arc::clone(self),
            bytes,
        })
    }
}

pub(super) struct Permit {
    budget: Arc<Budget>,
    bytes: u64,
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut usage = lock(&self.budget.usage);
        usage.live_snapshots -= 1;
        usage.scope_bytes -= self.bytes;
    }
}

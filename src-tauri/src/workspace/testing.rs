//! 仅开发测试实例启用的计划时钟与命名响应检查点。只控制时间／交付顺序，不改业务结果。
//! 检查点不占操作线程或同步锁；最多四个活跃、32 个会话记录，超时／退出唤醒等待者。

use super::error;
use layerlens_core::workspace_contract::{WorkspaceError, WorkspaceErrorCode as Code};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Notify;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Checkpoint {
    DesktopTasks,
    DesktopTaskCreated,
    DesktopContent,
    DesktopCleanupCommitted,
    DesktopSnapshot,
    DesktopNotification,
    PreviewPublication,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Mode {
    Pause,
    Fail,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Phase {
    Armed,
    Reached,
    Released,
    Failed,
    Expired,
    Abandoned,
    Stopped,
}
impl Phase {
    fn active(self) -> bool {
        matches!(self, Self::Armed | Self::Reached)
    }
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Gate {
    pub id: u32,
    pub checkpoint: Checkpoint,
    pub mode: Mode,
    pub phase: Phase,
}
struct Entry {
    gate: Gate,
    deadline: Instant,
}
#[derive(Default)]
pub(crate) struct Controls {
    enabled: AtomicBool,
    stopped: AtomicBool,
    clock_seconds: AtomicU64,
    gates: Mutex<BTreeMap<u32, Entry>>,
    changed: Notify,
}

impl Controls {
    pub(crate) fn enable(&self) {
        self.enabled.store(true, Ordering::Release);
    }
    fn check(&self) -> Result<(), WorkspaceError> {
        if !self.enabled.load(Ordering::Acquire) {
            return Err(error(Code::InvalidInput, "未启用开发测试实例。"));
        }
        if self.stopped.load(Ordering::Acquire) {
            return Err(error(Code::ShuttingDown, "测试控制已停止。"));
        }
        Ok(())
    }
    /// 偏移仅供清理计划使用；网络、等待和检查点始终使用真实单调时间。
    pub(crate) fn plan_now(&self) -> Instant {
        Instant::now() + Duration::from_secs(self.clock_seconds.load(Ordering::Acquire))
    }
    pub(crate) fn advance(&self, seconds: u32) -> Result<u64, WorkspaceError> {
        self.check()?;
        if !(1..=3600).contains(&seconds) {
            return Err(error(Code::InvalidInput, "计划时钟单次前进 1–3600 秒。"));
        }
        self.clock_seconds
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                old.checked_add(u64::from(seconds)).filter(|n| *n <= 86_400)
            })
            .map(|old| old + u64::from(seconds))
            .map_err(|_| error(Code::ResourceLimit, "计划时钟累计偏移上限为一天。"))
    }
    fn expire(entries: &mut BTreeMap<u32, Entry>, now: Instant) {
        for entry in entries.values_mut() {
            if entry.gate.phase.active() && now >= entry.deadline {
                entry.gate.phase = Phase::Expired;
            }
        }
    }
    pub(crate) fn arm(
        &self,
        checkpoint: Checkpoint,
        mode: Mode,
        timeout_ms: u32,
    ) -> Result<Gate, WorkspaceError> {
        self.check()?;
        if !(1..=30_000).contains(&timeout_ms) {
            return Err(error(Code::InvalidInput, "检查点有效期为 1–30000 ms。"));
        }
        let mut entries = self.gates.lock().unwrap_or_else(|p| p.into_inner());
        self.check()?; // 与 stop 的状态遍历串行，退出后不能再登记新检查点。
        Self::expire(&mut entries, Instant::now());
        if entries.len() >= 32 || entries.values().filter(|e| e.gate.phase.active()).count() >= 4 {
            return Err(error(Code::ResourceLimit, "检查点活跃数或会话记录已满。"));
        }
        if entries
            .values()
            .any(|e| e.gate.checkpoint == checkpoint && e.gate.phase.active())
        {
            return Err(error(Code::Busy, "此命名检查点已有活跃控制。"));
        }
        let id = entries.len() as u32 + 1;
        let gate = Gate {
            id,
            checkpoint,
            mode,
            phase: Phase::Armed,
        };
        entries.insert(
            id,
            Entry {
                gate: gate.clone(),
                deadline: Instant::now() + Duration::from_millis(u64::from(timeout_ms)),
            },
        );
        Ok(gate)
    }
    pub(crate) fn state(&self, id: u32) -> Result<Gate, WorkspaceError> {
        let mut entries = self.gates.lock().unwrap_or_else(|p| p.into_inner());
        Self::expire(&mut entries, Instant::now());
        entries
            .get(&id)
            .map(|e| e.gate.clone())
            .ok_or_else(|| error(Code::NotFound, "检查点记录不存在。"))
    }
    /// 按票据幂等释放，已结束记录不重开；不依赖变更执行名额，保证故障恢复可达。
    pub(crate) fn release(&self, id: u32) -> Result<Gate, WorkspaceError> {
        self.check()?;
        let gate = {
            let mut entries = self.gates.lock().unwrap_or_else(|p| p.into_inner());
            Self::expire(&mut entries, Instant::now());
            let entry = entries
                .get_mut(&id)
                .ok_or_else(|| error(Code::NotFound, "检查点记录不存在。"))?;
            if entry.gate.phase.active() {
                entry.gate.phase = Phase::Released;
            }
            entry.gate.clone()
        };
        self.changed.notify_waiters();
        Ok(gate)
    }
    pub(crate) async fn wait(
        &self,
        id: u32,
        phase: Phase,
        timeout_ms: u32,
    ) -> Result<(bool, Gate), WorkspaceError> {
        self.check()?;
        if !(1..=30_000).contains(&timeout_ms) {
            return Err(error(Code::InvalidInput, "检查点等待为 1–30000 ms。"));
        }
        let deadline = tokio::time::Instant::now() + Duration::from_millis(u64::from(timeout_ms));
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let gate = self.state(id)?;
            if gate.phase == phase || !gate.phase.active() {
                return Ok((gate.phase == phase, gate));
            }
            let expiry = self
                .gates
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&id)
                .expect("会话记录不删除")
                .deadline;
            tokio::select! {
                _ = changed => {},
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(expiry)) => {},
                _ = tokio::time::sleep_until(deadline) => {
                    let gate = self.state(id)?;
                    return Ok((gate.phase == phase, gate));
                }
            }
        }
    }
    /// 在真实结果已计算／固定租约后调用；等待期间不持有业务锁或工作线程。
    pub(crate) async fn hit(&self, checkpoint: Checkpoint) -> Result<(), WorkspaceError> {
        if !self.enabled.load(Ordering::Acquire) {
            return Ok(());
        }
        self.check()?;
        let entry = {
            let mut entries = self.gates.lock().unwrap_or_else(|p| p.into_inner());
            Self::expire(&mut entries, Instant::now());
            let entry = entries
                .values_mut()
                .find(|e| e.gate.checkpoint == checkpoint && e.gate.phase == Phase::Armed);
            entry.map(|e| {
                e.gate.phase = if e.gate.mode == Mode::Fail {
                    Phase::Failed
                } else {
                    Phase::Reached
                };
                (e.gate.clone(), e.deadline)
            })
        };
        let Some((gate, deadline)) = entry else {
            return Ok(());
        };
        self.changed.notify_waiters();
        if gate.mode == Mode::Fail {
            return Err(error(
                Code::Internal,
                format!(
                    "开发检查点 {:?} 单次交付故障（业务结果未改写）。",
                    checkpoint
                ),
            ));
        }
        let guard = Waiting {
            controls: self,
            id: gate.id,
        };
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            match self.state(gate.id)?.phase {
                Phase::Released => {
                    drop(guard);
                    return Ok(());
                }
                Phase::Stopped => return Err(error(Code::ShuttingDown, "检查点随实例退出终止。")),
                Phase::Expired => {
                    return Err(error(
                        Code::Internal,
                        "检查点超时，拒绝迟到交付；请核对业务状态。",
                    ));
                }
                Phase::Reached => {}
                _ => return Err(error(Code::Internal, "检查点等待已取消。")),
            }
            tokio::select! {
                _ = changed => {},
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {},
            }
        }
    }
    pub(crate) fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        for entry in self
            .gates
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values_mut()
        {
            if entry.gate.phase.active() {
                entry.gate.phase = Phase::Stopped;
            }
        }
        self.changed.notify_waiters();
    }
}
struct Waiting<'a> {
    controls: &'a Controls,
    id: u32,
}
impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        if let Some(entry) = self
            .controls
            .gates
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(&self.id)
            && entry.gate.phase == Phase::Reached
        {
            entry.gate.phase = Phase::Abandoned;
        }
        self.controls.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests;

//! 本实例与 WebView 后代的 OS 内存快照，通过 sysinfo 安全封装读取。
//! 不接受外部 PID；只在显式启用开发测试 MCP 的隔离实例注册工具。
use crate::workspace::error;
use layerlens_core::workspace_contract::{WorkspaceError, WorkspaceErrorCode};
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProcessMemory {
    pub process_id: u32,
    pub parent_id: u32,
    pub role: String,
    pub start_time_seconds: u64,
    pub working_set_bytes: u64,
    pub private_bytes: u64,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProcessMetrics {
    pub root_process_id: u32,
    pub sample_ms: f64,
    pub processes: Vec<ProcessMemory>,
    pub errors: Vec<String>,
    pub observations: Vec<ProcessObservation>,
    pub note: String,
}

/// 进程树枚举和采样之间的身份变化；不把排除或退出进程的内存计为零。
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProcessObservation {
    pub process_id: u32,
    pub parent_id: u32,
    pub start_time_seconds: u64,
    pub parent_start_time_seconds: u64,
    pub reason: String,
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    pid: u32,
    parent: u32,
    started: u64,
}

#[cfg(windows)]
impl Identity {
    fn from_process(process: &sysinfo::Process) -> Self {
        Self {
            pid: process.pid().as_u32(),
            parent: process.parent().map_or(0, |p| p.as_u32()),
            started: process.start_time(),
        }
    }
    fn observation(self, parent_started: u64, reason: &str) -> ProcessObservation {
        ProcessObservation {
            process_id: self.pid,
            parent_id: self.parent,
            start_time_seconds: self.started,
            parent_start_time_seconds: parent_started,
            reason: reason.into(),
        }
    }
}

#[cfg(windows)]
#[derive(Debug, PartialEq, Eq)]
enum IdentityCheck {
    Valid,
    Exited,
    Foreign,
    Invalid,
}

#[cfg(windows)]
fn check_identity(before: Identity, after: Option<Identity>, parent_started: u64) -> IdentityCheck {
    let Some(after) = after else {
        return IdentityCheck::Exited;
    };
    if after.started == 0
        || parent_started == 0
        || before.pid != after.pid
        || before.parent != after.parent
    {
        return IdentityCheck::Invalid;
    }
    if after.started < parent_started {
        return IdentityCheck::Foreign;
    }
    if before.started != 0 && before.started != after.started {
        return IdentityCheck::Invalid;
    }
    IdentityCheck::Valid
}

pub(super) async fn read() -> Result<ProcessMetrics, WorkspaceError> {
    tauri::async_runtime::spawn_blocking(snapshot)
        .await
        .map_err(|e| {
            error(
                WorkspaceErrorCode::Internal,
                format!("进程采样任务失败：{e}"),
            )
        })?
}

#[cfg(not(windows))]
fn snapshot() -> Result<ProcessMetrics, WorkspaceError> {
    Err(error(
        WorkspaceErrorCode::InvalidInput,
        "进程资源采样目前仅验证 Windows",
    ))
}

#[cfg(windows)]
fn snapshot() -> Result<ProcessMetrics, WorkspaceError> {
    use std::{collections::HashMap, time::Instant};
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let started = Instant::now();
    let root = Pid::from_u32(std::process::id());
    let mut system = System::new();
    // 首次只枚举身份／父子关系；内存刷新限定为本实例及其 WebView 后代。
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let own = system
        .process(root)
        .ok_or_else(|| error(WorkspaceErrorCode::Internal, "OS 枚举未返回本实例"))?;
    let root_identity = Identity::from_process(own);
    if root_identity.started == 0 {
        return Err(error(
            WorkspaceErrorCode::Internal,
            "本实例创建时间不可读取",
        ));
    }
    let mut selected = HashMap::from([(root, root_identity)]);
    let mut queue = vec![root];
    let mut errors = Vec::new();
    let mut observations = Vec::new();
    while let Some(parent) = queue.pop() {
        for (&pid, process) in system.processes() {
            if process.parent() != Some(parent)
                || selected.contains_key(&pid)
                || !process.name().eq_ignore_ascii_case("msedgewebview2.exe")
            {
                continue;
            }
            let identity = Identity::from_process(process);
            let parent_started = selected[&parent].started;
            // PPID 可以指向复用的 PID。已知早于当前父进程的候选不属于此树。
            // 零时间是读取失败，不能按“早于父进程”排除；在第二阶段重新确认身份。
            if identity.started != 0 && identity.started < parent_started {
                observations.push(identity.observation(parent_started, "excludedOlderThanParent"));
                continue;
            }
            selected.insert(pid, identity);
            queue.push(pid);
            if selected.len() > 128 {
                return Err(error(
                    WorkspaceErrorCode::ResourceLimit,
                    "实验进程树超过 128 项",
                ));
            }
        }
    }
    let pids: Vec<_> = selected.keys().copied().collect();
    // sysinfo 0.35.1 在现有 Process 上保留 start_time。使用新的身份枚举，
    // 避免把缓存时间与自身比较；确认归属后才读取这些进程的内存。
    let mut measured = System::new();
    measured.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing(),
    );
    if measured.process(root).map(Identity::from_process) != Some(root_identity) {
        return Err(error(
            WorkspaceErrorCode::Internal,
            "采样期间本实例身份不可验证",
        ));
    }
    let mut verified = HashMap::from([(root, root_identity)]);
    let mut ancestry = HashMap::from([(root, Some(root_identity.started))]);
    let mut queue = vec![root];
    while let Some(parent) = queue.pop() {
        for (&pid, &identity) in &selected {
            if pid == root || identity.parent != parent.as_u32() {
                continue;
            }
            let current = measured.process(pid).map(Identity::from_process);
            let parent_started = ancestry[&parent].unwrap_or(0);
            if measured
                .process(pid)
                .is_some_and(|p| !p.name().eq_ignore_ascii_case("msedgewebview2.exe"))
            {
                errors.push(format!("{}: 采样期间进程名称改变", pid.as_u32()));
                ancestry.insert(pid, None);
                queue.push(pid);
                continue;
            }
            match check_identity(identity, current, parent_started) {
                IdentityCheck::Valid => {
                    // Valid 已确认 current 存在；保持显式匹配而不依赖 unwrap。
                    if let Some(current) = current {
                        if identity.started == 0 {
                            observations.push(
                                current
                                    .observation(parent_started, "identityConfirmedAtMeasurement"),
                            );
                        }
                        verified.insert(pid, current);
                        ancestry.insert(pid, Some(current.started));
                    }
                }
                IdentityCheck::Exited => {
                    observations
                        .push(identity.observation(parent_started, "exitedBeforeMeasurement"));
                    ancestry.insert(pid, (identity.started != 0).then_some(identity.started));
                }
                IdentityCheck::Foreign => {
                    if let Some(current) = current {
                        observations
                            .push(current.observation(parent_started, "excludedOlderThanParent"));
                    }
                    // 不遍历已确认属于其他进程代次的分支。
                    continue;
                }
                IdentityCheck::Invalid => {
                    errors.push(format!("{}: 进程身份不可验证；枚举={identity:?}，采样={current:?}，父创建时间={parent_started}", pid.as_u32()));
                    ancestry.insert(pid, None);
                }
            }
            queue.push(pid);
        }
    }
    let pids: Vec<_> = verified.keys().copied().collect();
    measured.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );
    let mut processes = Vec::new();
    for pid in pids {
        let Some(process) = measured.process(pid) else {
            if pid == root {
                errors.push("内存刷新未返回本实例".into());
            } else {
                observations.push(verified[&pid].observation(
                    ancestry[&Pid::from_u32(verified[&pid].parent)].unwrap_or(0),
                    "exitedDuringMeasurement",
                ));
            }
            continue;
        };
        if Identity::from_process(process) != verified[&pid] || process.memory() == 0 {
            errors.push(format!("{}: 身份改变或内存不可读取", pid.as_u32()));
            continue;
        }
        processes.push(ProcessMemory {
            process_id: pid.as_u32(),
            parent_id: process.parent().map_or(0, |p| p.as_u32()),
            role: if pid == root {
                "application"
            } else {
                "webview"
            }
            .into(),
            start_time_seconds: process.start_time(),
            working_set_bytes: process.memory(),
            // 锁定的 sysinfo 0.35.1 Windows 实现将 PROCESS_MEMORY_COUNTERS_EX.PrivateUsage
            // 暴露为 virtual_memory；此处命名为私有提交量，不能称为私有 RSS。
            private_bytes: process.virtual_memory(),
        });
    }
    processes.sort_by_key(|p| p.process_id);
    observations.sort_by_key(|o| o.process_id);
    Ok(ProcessMetrics { root_process_id: root.as_u32(), sample_ms: started.elapsed().as_secs_f64() * 1000.0, processes, errors, observations, note: "Windows 工作集含共享页，跨进程求和可能重复计费；privateBytes 是私有提交量。两阶段身份枚举后读取内存；已确认退出或属于旧父 PID 的候选单列 observations，不计入总和。身份未知仍报错。离散快照不代表峰值或 GPU 显存，创建时间精度为秒。".into() })
}

#[cfg(all(test, windows))]
mod tests {
    use super::{Identity, IdentityCheck, check_identity};
    #[test]
    fn identity_checks_distinguish_departure_reuse_and_unreadable_time() {
        let before = Identity {
            pid: 12,
            parent: 10,
            started: 100,
        };
        assert_eq!(
            check_identity(before, Some(before), 100),
            IdentityCheck::Valid
        );
        assert_eq!(check_identity(before, None, 100), IdentityCheck::Exited);
        assert_eq!(
            check_identity(
                before,
                Some(Identity {
                    started: 99,
                    ..before
                }),
                100
            ),
            IdentityCheck::Foreign
        );
        assert_eq!(
            check_identity(
                before,
                Some(Identity {
                    started: 101,
                    ..before
                }),
                100
            ),
            IdentityCheck::Invalid
        );
        assert_eq!(
            check_identity(
                before,
                Some(Identity {
                    started: 0,
                    ..before
                }),
                100
            ),
            IdentityCheck::Invalid
        );
        assert_eq!(
            check_identity(
                before,
                Some(Identity {
                    parent: 11,
                    ..before
                }),
                100
            ),
            IdentityCheck::Invalid
        );
        assert_eq!(
            check_identity(before, Some(Identity { pid: 13, ..before }), 100),
            IdentityCheck::Invalid
        );
        assert_eq!(
            check_identity(before, Some(before), 0),
            IdentityCheck::Invalid
        );
        let unknown = Identity {
            started: 0,
            ..before
        };
        assert_eq!(
            check_identity(unknown, Some(before), 100),
            IdentityCheck::Valid
        );
        assert_eq!(
            check_identity(unknown, Some(unknown), 100),
            IdentityCheck::Invalid
        );
    }
    #[test]
    fn snapshot_contains_own_process_without_accepting_arbitrary_pid() {
        let metrics = super::snapshot().unwrap();
        let own = metrics
            .processes
            .iter()
            .find(|p| p.process_id == std::process::id())
            .unwrap();
        assert_eq!(own.role, "application");
        assert!(own.working_set_bytes > 0 && own.private_bytes > 0);
    }
}

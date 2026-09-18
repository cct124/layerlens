//! 后台预览计算；取消和发布同锁裁决，所有像素和修订析构位于锁外。

use super::super::{
    DocumentError, JobStatus, PreviewJobId,
    model::{Revision, lock},
    state::{Execution, Shared},
};
use super::{
    PreviewLease, PreviewOutcome, State,
    model::{Image, Key},
};
use crate::psd::{PngTimings, PsdDocument, PsdError};
use std::{sync::Arc, time::Instant};

pub(in super::super) type Renderer =
    Box<dyn Fn(&PsdDocument, usize) -> Result<(Vec<u8>, PngTimings), PsdError> + Send>;

pub(in super::super) struct Work {
    id: PreviewJobId,
    key: Key,
    revision: Arc<Revision>,
}
impl Work {
    pub fn start(state: &mut State) -> Self {
        let id = state.queue.pop_front().expect("非空预览队列必须有首项");
        let job = state.pending.get_mut(&id).expect("排队预览记录必须存在");
        job.execution = Execution::Running(Instant::now());
        job.handle.signal.set(JobStatus::Running);
        Self {
            id,
            key: job.key,
            revision: job.revision.take().expect("排队预览持有修订"),
        }
    }
}

pub(in super::super) fn render(shared: &Shared, work: Work, renderer: &Renderer) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        compute(shared, &work, renderer)
    }))
    .unwrap_or(Err(DocumentError::WorkerPanicked));
    let mut discarded = None;
    let mut evicted = Vec::new();
    let (outcome, timings) = {
        let mut state = lock(&shared.state);
        let job = state
            .previews
            .pending
            .get_mut(&work.id)
            .expect("发布前保留预览作业");
        let cancelled = matches!(job.execution, Execution::Cancelling(_));
        job.execution = Execution::Finishing(job.execution.started_at().expect("作业已经开始"));
        job.handle.signal.set(JobStatus::Finishing);
        let current = state
            .documents
            .iter()
            .any(|entry| entry.revision.id == work.key.revision);
        if cancelled || state.stopping || !current {
            discarded = Some(result);
            (PreviewOutcome::Cancelled, None)
        } else {
            match result {
                Ok(Some((image, timings))) => {
                    evicted = state.previews.insert(&image, shared.config.preview);
                    (PreviewOutcome::Ready(image), Some(timings))
                }
                Ok(None) => (PreviewOutcome::Cancelled, None),
                Err(error) => (PreviewOutcome::Failed(Arc::new(error)), None),
            }
        }
    };
    let id = work.id;
    drop(discarded);
    drop(evicted);
    drop(work);
    let job = lock(&shared.state)
        .previews
        .pending
        .remove(&id)
        .expect("释放期间保留预览名额");
    job.finish(outcome, timings);
}

fn compute(
    shared: &Shared,
    work: &Work,
    renderer: &Renderer,
) -> Result<Option<(PreviewLease, PngTimings)>, DocumentError> {
    let (decode_permit, mut output_permit) = loop {
        if cancelled(shared, work.id) {
            return Ok(None);
        }
        match shared
            .preview_budget
            .reserve(shared.config.parse_limits.max_decoded_bytes)
        {
            Ok(permits) => break permits,
            Err(error) => {
                // 先逐个移除最旧缓存，锁外释放，再重试；外部引用仍计费，不假定淘汰即回收。
                let evicted = lock(&shared.state).previews.evict();
                if evicted.is_none() {
                    return Err(error);
                }
                drop(evicted);
            }
        }
    };
    if cancelled(shared, work.id) {
        return Ok(None);
    }
    let (bytes, timings) = renderer(&work.revision.parsed, shared.config.preview.max_png_bytes)
        .map_err(|source| DocumentError::Preview {
            document: work.key.document,
            revision: work.key.revision,
            source,
        })?;
    output_permit.settle(bytes.capacity() as u64)?;
    let info = work.revision.parsed.info();
    let image = PreviewLease(Arc::new(Image {
        key: work.key,
        width: info.width,
        height: info.height,
        capability: info.preview.clone(),
        bytes,
        _permit: output_permit,
    }));
    drop(decode_permit);
    Ok(Some((image, timings)))
}

fn cancelled(shared: &Shared, id: PreviewJobId) -> bool {
    matches!(
        lock(&shared.state)
            .previews
            .pending
            .get(&id)
            .expect("运行时保留预览记录")
            .execution,
        Execution::Cancelling(_)
    )
}

use super::*;
use std::sync::Arc;

#[tokio::test]
async fn checkpoint_is_one_shot_recoverable_and_shutdown_wakes_waiter() {
    let controls = Arc::new(Controls::default());
    assert!(
        controls
            .arm(Checkpoint::DesktopTasks, Mode::Pause, 1000)
            .is_err()
    );
    controls.hit(Checkpoint::DesktopTasks).await.unwrap();
    controls.enable();
    let gate = controls
        .arm(Checkpoint::DesktopTasks, Mode::Pause, 30_000)
        .unwrap();
    let worker = controls.clone();
    let task = tokio::spawn(async move { worker.hit(Checkpoint::DesktopTasks).await });
    assert!(
        controls
            .wait(gate.id, Phase::Reached, 1000)
            .await
            .unwrap()
            .0
    );
    // 同名后续调用不重复进入一次性检查点。
    controls.hit(Checkpoint::DesktopTasks).await.unwrap();
    assert_eq!(controls.release(gate.id).unwrap().phase, Phase::Released);
    assert_eq!(controls.release(gate.id).unwrap().phase, Phase::Released);
    task.await.unwrap().unwrap();
    let gate = controls
        .arm(Checkpoint::DesktopContent, Mode::Pause, 30_000)
        .unwrap();
    let worker = controls.clone();
    let task = tokio::spawn(async move { worker.hit(Checkpoint::DesktopContent).await });
    controls.wait(gate.id, Phase::Reached, 1000).await.unwrap();
    controls.stop();
    assert_eq!(task.await.unwrap().unwrap_err().code, Code::ShuttingDown);
    assert_eq!(controls.state(gate.id).unwrap().phase, Phase::Stopped);
}

#[tokio::test]
async fn abandoned_failed_expired_and_capacity_paths_are_bounded() {
    let controls = Arc::new(Controls::default());
    controls.enable();
    let gate = controls
        .arm(Checkpoint::DesktopTasks, Mode::Pause, 30_000)
        .unwrap();
    let worker = controls.clone();
    let task = tokio::spawn(async move { worker.hit(Checkpoint::DesktopTasks).await });
    controls.wait(gate.id, Phase::Reached, 1000).await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(controls.state(gate.id).unwrap().phase, Phase::Abandoned);
    let gate = controls
        .arm(Checkpoint::DesktopSnapshot, Mode::Fail, 30_000)
        .unwrap();
    assert!(controls.hit(Checkpoint::DesktopSnapshot).await.is_err());
    controls.hit(Checkpoint::DesktopSnapshot).await.unwrap();
    assert_eq!(controls.state(gate.id).unwrap().phase, Phase::Failed);
    for point in [
        Checkpoint::DesktopTasks,
        Checkpoint::DesktopTaskCreated,
        Checkpoint::DesktopContent,
        Checkpoint::DesktopSnapshot,
    ] {
        controls.arm(point, Mode::Pause, 30_000).unwrap();
    }
    assert_eq!(
        controls
            .arm(Checkpoint::PreviewPublication, Mode::Pause, 30_000)
            .unwrap_err()
            .code,
        Code::ResourceLimit
    );
    {
        let mut entries = controls.gates.lock().unwrap();
        Controls::expire(&mut entries, Instant::now() + Duration::from_secs(31));
    }
    assert_eq!(controls.state(3).unwrap().phase, Phase::Expired);
    assert!(
        controls
            .arm(Checkpoint::PreviewPublication, Mode::Pause, 30_001)
            .is_err()
    );
    for _ in 6..32 {
        let gate = controls
            .arm(Checkpoint::PreviewPublication, Mode::Pause, 30_000)
            .unwrap();
        controls.release(gate.id).unwrap();
    }
    assert_eq!(
        controls
            .arm(Checkpoint::PreviewPublication, Mode::Pause, 30_000)
            .unwrap_err()
            .code,
        Code::ResourceLimit
    );
}

#[test]
fn plan_clock_does_not_expire_real_time_checkpoints() {
    let controls = Controls::default();
    controls.enable();
    let gate = controls
        .arm(Checkpoint::DesktopTasks, Mode::Pause, 30_000)
        .unwrap();
    assert_eq!(controls.advance(301).unwrap(), 301);
    assert!(controls.plan_now() >= Instant::now() + Duration::from_secs(300));
    assert_eq!(controls.state(gate.id).unwrap().phase, Phase::Armed);
    assert!(controls.advance(0).is_err());
    assert!(controls.advance(3601).is_err());
    for _ in 0..23 {
        controls.advance(3600).unwrap();
    }
    assert_eq!(
        controls.advance(3600).unwrap_err().code,
        Code::ResourceLimit
    );
}

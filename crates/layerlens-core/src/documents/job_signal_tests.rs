//! 固定发布交错，验证终态可见性与后台持有资源的释放顺序。

use super::{JobSignal, JobStatus};
use std::{
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

const DEADLINE: Duration = Duration::from_secs(10);

#[test]
fn completion_is_visible_only_after_the_publisher_releases_its_handle() {
    let signal = Arc::new(JobSignal::new());
    let publisher = Arc::clone(&signal);
    let resource = Arc::new(());
    let resource_ref = Arc::downgrade(&resource);
    let (published, publication) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let worker = thread::spawn(move || {
        publisher.finish_with(resource, || {
            published.send(()).unwrap();
            released.recv_timeout(DEADLINE).unwrap();
        });
    });
    publication.recv_timeout(DEADLINE).unwrap();
    assert!(matches!(signal.status(), JobStatus::Finishing));
    assert!(signal.wait_timeout(Duration::ZERO).is_none());

    let waiter = Arc::clone(&signal);
    let waiting = thread::spawn(move || waiter.wait());
    release.send(()).unwrap();
    let result = signal.wait_timeout(DEADLINE).unwrap();
    let other_result = waiting.join().unwrap();
    assert!(Arc::ptr_eq(&result, &other_result));
    assert!(matches!(signal.status(), JobStatus::Finished(_)));
    // 不等待 worker join：观察到完成本身就必须保证后台不再持有结果。
    assert_eq!(Arc::strong_count(&signal), 1);
    drop((signal, other_result));
    assert!(resource_ref.upgrade().is_some());
    drop(result);
    assert!(resource_ref.upgrade().is_none());
    worker.join().unwrap();
}

#[test]
fn completion_without_observers_releases_its_result() {
    let signal = Arc::new(JobSignal::new());
    let signal_ref = Arc::downgrade(&signal);
    let resource = Arc::new(());
    let resource_ref = Arc::downgrade(&resource);
    signal.finish(resource);
    assert!(signal_ref.upgrade().is_none());
    assert!(resource_ref.upgrade().is_none());
}

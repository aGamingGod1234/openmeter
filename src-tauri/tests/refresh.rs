use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use openmeter_lib::accounts::AccountContext;
use openmeter_lib::cache::SnapshotCache;
use openmeter_lib::providers::{Metric, ProviderSnapshot};
use openmeter_lib::refresh::{RefreshCoordinator, RefreshTarget};

#[test]
fn concurrent_accounts_are_retained_and_failures_use_only_their_last_good() {
    tauri::async_runtime::block_on(async {
        let now = Arc::new(AtomicI64::new(1_000));
        let coordinator =
            RefreshCoordinator::in_memory(SnapshotCache::default(), clock(Arc::clone(&now)));
        let personal = AccountContext::default_for("claude").unwrap();
        let work = AccountContext::named("claude", "work", "Work").unwrap();
        let personal_fetches = queue(vec![
            ok(&personal, 10.0),
            ProviderSnapshot::error("claude", "Claude", "HTTP 503".to_string()),
        ]);
        let work_fetches = queue(vec![ok(&work, 20.0), ok(&work, 30.0)]);
        let targets = vec![
            RefreshTarget::new(personal.clone(), "credential-a", personal_fetches),
            RefreshTarget::new(work.clone(), "credential-work", work_fetches),
        ];

        let first = coordinator.refresh(false, None, &targets).await;
        assert_eq!(first.len(), 2);
        assert!(first.iter().any(|snapshot| snapshot.card_id == "claude"));
        assert!(first
            .iter()
            .any(|snapshot| snapshot.card_id == "claude--work"));

        now.store(1_500, Ordering::SeqCst);
        let second = coordinator.refresh(true, Some("claude"), &targets).await;
        let personal_result = second
            .iter()
            .find(|snapshot| snapshot.card_id == "claude")
            .unwrap();
        let work_result = second
            .iter()
            .find(|snapshot| snapshot.card_id == "claude--work")
            .unwrap();
        assert_eq!(personal_result.metrics[0].used_percent, Some(10.0));
        assert!(personal_result.stale);
        assert_eq!(personal_result.warning.as_deref(), Some("HTTP 503"));
        assert_eq!(work_result.metrics[0].used_percent, Some(30.0));
    });
}

#[test]
fn freshness_and_filters_are_obeyed_while_force_still_respects_cooldown() {
    tauri::async_runtime::block_on(async {
        let now = Arc::new(AtomicI64::new(10_000));
        let calls = Arc::new(AtomicUsize::new(0));
        let account = AccountContext::default_for("codex").unwrap();
        let fetches = counted_queue(
            vec![
                ok(&account, 42.0),
                ProviderSnapshot::error("codex", "Codex", "HTTP 429 retry_after_s=120".to_string()),
            ],
            Arc::clone(&calls),
        );
        let target = RefreshTarget::new(account, "credential-c", fetches);
        let targets = vec![target];
        let coordinator =
            RefreshCoordinator::in_memory(SnapshotCache::default(), clock(Arc::clone(&now)));

        coordinator.refresh(false, None, &targets).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        coordinator.refresh(false, None, &targets).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1, "fresh cache avoids fetch");

        coordinator.refresh(true, Some("codex"), &targets).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2, "force bypasses freshness");
        coordinator.refresh(true, Some("codex"), &targets).await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "force does not bypass Retry-After"
        );

        assert!(coordinator
            .refresh(false, Some("claude"), &targets)
            .await
            .is_empty());
        assert_eq!(coordinator.cached(Some("codex"), &targets).len(), 1);
    });
}

fn clock(now: Arc<AtomicI64>) -> Arc<dyn Fn() -> i64 + Send + Sync> {
    Arc::new(move || now.load(Ordering::SeqCst))
}

fn queue(values: Vec<ProviderSnapshot>) -> impl Fn() -> openmeter_lib::refresh::FetchFuture {
    let values = Arc::new(Mutex::new(VecDeque::from(values)));
    move || {
        let value = values.lock().unwrap().pop_front().expect("queued snapshot");
        Box::pin(async move { value })
    }
}

fn counted_queue(
    values: Vec<ProviderSnapshot>,
    calls: Arc<AtomicUsize>,
) -> impl Fn() -> openmeter_lib::refresh::FetchFuture {
    let values = Arc::new(Mutex::new(VecDeque::from(values)));
    move || {
        calls.fetch_add(1, Ordering::SeqCst);
        let value = values.lock().unwrap().pop_front().expect("queued snapshot");
        Box::pin(async move { value })
    }
}

fn ok(account: &AccountContext, used: f64) -> ProviderSnapshot {
    ProviderSnapshot::ok(
        &account.card_id,
        &account.display_name,
        Some("Pro".to_string()),
        vec![Metric::progress("Session", used, None)],
    )
}

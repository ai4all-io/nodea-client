// Switching the node to a different model: download, drain, unload, load.
//
// The order is the backend's capacity model (scheduler spec §6.3), so each
// step has to hold for the backend's arithmetic to be true:
//
// - The download runs with NO model_manager lock held. Generations need that
//   lock, so a node that downloads under it serves nothing for minutes while
//   the backend still counts it as capacity for the old model.
// - Drain before taking the lock: stop accepting, let in-flight generations
//   finish (cancelling stragglers after a timeout).
// - Unload, THEN load. Building the new model while the old one is resident
//   peaks both at once, which is what "This model does not fit on the
//   devices" was.
// - On failure, reload the previous model so the node is not left empty.

use crate::generations::Generations;
use crate::model_manager::ModelManager;
use futures::future::BoxFuture;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// The one-model slot reload() swaps. ModelManager in the app; a fake in tests,
/// since a real model cannot be built without a GPU.
pub trait ModelSlot: Send {
    fn loaded_path(&self) -> Option<PathBuf>;
    fn unload(&mut self);
    fn load(&mut self, path: PathBuf) -> BoxFuture<'_, Result<(), String>>;
}

impl ModelSlot for ModelManager {
    fn loaded_path(&self) -> Option<PathBuf> {
        self.loaded_model_path().map(Path::to_path_buf)
    }

    fn unload(&mut self) {
        ModelManager::unload(self);
    }

    fn load(&mut self, path: PathBuf) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(self.load_model(path))
    }
}

/// A step of the switch, for the caller to report as a ModelLoadStatus.
/// Downloading is reported by the download future itself.
#[derive(Clone, Debug, PartialEq)]
pub enum ReloadEvent {
    /// Download done; no longer accepting generations.
    Draining,
    /// Old model unloaded, new one building; `load_ms` elapsed so far.
    Loading {
        load_ms: u32,
    },
    Ready {
        load_ms: u32,
    },
    Failed {
        error: String,
    },
    /// The new model failed and the previous one is back in service.
    Restored {
        path: PathBuf,
        load_ms: u32,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct ReloadTimeouts {
    /// How long in-flight generations get to finish before being cancelled.
    pub drain: Duration,
    /// Bounds each model build - never the download, which has its own limits.
    pub load: Duration,
    /// How often Loading is re-reported with the elapsed time.
    pub loading_report_every: Duration,
}

impl Default for ReloadTimeouts {
    fn default() -> Self {
        Self {
            drain: Duration::from_secs(60),
            load: Duration::from_secs(300),
            loading_report_every: Duration::from_secs(5),
        }
    }
}

fn elapsed_ms(started: Instant) -> u32 {
    started.elapsed().as_millis().min(u32::MAX as u128) as u32
}

/// Run the whole switch. Returns the load error if the new model is not the
/// one resident at the end.
pub async fn reload<M, D, R>(
    manager: &Mutex<M>,
    generations: &Generations,
    download: D,
    timeouts: ReloadTimeouts,
    mut report: R,
) -> Result<(), String>
where
    M: ModelSlot,
    D: Future<Output = Result<PathBuf, String>>,
    R: FnMut(ReloadEvent),
{
    let path = match download.await {
        Ok(path) => path,
        Err(error) => {
            report(ReloadEvent::Failed {
                error: error.clone(),
            });
            return Err(error);
        }
    };

    report(ReloadEvent::Draining);
    let drained = generations.drain(timeouts.drain).await;
    if drained.cancelled > 0 {
        println!(
            "Drain timed out: cancelled {} generation(s)",
            drained.cancelled
        );
    }

    let result = {
        let mut model = manager.lock().await;
        let previous = model.loaded_path();
        model.unload();

        match timed_load(&mut *model, path, &timeouts, &mut report).await {
            Ok(load_ms) => {
                report(ReloadEvent::Ready { load_ms });
                Ok(())
            }
            Err(error) => {
                report(ReloadEvent::Failed {
                    error: error.clone(),
                });
                if let Some(previous) = previous {
                    restore(&mut *model, previous, timeouts.load, &mut report).await;
                }
                Err(error)
            }
        }
    };

    generations.resume().await;
    result
}

/// Load `path`, re-reporting Loading with the elapsed time until it is done.
async fn timed_load<M: ModelSlot, R: FnMut(ReloadEvent)>(
    model: &mut M,
    path: PathBuf,
    timeouts: &ReloadTimeouts,
    report: &mut R,
) -> Result<u32, String> {
    let started = Instant::now();
    let load = tokio::time::timeout(timeouts.load, model.load(path));
    tokio::pin!(load);

    report(ReloadEvent::Loading { load_ms: 0 });
    let every = timeouts.loading_report_every;
    let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
    loop {
        tokio::select! {
            outcome = &mut load => {
                return match outcome {
                    Ok(Ok(())) => Ok(elapsed_ms(started)),
                    Ok(Err(error)) => Err(error),
                    Err(_) => Err(format!(
                        "Model load timed out after {}s", timeouts.load.as_secs()
                    )),
                };
            }
            _ = ticker.tick() => report(ReloadEvent::Loading { load_ms: elapsed_ms(started) }),
        }
    }
}

async fn restore<M: ModelSlot, R: FnMut(ReloadEvent)>(
    model: &mut M,
    previous: PathBuf,
    load_timeout: Duration,
    report: &mut R,
) {
    let started = Instant::now();
    match tokio::time::timeout(load_timeout, model.load(previous.clone())).await {
        Ok(Ok(())) => report(ReloadEvent::Restored {
            path: previous,
            load_ms: elapsed_ms(started),
        }),
        Ok(Err(error)) => eprintln!("Could not restore {:?} either: {}", previous, error),
        Err(_) => eprintln!("Restoring {:?} timed out", previous),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::oneshot;

    const OLD: &str = "/models/old.gguf";
    const NEW: &str = "/models/new.gguf";

    #[derive(Default)]
    struct FakeModel {
        resident: Option<PathBuf>,
        /// Loads that started while another model was still resident.
        double_resident_loads: usize,
        load_delay: Duration,
        fails: Vec<PathBuf>,
    }

    impl FakeModel {
        fn serving(path: &str) -> Self {
            Self {
                resident: Some(PathBuf::from(path)),
                ..Default::default()
            }
        }
    }

    impl ModelSlot for FakeModel {
        fn loaded_path(&self) -> Option<PathBuf> {
            self.resident.clone()
        }

        fn unload(&mut self) {
            self.resident = None;
        }

        fn load(&mut self, path: PathBuf) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async move {
                if self.resident.is_some() {
                    self.double_resident_loads += 1;
                }
                tokio::time::sleep(self.load_delay).await;
                if self.fails.contains(&path) {
                    return Err(format!("cannot load {:?}", path));
                }
                self.resident = Some(path);
                Ok(())
            })
        }
    }

    type Events = Arc<std::sync::Mutex<Vec<ReloadEvent>>>;

    fn recorder() -> (Events, impl FnMut(ReloadEvent) + Send + 'static) {
        let events: Events = Default::default();
        let sink = events.clone();
        (events, move |event| sink.lock().unwrap().push(event))
    }

    fn fast() -> ReloadTimeouts {
        ReloadTimeouts {
            drain: Duration::from_secs(5),
            load: Duration::from_secs(5),
            loading_report_every: Duration::from_secs(60),
        }
    }

    async fn ready(path: &str) -> Result<PathBuf, String> {
        Ok(PathBuf::from(path))
    }

    fn kinds(events: &Events) -> Vec<&'static str> {
        events
            .lock()
            .unwrap()
            .iter()
            .map(|event| match event {
                ReloadEvent::Draining => "draining",
                ReloadEvent::Loading { .. } => "loading",
                ReloadEvent::Ready { .. } => "ready",
                ReloadEvent::Failed { .. } => "failed",
                ReloadEvent::Restored { .. } => "restored",
            })
            .collect()
    }

    #[tokio::test]
    async fn a_generation_completes_while_a_download_is_in_flight() {
        let manager = Arc::new(Mutex::new(FakeModel::serving(OLD)));
        let generations = Arc::new(Generations::new());
        let (finish_download, download) = oneshot::channel::<Result<PathBuf, String>>();
        let (events, report) = recorder();

        let switch = tokio::spawn({
            let (manager, generations) = (manager.clone(), generations.clone());
            async move {
                reload(
                    &*manager,
                    &generations,
                    async { download.await.unwrap() },
                    fast(),
                    report,
                )
                .await
            }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        // A generation mid-download: accepted, and gets the lock it needs.
        let token = generations
            .begin("r1")
            .await
            .expect("a downloading node accepts work");
        {
            let model = tokio::time::timeout(Duration::from_secs(1), manager.lock())
                .await
                .expect("the download must not hold the model_manager lock");
            assert_eq!(
                model.resident,
                Some(PathBuf::from(OLD)),
                "still serving the old model"
            );
        }
        generations.finish("r1").await;
        assert!(!token.is_cancelled());
        assert!(!switch.is_finished());
        assert!(
            events.lock().unwrap().is_empty(),
            "nothing past downloading yet"
        );

        finish_download.send(Ok(PathBuf::from(NEW))).unwrap();
        switch.await.unwrap().unwrap();

        assert_eq!(manager.lock().await.resident, Some(PathBuf::from(NEW)));
        assert_eq!(kinds(&events), ["draining", "loading", "ready"]);
    }

    #[tokio::test]
    async fn drain_waits_for_in_flight_generations_before_unloading() {
        let manager = Arc::new(Mutex::new(FakeModel::serving(OLD)));
        let generations = Arc::new(Generations::new());
        generations.begin("r1").await.unwrap();
        let (events, report) = recorder();

        let switch = tokio::spawn({
            let (manager, generations) = (manager.clone(), generations.clone());
            async move { reload(&*manager, &generations, ready(NEW), fast(), report).await }
        });
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert_eq!(kinds(&events), ["draining"]);
        assert_eq!(manager.lock().await.resident, Some(PathBuf::from(OLD)));
        assert!(
            generations.begin("r2").await.is_none(),
            "draining stops accepting"
        );

        generations.finish("r1").await;
        switch.await.unwrap().unwrap();

        assert_eq!(kinds(&events), ["draining", "loading", "ready"]);
        assert!(
            generations.begin("r3").await.is_some(),
            "accepting again once ready"
        );
    }

    #[tokio::test]
    async fn the_old_model_is_unloaded_before_the_new_one_loads() {
        let manager = Mutex::new(FakeModel::serving(OLD));
        let (_, report) = recorder();

        reload(&manager, &Generations::new(), ready(NEW), fast(), report)
            .await
            .unwrap();

        let model = manager.lock().await;
        assert_eq!(
            model.double_resident_loads, 0,
            "both models were resident at once"
        );
        assert_eq!(model.resident, Some(PathBuf::from(NEW)));
    }

    #[tokio::test]
    async fn load_ms_is_reported_while_loading_and_on_ready() {
        let manager = Mutex::new(FakeModel {
            load_delay: Duration::from_millis(150),
            ..Default::default()
        });
        let (events, report) = recorder();
        let timeouts = ReloadTimeouts {
            loading_report_every: Duration::from_millis(40),
            ..fast()
        };

        reload(&manager, &Generations::new(), ready(NEW), timeouts, report)
            .await
            .unwrap();

        let events = events.lock().unwrap();
        let loading: Vec<u32> = events
            .iter()
            .filter_map(|event| match event {
                ReloadEvent::Loading { load_ms } => Some(*load_ms),
                _ => None,
            })
            .collect();
        assert_eq!(loading[0], 0, "loading is reported the moment it starts");
        assert!(
            loading.iter().any(|ms| *ms > 0),
            "and again with the elapsed time: {loading:?}"
        );
        assert!(matches!(events.last(), Some(ReloadEvent::Ready { load_ms }) if *load_ms >= 150));
    }

    #[tokio::test]
    async fn a_failed_load_restores_the_previous_model() {
        let manager = Mutex::new(FakeModel {
            fails: vec![PathBuf::from(NEW)],
            ..FakeModel::serving(OLD)
        });
        let generations = Generations::new();
        let (events, report) = recorder();

        let result = reload(&manager, &generations, ready(NEW), fast(), report).await;

        assert!(result.is_err());
        assert_eq!(
            kinds(&events),
            ["draining", "loading", "failed", "restored"]
        );
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(ReloadEvent::Restored { path, .. }) if path == Path::new(OLD)
        ));
        assert_eq!(manager.lock().await.resident, Some(PathBuf::from(OLD)));
        assert!(
            !generations.is_draining().await,
            "back in service on the old model"
        );
    }

    #[tokio::test]
    async fn a_download_error_fails_without_draining_or_unloading() {
        let manager = Mutex::new(FakeModel::serving(OLD));
        let generations = Generations::new();
        let (events, report) = recorder();

        let result = reload(
            &manager,
            &generations,
            async { Err("HTTP error: 404 Not Found".to_string()) },
            fast(),
            report,
        )
        .await;

        assert_eq!(result, Err("HTTP error: 404 Not Found".to_string()));
        assert_eq!(kinds(&events), ["failed"]);
        assert_eq!(manager.lock().await.resident, Some(PathBuf::from(OLD)));
        assert!(!generations.is_draining().await);
    }

    #[tokio::test]
    async fn a_slow_download_does_not_count_against_the_load_timeout() {
        let manager = Mutex::new(FakeModel::default());
        let (_, report) = recorder();
        let timeouts = ReloadTimeouts {
            load: Duration::from_millis(100),
            ..fast()
        };

        let download = async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            Ok(PathBuf::from(NEW))
        };

        reload(&manager, &Generations::new(), download, timeouts, report)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_slow_load_times_out_and_restores_the_previous_model() {
        let manager = Mutex::new(FakeModel {
            load_delay: Duration::from_millis(300),
            ..FakeModel::serving(OLD)
        });
        let (events, report) = recorder();
        let timeouts = ReloadTimeouts {
            load: Duration::from_millis(100),
            ..fast()
        };

        let result = reload(&manager, &Generations::new(), ready(NEW), timeouts, report).await;

        assert!(result.unwrap_err().contains("timed out"));
        // The restore is bounded by the same timeout, so it times out too and
        // leaves the node empty rather than hanging.
        assert_eq!(kinds(&events), ["draining", "loading", "failed"]);
    }
}

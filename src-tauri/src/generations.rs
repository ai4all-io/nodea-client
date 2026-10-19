// In-flight generations, and the gate a model switch closes to drain them.
//
// One cancellation token per generation, keyed by request id, so a
// CancelGeneration from the backend can stop a specific one. Held outside
// ModelManager because a generation holds the model_manager lock for its whole
// run - a cancel that needed that lock could never arrive in time to matter.
//
// The accepting flag lives under the same lock as the map, so "is this node
// draining?" and "register this generation" are one step: a generation can
// never slip in after drain() has looked at the map and decided it was empty.

use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// How often drain() re-checks whether the in-flight generations are gone.
const DRAIN_POLL: Duration = Duration::from_millis(50);

/// How long drain() waits, after cancelling, for cancelled generations to
/// unwind and release the model.
const CANCEL_GRACE: Duration = Duration::from_secs(10);

#[derive(Default)]
struct State {
    draining: bool,
    cancels: HashMap<String, CancellationToken>,
}

#[derive(Default)]
pub struct Generations {
    state: Mutex<State>,
}

/// What drain() had to do to empty the node.
#[derive(Debug, PartialEq, Eq)]
pub struct DrainOutcome {
    /// Generations still running at the timeout, and therefore cancelled.
    pub cancelled: usize,
}

impl Generations {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a generation. None while draining: the node is about to drop
    /// the model the request was routed for.
    pub async fn begin(&self, request_id: &str) -> Option<CancellationToken> {
        let mut state = self.state.lock().await;
        if state.draining {
            return None;
        }
        let token = CancellationToken::new();
        state.cancels.insert(request_id.to_string(), token.clone());
        Some(token)
    }

    pub async fn finish(&self, request_id: &str) {
        self.state.lock().await.cancels.remove(request_id);
    }

    /// Cancel one generation. False when it is not running - normally because
    /// it already finished and the cancel lost the race.
    pub async fn cancel(&self, request_id: &str) -> bool {
        match self.state.lock().await.cancels.get(request_id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    pub async fn in_flight(&self) -> usize {
        self.state.lock().await.cancels.len()
    }

    pub async fn is_draining(&self) -> bool {
        self.state.lock().await.draining
    }

    /// Stop accepting generations, then wait for the in-flight ones to finish.
    /// Any still running after `timeout` are cancelled, and given a grace
    /// period to unwind. The gate stays closed until resume().
    pub async fn drain(&self, timeout: Duration) -> DrainOutcome {
        self.state.lock().await.draining = true;

        if self.wait_until_empty(timeout).await {
            return DrainOutcome { cancelled: 0 };
        }

        let cancelled = {
            let state = self.state.lock().await;
            for token in state.cancels.values() {
                token.cancel();
            }
            state.cancels.len()
        };
        self.wait_until_empty(CANCEL_GRACE).await;
        DrainOutcome { cancelled }
    }

    /// Accept generations again.
    pub async fn resume(&self) {
        self.state.lock().await.draining = false;
    }

    async fn wait_until_empty(&self, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if self.state.lock().await.cancels.is_empty() {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(DRAIN_POLL).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn begin_registers_and_finish_removes() {
        let generations = Generations::new();
        assert!(generations.begin("r1").await.is_some());
        assert_eq!(generations.in_flight().await, 1);

        generations.finish("r1").await;
        assert_eq!(generations.in_flight().await, 0);
    }

    #[tokio::test]
    async fn cancel_reaches_the_running_generation_only() {
        let generations = Generations::new();
        let token = generations.begin("r1").await.unwrap();

        assert!(generations.cancel("r1").await);
        assert!(token.is_cancelled());
        assert!(!generations.cancel("gone").await);
    }

    #[tokio::test]
    async fn drain_waits_for_in_flight_generations() {
        let generations = Arc::new(Generations::new());
        let token = generations.begin("r1").await.unwrap();

        let drain = tokio::spawn({
            let generations = generations.clone();
            async move { generations.drain(Duration::from_secs(5)).await }
        });

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            !drain.is_finished(),
            "drain returned with a generation still running"
        );

        generations.finish("r1").await;
        let outcome = drain.await.unwrap();
        assert_eq!(outcome, DrainOutcome { cancelled: 0 });
        assert!(
            !token.is_cancelled(),
            "a generation that finished in time is not cancelled"
        );
    }

    #[tokio::test]
    async fn a_draining_node_accepts_no_new_generations() {
        let generations = Generations::new();
        generations.drain(Duration::from_millis(10)).await;

        assert!(generations.is_draining().await);
        assert!(generations.begin("r2").await.is_none());

        generations.resume().await;
        assert!(generations.begin("r2").await.is_some());
    }

    #[tokio::test]
    async fn drain_cancels_generations_that_outlive_the_timeout() {
        let generations = Arc::new(Generations::new());
        let token = generations.begin("stuck").await.unwrap();

        // A generation that stops as soon as it is cancelled, as the real one does.
        tokio::spawn({
            let generations = generations.clone();
            let token = token.clone();
            async move {
                token.cancelled().await;
                generations.finish("stuck").await;
            }
        });

        let outcome = generations.drain(Duration::from_millis(100)).await;

        assert_eq!(outcome, DrainOutcome { cancelled: 1 });
        assert!(token.is_cancelled());
        assert_eq!(generations.in_flight().await, 0);
    }
}

//! Reconnect policy for the gRPC stream.
//!
//! Split out from `grpc_client` so it can be tested: the connection loop needs
//! a `tauri::AppHandle`, which cannot be built outside a running app, while the
//! decisions it makes between attempts are plain functions of an outcome. The
//! bugs this replaced were all in those decisions, not in the plumbing.

use tonic::Status;

// Retry configuration constants.
//
// Retries are unbounded. A bounded count with a per-call counter meant a laptop
// that slept overnight gave up after ~30s and sat idle until the user clicked
// Retry - and a backend deploy, which closes every stream at once, did the same
// thing to every device at once. With the cap below, an unbounded loop
// reconnects within a minute of the machine waking or the server returning,
// which is also why no wake-from-sleep hook is needed.
pub const INITIAL_RETRY_DELAY_SECS: u64 = 2;
pub const MAX_RETRY_DELAY_SECS: u64 = 60;

// Keepalive. The server pings on the same schedule (see `serve_grpc`), so a
// half-open TCP connection is detected from both ends rather than parking
// forever in a read that will never complete.
pub const KEEPALIVE_INTERVAL_SECS: u64 = 20;
pub const KEEPALIVE_TIMEOUT_SECS: u64 = 10;

/// Why a connection attempt ended, and whether another one could do better.
#[derive(Debug)]
pub enum ConnectError {
    /// Worth another attempt: a network blip, a server restart, a deploy.
    Retryable(String),
    /// Retrying cannot change the outcome: the org's plan, say.
    Terminal(String),
    /// The server refused the credentials. Terminal with the token in hand -
    /// but a *refreshed* token is a different credential, so the retry loop
    /// gets one chance to fetch one before giving up. Access tokens are
    /// short-lived, so a node that was offline longer than one lifetime hits
    /// this on every reconnect and would otherwise never come back.
    Unauthenticated(String),
}

impl ConnectError {
    pub fn message(&self) -> &str {
        match self {
            ConnectError::Retryable(m)
            | ConnectError::Terminal(m)
            | ConnectError::Unauthenticated(m) => m,
        }
    }

    /// Whether another attempt with what we have could do better. Refused
    /// credentials count as terminal: they only stop being so once a refresh
    /// has actually produced a new token, which is the caller's business.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ConnectError::Terminal(_) | ConnectError::Unauthenticated(_)
        )
    }

    /// Worth refreshing the token and trying once more.
    pub fn is_unauthenticated(&self) -> bool {
        matches!(self, ConnectError::Unauthenticated(_))
    }
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for ConnectError {}

/// A stream_events rejection is a device-limit error, not a transient one, when
/// the server used RESOURCE_EXHAUSTED - retrying it won't change the org's plan.
pub fn is_device_limit_error(status: &Status) -> bool {
    status.code() == tonic::Code::ResourceExhausted
}

/// Credentials the server refused. A new attempt would present the same ones.
pub fn is_auth_error(status: &Status) -> bool {
    matches!(
        status.code(),
        tonic::Code::Unauthenticated | tonic::Code::PermissionDenied
    )
}

/// How long to wait before attempt number `attempt` (1-based, so the delay
/// after the first failure is INITIAL_RETRY_DELAY_SECS).
///
/// Doubles until it reaches the cap and stays there. The shift is saturating
/// because the loop is unbounded now: a device left offline for a day reaches
/// attempt numbers that would overflow `2u64.pow(attempt - 1)`.
pub fn retry_delay_secs(attempt: u32) -> u64 {
    let factor = 1u64
        .checked_shl(attempt.saturating_sub(1))
        .unwrap_or(u64::MAX);

    INITIAL_RETRY_DELAY_SECS
        .saturating_mul(factor)
        .min(MAX_RETRY_DELAY_SECS)
}

/// The attempt number to count a failed attempt as, for the backoff.
///
/// Failures only count while they are consecutive. A connection that was
/// established and then closed - a deploy, a revoked or superseded session -
/// proves the server was reachable moments ago, so it starts a fresh backoff
/// instead of inheriting every earlier attempt's: without this, a device that
/// had lived through a few deploys waited the full cap after each one (#241).
pub fn attempt_after_failure(attempt: u32, was_established: bool) -> u32 {
    if was_established {
        1
    } else {
        attempt
    }
}

/// What to do when the server closes the stream cleanly.
///
/// A graceful close used to return `Ok(())`, which `connect_with_retry` reads
/// as success - so a backend deploy left every device idle until someone
/// clicked Retry. It is only really "done" when the user asked to disconnect.
pub fn graceful_close_outcome(cancelled: bool) -> Result<(), ConnectError> {
    if cancelled {
        return Ok(());
    }

    Err(ConnectError::Retryable(
        "Server closed the connection".to_string(),
    ))
}

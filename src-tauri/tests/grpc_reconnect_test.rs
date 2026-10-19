// Reconnect policy for the desktop client.
//
// These cover the decisions `connect_with_retry` makes between attempts. They
// are unit tests of the policy rather than of a live stream on purpose: the
// connection loop needs a `tauri::AppHandle`, which cannot be built outside a
// running app, so driving it end to end here would test the harness. The
// behaviour that used to be wrong was never in the plumbing - it was in what
// the loop concluded from an outcome.
//
// Heartbeat behaviour is covered where it is decided rather than observed: the
// client's cadence is a 5s `interval` with no branch to test, and what the
// server does with a ping - stamping, throttling the write, and reaping a
// device that stops - is asserted in
// backend/tests/integration/test_connection_liveness.py against a real
// stream. The reconnect decisions a missed heartbeat leads to are below.

use ai4all_tauri::reconnect::{
    attempt_after_failure, graceful_close_outcome, is_auth_error, is_device_limit_error,
    retry_delay_secs, ConnectError,
};
use tonic::{Code, Status};

#[test]
fn backoff_doubles_from_two_seconds() {
    assert_eq!(retry_delay_secs(1), 2);
    assert_eq!(retry_delay_secs(2), 4);
    assert_eq!(retry_delay_secs(3), 8);
    assert_eq!(retry_delay_secs(4), 16);
    assert_eq!(retry_delay_secs(5), 32);
}

#[test]
fn backoff_caps_at_sixty_seconds() {
    assert_eq!(retry_delay_secs(6), 60);
    assert_eq!(retry_delay_secs(7), 60);
    assert_eq!(retry_delay_secs(100), 60);
}

#[test]
fn backoff_survives_an_overnight_sleep() {
    // Retries are unbounded now. A laptop left offline reaches attempt numbers
    // that overflow a naive `2u64.pow(attempt - 1)`, and the retry loop must
    // not panic its way out of reconnecting.
    assert_eq!(retry_delay_secs(u32::MAX), 60);
    assert_eq!(retry_delay_secs(1000), 60);
}

#[test]
fn a_graceful_close_is_retried() {
    // A deploy closes every stream cleanly. This used to return Ok(()), which
    // the retry loop read as success, leaving the device idle until the user
    // clicked Retry.
    let outcome = graceful_close_outcome(false);

    let error = outcome.expect_err("a server-side close should be retried");
    assert!(!error.is_terminal());
    assert!(error.message().contains("Server closed"));
}

#[test]
fn a_graceful_close_after_disconnecting_is_not_retried() {
    // The user asked for this one, so reconnecting would fight them.
    assert!(graceful_close_outcome(true).is_ok());
}

#[test]
fn a_device_limit_rejection_is_recognised() {
    let status = Status::new(Code::ResourceExhausted, "device limit");

    assert!(is_device_limit_error(&status));
    assert!(!is_auth_error(&status));
}

#[test]
fn refused_credentials_are_recognised() {
    assert!(is_auth_error(&Status::new(
        Code::Unauthenticated,
        "bad token"
    )));
    assert!(is_auth_error(&Status::new(Code::PermissionDenied, "nope")));
}

#[test]
fn a_transient_status_is_neither() {
    let status = Status::new(Code::Unavailable, "server restarting");

    assert!(!is_device_limit_error(&status));
    assert!(!is_auth_error(&status));
}

#[test]
fn connect_errors_carry_their_message_and_verdict() {
    let retryable = ConnectError::Retryable("network".to_string());
    let terminal = ConnectError::Terminal("plan limit".to_string());

    assert!(!retryable.is_terminal());
    assert_eq!(retryable.message(), "network");
    assert_eq!(format!("{}", retryable), "network");

    assert!(terminal.is_terminal());
    assert_eq!(terminal.message(), "plan limit");
    assert!(
        !terminal.is_unauthenticated(),
        "a plan limit is not a credential"
    );
}

#[test]
fn refused_credentials_are_terminal_but_worth_a_refresh_first() {
    // The distinction the retry loop acts on. Access tokens are short-lived,
    // so a node offline for longer than one lifetime is refused on every
    // reconnect; it used to retry the same dead token until the 60s backoff
    // cap, forever. `is_terminal` stays true so that a refresh which fails
    // behaves exactly as before - the user has to sign in.
    let refused = ConnectError::Unauthenticated("expired".to_string());

    assert!(
        refused.is_unauthenticated(),
        "the loop refreshes and retries once"
    );
    assert!(
        refused.is_terminal(),
        "but gives up if the refresh does not help"
    );
    assert_eq!(refused.message(), "expired");
    assert_eq!(format!("{}", refused), "expired");
}

#[test]
fn a_connection_that_was_established_starts_a_fresh_backoff() {
    // The 6th consecutive failure would wait the 60s cap; a close after a
    // successful connection (a deploy, a revoked session) waits the initial 2s.
    assert_eq!(retry_delay_secs(attempt_after_failure(6, false)), 60);
    assert_eq!(retry_delay_secs(attempt_after_failure(6, true)), 2);
}

#[test]
fn failures_that_never_connected_keep_growing_the_backoff() {
    let mut attempt = 0;
    let mut delays = Vec::new();
    for _ in 0..4 {
        attempt += 1;
        attempt = attempt_after_failure(attempt, false);
        delays.push(retry_delay_secs(attempt));
    }
    assert_eq!(delays, vec![2, 4, 8, 16]);
}

#[test]
fn a_connect_close_cycle_never_climbs_to_the_cap() {
    // What the desktop saw after leaving an org: connect, get closed, reconnect,
    // get closed again. Each cycle had connected, so each waits the initial delay.
    let mut attempt = 0;
    for _ in 0..6 {
        attempt += 1;
        attempt = attempt_after_failure(attempt, true);
        assert_eq!(retry_delay_secs(attempt), 2);
    }
}

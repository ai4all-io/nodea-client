use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::timeout;

// Load Supabase configuration from environment variables (set at compile time)
const SUPABASE_URL: &str = env!("SUPABASE_URL");
const SUPABASE_ANON_KEY: &str = env!("SUPABASE_ANON_KEY");
// Same REST API base URL the frontend's ApiClient uses (VITE_API_BASE_URL),
// forwarded to compile time by build.rs.
const BACKEND_URL: &str = env!("VITE_API_BASE_URL");

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthResult {
    pub access_token: String,
    pub refresh_token: String,
    pub user: User,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
}

#[derive(Debug, Deserialize)]
struct SupabaseAuthResponse {
    access_token: String,
    #[allow(dead_code)]
    token_type: String,
    #[allow(dead_code)]
    expires_in: u64,
    refresh_token: String,
    user: User,
}

#[derive(Debug, Serialize)]
struct EmailPasswordRequest {
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct SignupResponse {
    message: String,
    #[allow(dead_code)]
    email: String,
}

/// Authenticate with email and password
pub async fn authenticate_email_password(
    email: String,
    password: String,
) -> Result<AuthResult, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();

    let response = client
        .post(format!(
            "{}/auth/v1/token?grant_type=password",
            SUPABASE_URL
        ))
        .header("apikey", SUPABASE_ANON_KEY)
        .header("Content-Type", "application/json")
        .json(&EmailPasswordRequest { email, password })
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(format!("Authentication failed: {}", error_text).into());
    }

    let auth_response = response.json::<SupabaseAuthResponse>().await?;

    Ok(AuthResult {
        access_token: auth_response.access_token,
        refresh_token: auth_response.refresh_token,
        user: auth_response.user,
    })
}

/// Where the browser finally hands sign-in back to the app.
///
/// The scheme is the one already registered with the OS for invitation and
/// confirm-email links (`DEEP_LINK_SCHEME` in main.rs), which is why the app
/// needs no loopback listener: the OS hands the whole URL - fragment included -
/// straight to the running app.
pub const OAUTH_REDIRECT_URI: &str = "nodea://auth/callback";

/// Where *Supabase* redirects to, which is not the same thing.
///
/// A custom-scheme redirect never commits a document, so pointing Supabase at
/// `nodea://` directly leaves the browser tab spinning on the Google page for
/// as long as the user tolerates it, even though sign-in succeeded. The
/// backend serves a page at this path that forwards to the deep link and says
/// the sign-in is done (see backend/lib/desktop_callback.py).
///
/// Derived from the API base URL rather than configured separately, so it is
/// automatically right for whichever backend this build points at - but that
/// URL must be on the project's redirect allowlist, or Supabase silently
/// substitutes site_url.
fn supabase_redirect_url() -> String {
    format!(
        "{}/v1/auth/desktop-callback",
        BACKEND_URL.trim_end_matches('/')
    )
}

/// How long to wait for the user to finish in the browser before giving up.
const OAUTH_TIMEOUT: Duration = Duration::from_secs(300);

/// The sign-in attempt currently waiting for a deep link, if any. A sign-in
/// starts in one thread (the Tauri command) and finishes in another (the OS
/// delivering the URL), so the two have to meet somewhere.
///
/// Tagged with an id because attempts overlap: a user who gives up, clicks
/// again and then finishes has two attempts in flight, and the first one's
/// timeout must not clear the second one's sender.
type PendingAttempt = Option<(u64, oneshot::Sender<String>)>;

fn pending_callback() -> &'static Mutex<PendingAttempt> {
    static PENDING: OnceLock<Mutex<PendingAttempt>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(None))
}

fn next_attempt_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Clear the pending attempt, but only if it is still the one we registered.
fn abandon_attempt(id: u64) {
    let mut pending = pending_callback()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if matches!(*pending, Some((pending_id, _)) if pending_id == id) {
        *pending = None;
    }
}

/// Whether a deep link is an OAuth callback rather than an invitation or
/// confirm-email link, which the frontend router handles instead.
pub fn is_oauth_callback(url: &str) -> bool {
    url.starts_with(OAUTH_REDIRECT_URI)
}

/// Hands a deep link to the sign-in waiting for it.
///
/// Returns false when this is not an OAuth callback, or when nothing is
/// waiting for one, so the caller can forward the link to the frontend as
/// usual - a stale callback should not be mistaken for an invitation.
pub fn deliver_oauth_callback(url: &str) -> bool {
    if !is_oauth_callback(url) {
        return false;
    }

    let sender = pending_callback()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();

    match sender {
        Some((_, tx)) => tx.send(url.to_string()).is_ok(),
        None => false,
    }
}

/// A PKCE code verifier: 43-128 unreserved characters, per RFC 7636.
fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Its S256 challenge - the only thing that travels through the browser, so an
/// intercepted authorization code is useless without the verifier this process
/// kept in memory.
fn code_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn build_authorize_url(
    supabase_url: &str,
    provider: &str,
    redirect_to: &str,
    challenge: &str,
) -> String {
    format!(
        "{}/auth/v1/authorize?provider={}&redirect_to={}&code_challenge={}&code_challenge_method=s256",
        supabase_url.trim_end_matches('/'),
        urlencoding::encode(provider),
        urlencoding::encode(redirect_to),
        urlencoding::encode(challenge),
    )
}

/// What came back on the deep link.
#[derive(Debug, PartialEq)]
pub enum Callback {
    /// PKCE: an authorization code still to be exchanged for a session.
    Code(String),
    /// The implicit flow's tokens, already in the URL fragment. Supabase uses
    /// this shape when a code challenge is absent, and it costs little to keep
    /// accepting it.
    Tokens {
        access_token: String,
        refresh_token: String,
    },
}

/// Reads a callback's parameters, which Supabase puts in the query for PKCE
/// and in the fragment for the implicit flow - so both are searched.
pub fn parse_callback(url: &str) -> Result<Callback, String> {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let params: HashMap<String, String> = after_scheme
        .split(['?', '#'])
        .skip(1)
        .flat_map(|section| section.split('&'))
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            let value = urlencoding::decode(value).ok()?.into_owned();
            Some((key.to_string(), value))
        })
        .collect();

    if let Some(error) = params
        .get("error_description")
        .or_else(|| params.get("error"))
    {
        // Form encoding, so spaces may have arrived as '+' - and this string is
        // shown to the user. Codes and tokens are base64url and never contain
        // '+', so only the message is touched.
        return Err(error.replace('+', " "));
    }

    if let Some(code) = params.get("code") {
        return Ok(Callback::Code(code.clone()));
    }

    if let Some(access_token) = params.get("access_token") {
        return Ok(Callback::Tokens {
            access_token: access_token.clone(),
            refresh_token: params.get("refresh_token").cloned().unwrap_or_default(),
        });
    }

    Err("Sign-in was cancelled or returned no credentials".to_string())
}

#[derive(Debug, Serialize)]
struct PkceExchangeRequest<'a> {
    auth_code: &'a str,
    code_verifier: &'a str,
}

/// Authenticate with an OAuth provider (Google, GitHub, etc.).
///
/// The browser does the talking; Supabase redirects to the backend's hand-off
/// page, which forwards to `OAUTH_REDIRECT_URI`, and the OS delivers that back
/// to this process. Using the deep link rather than a loopback listener avoids
/// binding a port the backend may already hold, and avoids the fragment
/// problem: a URL fragment is never sent to a server, so a local HTTP listener
/// cannot read the one Supabase returns.
pub async fn authenticate_oauth(provider: &str) -> Result<AuthResult, Box<dyn std::error::Error>> {
    let verifier = generate_code_verifier();
    let auth_url = build_authorize_url(
        SUPABASE_URL,
        provider,
        &supabase_redirect_url(),
        &code_challenge(&verifier),
    );

    // Registered before the browser opens, so a callback that arrives while
    // this thread is still starting up is not dropped.
    let attempt = next_attempt_id();
    let (tx, rx) = oneshot::channel();
    *pending_callback()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((attempt, tx));

    println!("Opening browser for {} authentication...", provider);
    if let Err(e) = open::that(&auth_url) {
        abandon_attempt(attempt);
        return Err(format!("Could not open a browser for sign-in: {}", e).into());
    }

    let callback_url = match timeout(OAUTH_TIMEOUT, rx).await {
        Ok(Ok(url)) => url,
        Ok(Err(_)) => return Err("Sign-in was superseded by another attempt".into()),
        Err(_) => {
            abandon_attempt(attempt);
            return Err("Timed out waiting for the browser to complete sign-in".into());
        }
    };

    match parse_callback(&callback_url)? {
        Callback::Code(code) => exchange_code_for_session(&code, &verifier).await,
        Callback::Tokens {
            access_token,
            refresh_token,
        } => {
            let user = fetch_user(&access_token).await?;
            Ok(AuthResult {
                access_token,
                refresh_token,
                user,
            })
        }
    }
}

/// Trades the authorization code for a session. The verifier never left this
/// process, which is what proves the exchange belongs to the sign-in that
/// started it.
async fn exchange_code_for_session(
    code: &str,
    verifier: &str,
) -> Result<AuthResult, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/auth/v1/token?grant_type=pkce", SUPABASE_URL))
        .header("apikey", SUPABASE_ANON_KEY)
        .header("Content-Type", "application/json")
        .json(&PkceExchangeRequest {
            auth_code: code,
            code_verifier: verifier,
        })
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(format!("Sign-in could not be completed: {}", error_text).into());
    }

    let auth_response = response.json::<SupabaseAuthResponse>().await?;

    Ok(AuthResult {
        access_token: auth_response.access_token,
        refresh_token: auth_response.refresh_token,
        user: auth_response.user,
    })
}

async fn fetch_user(access_token: &str) -> Result<User, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/auth/v1/user", SUPABASE_URL))
        .header("apikey", SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await?;

    Ok(response.json::<User>().await?)
}

/// Sign up a new user with email and password.
///
/// Routes through our own backend rather than Supabase's `/auth/v1/signup`
/// directly, so the desktop app gets the same server-side validation and
/// Admin-API-based (unconfirmed) account creation as the web app - our own
/// verification email is what confirms the account, not Supabase's mailer.
/// There is no session to return here: signup only succeeds once the
/// emailed confirmation link is clicked.
pub async fn sign_up_email_password(
    email: String,
    password: String,
) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/v1/signup", BACKEND_URL))
        .header("Content-Type", "application/json")
        .json(&EmailPasswordRequest { email, password })
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(format!("Sign up failed: {}", error_text).into());
    }

    let signup_response = response.json::<SignupResponse>().await?;

    Ok(signup_response.message)
}

/// Refresh access token using refresh token
pub async fn refresh_access_token(refresh_token: String) -> Result<AuthResult, String> {
    let client = reqwest::Client::new();

    #[derive(Serialize)]
    struct RefreshTokenRequest {
        refresh_token: String,
    }

    let response = client
        .post(format!(
            "{}/auth/v1/token?grant_type=refresh_token",
            SUPABASE_URL
        ))
        .header("apikey", SUPABASE_ANON_KEY)
        .header("Content-Type", "application/json")
        .json(&RefreshTokenRequest { refresh_token })
        .send()
        .await
        .map_err(|e| format!("Failed to send refresh request: {}", e))?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("Token refresh failed: {}", error_text));
    }

    let auth_response = response
        .json::<SupabaseAuthResponse>()
        .await
        .map_err(|e| format!("Failed to parse refresh response: {}", e))?;

    Ok(AuthResult {
        access_token: auth_response.access_token,
        refresh_token: auth_response.refresh_token,
        user: auth_response.user,
    })
}

/// Validate access token by fetching user info
pub async fn validate_access_token(access_token: String) -> Result<User, String> {
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/auth/v1/user", SUPABASE_URL))
        .header("apikey", SUPABASE_ANON_KEY)
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await
        .map_err(|e| format!("Failed to send validation request: {}", e))?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("Token validation failed: {}", error_text));
    }

    let user = response
        .json::<User>()
        .await
        .map_err(|e| format!("Failed to parse user response: {}", e))?;
    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pending attempt is process-wide state and cargo runs tests in
    /// parallel threads, so the tests that touch it take turns.
    fn pending_state_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn authorize_url_carries_the_redirect_and_a_challenge() {
        let url = build_authorize_url(
            "https://ref.supabase.co/",
            "google",
            "https://api.example.com/site/v1/auth/desktop-callback",
            "chal+lenge",
        );

        assert!(url.starts_with("https://ref.supabase.co/auth/v1/authorize?provider=google"));
        // Encoded, or Supabase reads the redirect as ending at the first colon.
        assert!(url.contains(
            "redirect_to=https%3A%2F%2Fapi.example.com%2Fsite%2Fv1%2Fauth%2Fdesktop-callback"
        ));
        assert!(url.contains("code_challenge=chal%2Blenge"));
        assert!(url.contains("code_challenge_method=s256"));
    }

    #[test]
    fn supabase_redirects_to_the_backend_this_build_points_at() {
        // Not a separate setting: a build talking to the local API must not
        // send Supabase to the production hand-off page, or the code lands in
        // the wrong browser tab and the app waits forever.
        let url = supabase_redirect_url();

        assert!(url.starts_with(BACKEND_URL.trim_end_matches('/')));
        assert!(url.ends_with("/v1/auth/desktop-callback"));
        assert!(!url.contains("//v1/"));
    }

    #[test]
    fn challenge_is_the_url_safe_sha256_of_the_verifier() {
        // RFC 7636 appendix B's worked example.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

        assert_eq!(
            code_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifier_is_long_enough_and_unpredictable() {
        let first = generate_code_verifier();

        assert!((43..=128).contains(&first.len()));
        assert_ne!(first, generate_code_verifier());
    }

    #[test]
    fn reads_a_pkce_code_from_the_query() {
        assert_eq!(
            parse_callback("nodea://auth/callback?code=abc123"),
            Ok(Callback::Code("abc123".to_string()))
        );
    }

    #[test]
    fn reads_implicit_tokens_from_the_fragment() {
        // The shape a loopback listener could never see: a fragment is never
        // sent to a server, but the OS hands the app the whole URL.
        assert_eq!(
            parse_callback(
                "nodea://auth/callback#access_token=at&refresh_token=rt&expires_in=3600"
            ),
            Ok(Callback::Tokens {
                access_token: "at".to_string(),
                refresh_token: "rt".to_string(),
            })
        );
    }

    #[test]
    fn surfaces_a_refused_consent_rather_than_a_missing_code() {
        assert_eq!(
            parse_callback("nodea://auth/callback?error=access_denied&error_description=User%20denied%20access"),
            Err("User denied access".to_string())
        );
    }

    #[test]
    fn rejects_a_callback_with_nothing_in_it() {
        assert!(parse_callback("nodea://auth/callback").is_err());
    }

    #[test]
    fn only_the_auth_path_is_an_oauth_callback() {
        assert!(is_oauth_callback("nodea://auth/callback?code=abc"));
        // Invitation and confirm-email links must still reach the frontend.
        assert!(!is_oauth_callback("nodea://invitation/some-token"));
        assert!(!is_oauth_callback("https://app.getnodea.com/auth/callback"));
    }

    // The guard only serialises these tests against each other; holding it
    // across the await is the point, and the runtime is single-threaded.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn an_abandoned_attempt_does_not_disarm_the_one_that_replaced_it() {
        let _guard = pending_state_guard();

        // A user who gives up, clicks again, and then finishes: the first
        // attempt's timeout fires while the second is waiting for its callback.
        let first = next_attempt_id();
        let (tx_first, _rx_first) = oneshot::channel();
        *pending_callback().lock().unwrap() = Some((first, tx_first));

        let second = next_attempt_id();
        let (tx_second, rx_second) = oneshot::channel();
        *pending_callback().lock().unwrap() = Some((second, tx_second));

        abandon_attempt(first);

        assert!(deliver_oauth_callback("nodea://auth/callback?code=abc"));
        assert_eq!(rx_second.await.unwrap(), "nodea://auth/callback?code=abc");
    }

    #[test]
    fn a_refused_consent_reads_as_a_sentence() {
        assert_eq!(
            parse_callback(
                "nodea://auth/callback?error=access_denied&error_description=User+denied+access"
            ),
            Err("User denied access".to_string())
        );
    }

    // The guard only serialises these tests against each other; holding it
    // across the await is the point, and the runtime is single-threaded.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn a_deep_link_reaches_the_sign_in_waiting_for_it() {
        let _guard = pending_state_guard();

        // Nothing waiting: the link belongs to the frontend, not to us.
        assert!(!deliver_oauth_callback("nodea://auth/callback?code=abc"));

        let (tx, rx) = oneshot::channel();
        *pending_callback().lock().unwrap() = Some((next_attempt_id(), tx));

        // An invitation link must not consume a sign-in that is still waiting.
        assert!(!deliver_oauth_callback("nodea://invitation/token"));

        assert!(deliver_oauth_callback("nodea://auth/callback?code=abc"));
        assert_eq!(rx.await.unwrap(), "nodea://auth/callback?code=abc");

        // Taken, not merely read: a second delivery resolves nothing.
        assert!(!deliver_oauth_callback("nodea://auth/callback?code=abc"));
    }
}

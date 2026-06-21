//! HTTP listener and routes (`/v1/wallet/state`, `/v1/wallet/command`).

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{middleware, Json, Router};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use super::auth::require_api_auth;
use super::state::{save_session_state, validate_session_state, WalletSessionState};
use super::TuiCommandResponse;

/// A unit of wallet work executed on the TUI `LocalSet` (the wallet is `!Send`). Typed so the same
/// endpoints a web UI calls drive the composite flows that need the wallet/snapshot.
#[derive(Debug)]
pub(crate) enum TuiApiRequest {
    /// Generic command from clap `argv` tokens.
    Command(Vec<String>),
    /// Planner preview for a simple send (no kernel poke, no file written).
    PlanSimpleSend {
        recipient: String,
        amount_nicks: u64,
    },
    /// Build + broadcast a simple send (create-tx then send-tx).
    CreateAndSendSimple {
        recipient: String,
        amount_nicks: u64,
    },
    /// Register a `.nock` name: registry-payment create-tx then send-tx.
    NnsRegister { name: String },
}

#[derive(Debug)]
pub(crate) struct TuiApiJob {
    pub request: TuiApiRequest,
    pub resp: oneshot::Sender<TuiCommandResponse>,
}

#[derive(Clone)]
struct HttpState {
    jobs: mpsc::Sender<TuiApiJob>,
    session: Arc<RwLock<WalletSessionState>>,
    session_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CommandRequest {
    argv: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SimpleSendRequest {
    recipient: String,
    amount_nicks: u64,
}

#[derive(Debug, Deserialize)]
struct NnsRequest {
    name: String,
}

/// Handle to stop the background HTTP listener (e.g. when Settings changes the bind address).
pub(crate) struct ApiServerHandle {
    shutdown: Option<oneshot::Sender<()>>,
    join: Option<JoinHandle<()>>,
}

impl ApiServerHandle {
    pub(crate) fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Spawn the HTTP server on a background thread.
pub(crate) fn spawn_http_server(
    listen: String,
    job_tx: mpsc::Sender<TuiApiJob>,
    session_path: PathBuf,
    session: Arc<RwLock<WalletSessionState>>,
    api_auth_token: Arc<str>,
) -> Result<ApiServerHandle, String> {
    let addr: SocketAddr = listen
        .parse()
        .map_err(|e| format!("Invalid API listen address '{listen}': {e}"))?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let join = thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                warn!("wallet API runtime failed: {e}");
                return;
            }
        };
        rt.block_on(async move {
            let state = HttpState {
                jobs: job_tx,
                session,
                session_path,
            };
            let auth_token = Arc::clone(&api_auth_token);
            let protected = Router::new()
                .route("/v1/wallet/state", get(get_session).post(post_session))
                .route("/v1/wallet/command", post(run_command))
                .route("/v1/wallet/tx/plan", post(post_plan))
                .route("/v1/wallet/tx/create-and-send", post(post_create_and_send))
                .route("/v1/wallet/nns/register", post(post_nns_register))
                .route_layer(middleware::from_fn_with_state(auth_token, require_api_auth))
                .with_state(state.clone());

            let app = Router::new()
                .route("/health", get(health))
                .merge(protected)
                .with_state(state);

            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    warn!(%addr, "wallet API bind failed: {e}");
                    return;
                }
            };
            info!(%addr, "wallet JSON API listening (TUI session)");
            tokio::select! {
                r = axum::serve(listener, app).into_future() => {
                    if let Err(e) = r {
                        warn!("wallet API server stopped: {e}");
                    }
                }
                _ = shutdown_rx => {
                    info!("wallet API stopping (TUI exit)");
                }
            }
        });
    });

    Ok(ApiServerHandle {
        shutdown: Some(shutdown_tx),
        join: Some(join),
    })
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "schema_version": super::TUI_OUTCOME_SCHEMA,
        "session_schema_version": super::WALLET_SESSION_SCHEMA,
        // Responses carry typed `events` plus normalized `reports` (markdown commands without a
        // structured event yet). `report_section_types` describes the `reports[].sections[].type`.
        "wallet_event_kinds": [
            "balance_snapshot_v1",
            "notes_list_v1",
            "address_list_v1",
            "key_tree_v1",
            "keygen_v1",
            "migrate_summary_v1",
            "tx_accepted_v1",
            "nns_registration_v1",
            "create_tx_v1"
        ],
        "report_section_types": ["heading", "text", "key_value", "table", "raw"],
    }))
}

async fn get_session(State(ctx): State<HttpState>) -> Json<WalletSessionState> {
    Json(ctx.session.read().unwrap().clone())
}

async fn post_session(
    State(ctx): State<HttpState>,
    Json(body): Json<WalletSessionState>,
) -> Result<Json<WalletSessionState>, (StatusCode, String)> {
    validate_session_state(&body).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    save_session_state(&ctx.session_path, &body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    *ctx.session.write().unwrap() = body.clone();
    Ok(Json(body))
}

/// Hand a request to the TUI executor (on the `LocalSet`) and await its response.
async fn dispatch_job(
    state: &HttpState,
    request: TuiApiRequest,
) -> (StatusCode, Json<TuiCommandResponse>) {
    let (resp_tx, resp_rx) = oneshot::channel();
    let job = TuiApiJob {
        request,
        resp: resp_tx,
    };
    if state.jobs.send(job).await.is_err() {
        return server_error("TUI wallet executor unavailable (is the TUI running?)");
    }
    match resp_rx.await {
        Ok(json) => {
            let status = if json.is_success() {
                StatusCode::OK
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            (status, Json(json))
        }
        Err(_) => server_error("TUI wallet executor dropped response"),
    }
}

async fn run_command(
    State(state): State<HttpState>,
    Json(body): Json<CommandRequest>,
) -> (StatusCode, Json<TuiCommandResponse>) {
    if body.argv.is_empty() {
        return bad_request("argv must contain at least one command token".into());
    }
    dispatch_job(&state, TuiApiRequest::Command(body.argv)).await
}

async fn post_plan(
    State(state): State<HttpState>,
    Json(body): Json<SimpleSendRequest>,
) -> (StatusCode, Json<TuiCommandResponse>) {
    dispatch_job(
        &state,
        TuiApiRequest::PlanSimpleSend {
            recipient: body.recipient,
            amount_nicks: body.amount_nicks,
        },
    )
    .await
}

async fn post_create_and_send(
    State(state): State<HttpState>,
    Json(body): Json<SimpleSendRequest>,
) -> (StatusCode, Json<TuiCommandResponse>) {
    dispatch_job(
        &state,
        TuiApiRequest::CreateAndSendSimple {
            recipient: body.recipient,
            amount_nicks: body.amount_nicks,
        },
    )
    .await
}

async fn post_nns_register(
    State(state): State<HttpState>,
    Json(body): Json<NnsRequest>,
) -> (StatusCode, Json<TuiCommandResponse>) {
    dispatch_job(&state, TuiApiRequest::NnsRegister { name: body.name }).await
}

fn bad_request(msg: String) -> (StatusCode, Json<TuiCommandResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(TuiCommandResponse::failure(msg)),
    )
}

fn server_error(msg: &str) -> (StatusCode, Json<TuiCommandResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(TuiCommandResponse::failure(msg)),
    )
}

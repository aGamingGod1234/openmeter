//! Loopback-only, read-only OpenUsage-compatible HTTP API.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};
use tiny_http::Method;

use crate::accounts::AccountRegistry;
use crate::contracts::{serialize_limits_with_registry, serialize_usage_with_registry};
use crate::providers::ProviderSnapshot;

const DEFAULT_MAX_IN_FLIGHT: usize = 16;

#[derive(Debug, Clone)]
pub struct RouteResponse {
    pub status: u16,
    pub body: Value,
    pub headers: Vec<(String, String)>,
}

impl RouteResponse {
    fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            body,
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
        }
    }
}

#[derive(Default)]
struct PublishedState {
    snapshots: Vec<ProviderSnapshot>,
    generated_at: i64,
}

pub struct ApiState {
    registry: AccountRegistry,
    published: Mutex<PublishedState>,
    in_flight: AtomicUsize,
    max_in_flight: usize,
}

impl ApiState {
    pub fn new(registry: AccountRegistry) -> Self {
        Self::with_max_in_flight(registry, DEFAULT_MAX_IN_FLIGHT)
    }

    pub fn with_max_in_flight(registry: AccountRegistry, max_in_flight: usize) -> Self {
        Self {
            registry,
            published: Mutex::new(PublishedState::default()),
            in_flight: AtomicUsize::new(0),
            max_in_flight,
        }
    }

    pub fn publish(&self, snapshots: &[ProviderSnapshot], generated_at: i64) {
        if let Ok(mut published) = self.published.lock() {
            published.snapshots = snapshots.to_vec();
            published.generated_at = generated_at;
        }
    }

    pub fn route(&self, method: &Method, url: &str) -> RouteResponse {
        let Some(_permit) = self.try_acquire() else {
            return RouteResponse::json(503, json!({"error": "server_busy"}));
        };
        let path = url.split('?').next().unwrap_or(url);

        if method == &Method::Options {
            return if is_api_path(path) {
                RouteResponse::json(204, Value::Null)
            } else {
                RouteResponse::json(404, json!({"error": "not_found"}))
            };
        }
        if method != &Method::Get {
            return RouteResponse::json(405, json!({"error": "method_not_allowed"}));
        }

        let published = match self.published.lock() {
            Ok(published) => published,
            Err(_) => return RouteResponse::json(503, json!({"error": "server_busy"})),
        };
        match path {
            "/v1/usage" => RouteResponse::json(
                200,
                serialize_usage_with_registry(&published.snapshots, &self.registry),
            ),
            "/v1/limits" => RouteResponse::json(
                200,
                serialize_limits_with_registry(
                    &published.snapshots,
                    published.generated_at,
                    &self.registry,
                ),
            ),
            _ => {
                if let Some(token) = path.strip_prefix("/v1/usage/") {
                    self.selected_response(&published, token, false)
                } else if let Some(token) = path.strip_prefix("/v1/limits/") {
                    self.selected_response(&published, token, true)
                } else {
                    RouteResponse::json(404, json!({"error": "not_found"}))
                }
            }
        }
    }

    fn selected_response(
        &self,
        published: &PublishedState,
        token: &str,
        limits: bool,
    ) -> RouteResponse {
        if token.is_empty() || !self.is_known(&published.snapshots, token) {
            return RouteResponse::json(404, json!({"error": "provider_not_found"}));
        }
        let selected: Vec<ProviderSnapshot> = published
            .snapshots
            .iter()
            .filter(|snapshot| {
                snapshot_card_id(snapshot) == token || snapshot_provider_id(snapshot) == token
            })
            .cloned()
            .collect();
        let body = if limits {
            serialize_limits_with_registry(&selected, published.generated_at, &self.registry)
        } else {
            serialize_usage_with_registry(&selected, &self.registry)
        };
        RouteResponse::json(200, body)
    }

    fn is_known(&self, snapshots: &[ProviderSnapshot], token: &str) -> bool {
        !self.registry.match_token(token).is_empty()
            || snapshots.iter().any(|snapshot| {
                snapshot_card_id(snapshot) == token || snapshot_provider_id(snapshot) == token
            })
    }

    fn try_acquire(&self) -> Option<InFlightPermit<'_>> {
        let mut current = self.in_flight.load(Ordering::Acquire);
        loop {
            if current >= self.max_in_flight {
                return None;
            }
            match self.in_flight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(InFlightPermit {
                        counter: &self.in_flight,
                    })
                }
                Err(observed) => current = observed,
            }
        }
    }
}

struct InFlightPermit<'a> {
    counter: &'a AtomicUsize,
}

impl Drop for InFlightPermit<'_> {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Release);
    }
}

fn snapshot_card_id(snapshot: &ProviderSnapshot) -> &str {
    if snapshot.card_id.is_empty() {
        &snapshot.id
    } else {
        &snapshot.card_id
    }
}

fn snapshot_provider_id(snapshot: &ProviderSnapshot) -> &str {
    if snapshot.provider_id.is_empty() {
        &snapshot.id
    } else {
        &snapshot.provider_id
    }
}

fn is_api_path(path: &str) -> bool {
    path == "/v1/usage"
        || path == "/v1/limits"
        || path.starts_with("/v1/usage/")
        || path.starts_with("/v1/limits/")
}

fn global() -> &'static ApiState {
    static STATE: OnceLock<ApiState> = OnceLock::new();
    STATE.get_or_init(|| ApiState::new(AccountRegistry::default()))
}

/// Called after each usage refresh with all enabled provider snapshots.
pub fn publish(snapshots: &[ProviderSnapshot]) {
    global().publish(snapshots, chrono::Utc::now().timestamp_millis());
}

/// Binds only to `127.0.0.1:6736` and serves until the app exits. If the port
/// is occupied, the API is unavailable for that session and the app continues.
pub fn start() {
    std::thread::spawn(|| {
        let server = match tiny_http::Server::http("127.0.0.1:6736") {
            Ok(server) => server,
            Err(error) => {
                eprintln!("[openmeter] local API: port 6736 unavailable ({error}) - API off");
                return;
            }
        };
        eprintln!("[openmeter] local API: http://127.0.0.1:6736/v1/usage");
        for request in server.incoming_requests() {
            std::thread::spawn(move || respond(request));
        }
    });
}

fn respond(request: tiny_http::Request) {
    let routed = global().route(request.method(), request.url());
    let body = if routed.status == 204 {
        String::new()
    } else {
        routed.body.to_string()
    };
    let mut response = tiny_http::Response::from_string(body).with_status_code(routed.status);
    for (name, value) in routed.headers {
        if let Ok(header) = tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()) {
            response.add_header(header);
        }
    }
    // Deliberately no Access-Control-Allow-Origin header. Native clients can
    // read this loopback API; arbitrary websites cannot.
    let _ = request.respond(response);
}

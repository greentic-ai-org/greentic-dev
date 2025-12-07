use std::net::SocketAddr;

use axum::Json;
use axum::extract::connect_info::ConnectInfo;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use serde_json::json;

use crate::runner::ServerState;

#[derive(Clone, Default)]
pub struct AdminAuth {
    token: Option<String>,
}

impl AdminAuth {
    pub fn from_env() -> Self {
        let token = std::env::var("ADMIN_TOKEN")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Self { token }
    }

    fn authorize(&self, addr: SocketAddr, bearer: Option<&str>) -> Result<(), StatusCode> {
        if let Some(expected) = &self.token {
            let token = bearer.ok_or(StatusCode::UNAUTHORIZED)?;
            if constant_time_eq(token.as_bytes(), expected.as_bytes()) {
                Ok(())
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        } else if addr.ip().is_loopback() {
            Ok(())
        } else {
            Err(StatusCode::FORBIDDEN)
        }
    }
}

pub struct AdminGuard;

impl<S> FromRequestParts<S> for AdminGuard
where
    ServerState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = (StatusCode, Json<serde_json::Value>);

    fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        let server_state = ServerState::from_ref(state);
        let admin = server_state.admin.clone();
        let addr = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0);
        let bearer = extract_bearer(parts);

        async move {
            let addr = addr.ok_or((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "connect info unavailable" })),
            ))?;
            admin.authorize(addr, bearer.as_deref()).map_err(|status| {
                (
                    status,
                    Json(json!({
                        "error": if status == StatusCode::UNAUTHORIZED {
                            "admin token required"
                        } else {
                            "admin access restricted"
                        }
                    })),
                )
            })?;
            Ok(AdminGuard)
        }
    }
}

fn extract_bearer(parts: &Parts) -> Option<String> {
    let header = parts.headers.get(AUTHORIZATION)?.to_str().ok()?;
    header
        .strip_prefix("Bearer ")
        .map(|value| value.trim().to_string())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&left, &right) in a.iter().zip(b.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_without_token_is_allowed() {
        let auth = AdminAuth::from_env();
        assert!(auth.authorize("127.0.0.1:0".parse().unwrap(), None).is_ok());
    }

    #[test]
    fn remote_without_token_is_forbidden() {
        let auth = AdminAuth::default();
        assert_eq!(
            auth.authorize("10.0.0.1:0".parse().unwrap(), None),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn token_requires_bearer() {
        let auth = AdminAuth {
            token: Some("secret".into()),
        };
        assert_eq!(
            auth.authorize("127.0.0.1:0".parse().unwrap(), None),
            Err(StatusCode::UNAUTHORIZED)
        );
        assert!(
            auth.authorize("127.0.0.1:0".parse().unwrap(), Some("secret"))
                .is_ok()
        );
    }
}

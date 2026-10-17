//! Pane, session and snapshot handlers. OWNER: worker H2.
use super::{ApiError, ApiResult, Request};
use crate::server::Shared;

pub fn handle(shared: &Shared, req: Request) -> ApiResult {
    let _ = shared;
    Err(ApiError::new("not_implemented", format!("{req:?}")))
}

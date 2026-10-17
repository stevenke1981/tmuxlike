//! Agent state model, reports, waits and prompts. OWNER: worker H3.
use super::{ApiError, ApiResult, Request, Status};
use crate::config::Config;
use crate::server::{Pane, Shared};

/// Effective status of a pane (see contract in docs/team/herdr-alignment/contract.md).
pub fn status(p: &Pane, cfg: &Config) -> Status {
    if p.busy(cfg) {
        Status::Working
    } else if p.attention {
        Status::Done
    } else {
        Status::Idle
    }
}

pub fn handle(shared: &Shared, req: Request) -> ApiResult {
    let _ = shared;
    Err(ApiError::new("not_implemented", format!("{req:?}")))
}

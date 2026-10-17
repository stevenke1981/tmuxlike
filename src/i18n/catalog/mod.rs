//! Catalog registry. Each area file owns its keys (prefix = area name).
pub mod cli;
pub mod messages;
pub mod render;

/// Every catalog, for lookup and completeness tests.
pub fn all() -> impl Iterator<Item = &'static [super::Entry]> {
    [render::ENTRIES, messages::ENTRIES, cli::ENTRIES].into_iter()
}

//! The crates the mod API's signatures name, indexed by [`mod_source::index`].
pub(crate) use mod_source::index::{Found, Index, ItemId, uses};

/// The crates whose items mod API signatures name: crate name and source directory.
pub(crate) const CRATES: &[(&str, &str)] = &[
    ("mod_api", "crates/sunrise-mod-vocab/src"),
    ("sunrise_core", "crates/sunrise-core/src"),
    ("sunrise_mod_runtime", "crates/sunrise-mod-runtime/src"),
    ("sunrise_world", "crates/sunrise-world/src"),
    ("sunrise_items", "crates/sunrise-items/src"),
    ("sunrise_client_core", "crates/sunrise-client-core/src"),
    ("sunrise_assets", "crates/sunrise-assets/src"),
    ("sunrise_mod_package", "crates/sunrise-mod-package/src"),
];

/// An index of the game's crates, read below `root`.
pub(crate) fn game<'a>(root: &'a std::path::Path, included: &'a [(&'a str, &'a str)]) -> Index<'a> {
    Index::new(root, included, CRATES).expanding(crate::exports::expand)
}

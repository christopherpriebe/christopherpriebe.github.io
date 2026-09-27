//! What the page reports about the build it runs.

use serde::Serialize;

use crate::share::SCHEMA_VERSION;

/// The build: the crates' version, and the newest share format it reads and
/// the one it writes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    /// The crates' version, such as `0.1.0`.
    pub version: String,
    /// The share format: [`SCHEMA_VERSION`].
    pub schema_version: u32,
}

/// This build's [`BuildInfo`].
pub fn build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        schema_version: SCHEMA_VERSION,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    #[test]
    fn the_build_reports_the_crate_version_and_the_share_format() {
        let info = build_info();
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(info.schema_version, SCHEMA_VERSION);
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "schemaVersion": SCHEMA_VERSION,
            })
        );
    }
}

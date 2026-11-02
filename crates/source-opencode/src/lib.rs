//! OpenCode source adapter (M0-06); source SQLite must remain read-only.
//!
//! M0-01 intentionally exposes no fake parser, persistence, or sync functions.
//! Implement the module at its designated milestone after verifying its contract.

/// Build-time module identifier for bootstrap inspection only.
pub const MODULE_ID: &str = "source-opencode";

#[cfg(test)]
mod tests {
    use super::MODULE_ID;

    #[test]
    fn reports_module_id() {
        assert_eq!(MODULE_ID, "source-opencode");
    }
}

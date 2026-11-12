//! Logging: `tracing` to stderr, level from one environment variable.
//!
//! Set `ORCH_LOG` to a level (`error`, `warn`, `info`, `debug`, `trace`) or an
//! `EnvFilter` directive string (e.g. `orch_core=debug`). Unset means **off** (quiet).
//!
//! Rule: never log secrets (tokens, keys, environment values, prompt bodies). Log names,
//! ids and paths only.

use tracing_subscriber::EnvFilter;

/// The environment variable that controls the log level.
pub const LOG_ENV: &str = "ORCH_LOG";

/// Build the filter from a raw value; unset, empty or invalid means off.
pub fn filter_from(value: Option<&str>) -> EnvFilter {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => EnvFilter::try_new(v).unwrap_or_else(|_| EnvFilter::new("off")),
        None => EnvFilter::new("off"),
    }
}

/// Install the global subscriber (stderr). Safe to call more than once.
pub fn init() {
    let value = std::env::var(LOG_ENV).ok();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter_from(value.as_deref()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_by_default() {
        assert_eq!(filter_from(None).to_string(), "off");
        assert_eq!(filter_from(Some("  ")).to_string(), "off");
    }

    #[test]
    fn level_is_honoured() {
        assert_eq!(filter_from(Some("debug")).to_string(), "debug");
    }

    #[test]
    fn init_twice_is_fine() {
        init();
        init();
    }
}

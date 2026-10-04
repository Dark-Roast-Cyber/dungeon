mod binding;
mod cli;
mod config;
mod export;
mod imports;
mod machine;
mod openshell;
mod output;
mod policy;

pub(crate) const OPEN_SHELL_VERSION: &str = "0.0.116";

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorCode {
    InvalidRequest,
    ApprovalRequired,
    Unbound,
    Unsupported,
    // Keep the machine error vocabulary fixed where the producing capability is unsupported.
    #[cfg_attr(not(unix), allow(dead_code))]
    Busy,
    Conflict,
    IdentityMismatch,
    PolicyDrift,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    CleanupUnconfirmed,
    OperationFailed,
}

#[derive(Debug)]
pub(crate) struct Error {
    message: String,
    code: ErrorCode,
    effects: &'static str,
}

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: ErrorCode::OperationFailed,
            effects: "unknown",
        }
    }

    pub(crate) fn refusal(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code,
            effects: "none",
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn classified(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code,
            effects: "unknown",
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::new(value.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::new(value.to_string())
    }
}

fn main() {
    if let Some(code) = machine::entry(std::env::args_os().collect()) {
        std::process::exit(code);
    }
    std::process::exit(match cli::run() {
        Ok(code) => code,
        Err(error) => {
            use std::io::Write;
            let _ = writeln!(std::io::stderr(), "dungeon: {error}");
            1
        }
    });
}

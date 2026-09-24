mod cli;
mod config;
mod export;
mod openshell;
mod policy;

pub(crate) const OPEN_SHELL_VERSION: &str = "0.0.116";

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub(crate) struct Error(String);

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self(value.to_string())
    }
}

fn main() {
    std::process::exit(match cli::run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("dungeon: {error}");
            1
        }
    });
}

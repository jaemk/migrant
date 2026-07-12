/*!
Small shared helpers
*/
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use percent_encoding::{percent_encode, NON_ALPHANUMERIC};

use crate::errors::*;
use crate::macros::bail;

/// Percent encode a string
pub(crate) fn encode(s: &str) -> String {
    percent_encode(s.as_bytes(), NON_ALPHANUMERIC).to_string()
}

/// Write the provided bytes to the specified path
pub(crate) fn write_to_path(path: &Path, content: &[u8]) -> Result<()> {
    std::fs::write(path, content)?;
    Ok(())
}

/// Run the given command in the foreground
pub(crate) fn open_file_in_fg(command: &str, file_path: &str) -> Result<()> {
    let status = Command::new(command).arg(file_path).spawn()?.wait()?;
    if !status.success() {
        bail!(
            ShellCommand,
            "Command `{}` exited with status `{}`",
            command,
            status
        )
    }
    Ok(())
}

/// Prompt the user and return their input
pub(crate) fn prompt(msg: &str) -> Result<String> {
    print!("{}", msg);
    io::stdout().flush()?;
    let mut resp = String::new();
    io::stdin().read_line(&mut resp)?;
    Ok(resp.trim().to_string())
}

/// Print and flush stdout, for partial-line progress output
pub(crate) fn print_flush(s: &str) {
    print!("{}", s);
    let _ = io::stdout().flush();
}

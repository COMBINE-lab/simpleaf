use anyhow::{Context, bail};

use crate::utils::prog_utils::{self, CommandVerbosityLevel};

pub fn run_capture(
    cmd: &mut std::process::Command,
    context: &str,
) -> anyhow::Result<std::process::Output> {
    prog_utils::execute_command(cmd, CommandVerbosityLevel::Quiet)
        .with_context(|| format!("failed to execute {}", context))
}

pub fn ensure_success(output: &std::process::Output, context: &str) -> anyhow::Result<()> {
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "{} failed with exit status {}. stderr: {}",
        context,
        output.status,
        stderr.trim()
    );
}

pub fn run_checked(
    cmd: &mut std::process::Command,
    context: &str,
) -> anyhow::Result<std::process::Output> {
    let output = run_capture(cmd, context)?;
    ensure_success(&output, context)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::run_checked;

    #[test]
    fn run_checked_succeeds_for_true_command() {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg("true");
        run_checked(&mut cmd, "sh true").expect("expected true command to succeed");
    }

    #[test]
    fn run_checked_errors_for_false_command() {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg("false");
        let err = run_checked(&mut cmd, "sh false").expect_err("expected false command to fail");
        assert!(format!("{:#}", err).contains("failed with exit status"));
    }
}

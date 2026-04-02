//! Shell completion and man page generation.
//!
//! Generates shell completions for bash, zsh, fish, and powershell.
//! Also generates man pages from clap command definitions.

use std::io;
use std::path::Path;

use clap::CommandFactory;
use clap_complete::{generate, Shell};

use crate::Cli;

/// Generate shell completions to stdout.
pub fn generate_completions(shell: Shell, buf: &mut impl io::Write) {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "oxicode", buf);
}

/// Generate shell completions and write to a file.
// Planned CLI feature
#[allow(dead_code)]
pub fn generate_completions_to_file(shell: Shell, out_dir: &Path) -> io::Result<()> {
    let mut cmd = Cli::command();
    let path = clap_complete::generate_to(shell, &mut cmd, "oxicode", out_dir)?;
    eprintln!("Generated: {}", path.display());
    Ok(())
}

/// Generate man page and write to a buffer.
pub fn generate_man_page(buf: &mut impl io::Write) -> io::Result<()> {
    let cmd = Cli::command();
    let man = clap_mangen::Man::new(cmd);
    man.render(buf)
}

/// Generate man page to a file in the given directory.
// Planned CLI feature
#[allow(dead_code)]
pub fn generate_man_page_to_file(out_dir: &Path) -> io::Result<()> {
    let path = out_dir.join("oxicode.1");
    let mut file = std::fs::File::create(&path)?;
    generate_man_page(&mut file)?;
    eprintln!("Generated: {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_bash_completions() {
        let mut buf = Vec::new();
        generate_completions(Shell::Bash, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("oxicode"),
            "Bash completions should reference 'oxicode'"
        );
    }

    #[test]
    fn test_generate_zsh_completions() {
        let mut buf = Vec::new();
        generate_completions(Shell::Zsh, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(!output.is_empty());
    }

    #[test]
    fn test_generate_fish_completions() {
        let mut buf = Vec::new();
        generate_completions(Shell::Fish, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("oxicode"));
    }

    #[test]
    fn test_generate_powershell_completions() {
        let mut buf = Vec::new();
        generate_completions(Shell::PowerShell, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(!output.is_empty());
    }

    #[test]
    fn test_generate_man_page() {
        let mut buf = Vec::new();
        generate_man_page(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("oxicode"),
            "Man page should reference 'oxicode'"
        );
    }
}

//! Shell completion generation command.

use anyhow::Result;
use clap::{Args, CommandFactory, ValueEnum};
use clap_complete::{Shell, generate};
use std::io;

/// Arguments for the completion command
#[derive(Args, Debug, Clone)]
pub struct CompletionArgs {
    /// The shell to generate completion for
    #[arg(value_enum)]
    pub shell: ShellType,
}

/// Shell types for completion generation
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ShellType {
    /// Bash shell
    Bash,
    /// Zsh shell
    Zsh,
    /// Fish shell
    Fish,
    /// `PowerShell`
    #[value(alias = "powershell")]
    PowerShell,
    /// Elvish shell
    Elvish,
}

impl From<ShellType> for Shell {
    fn from(shell: ShellType) -> Self {
        match shell {
            ShellType::Bash => Self::Bash,
            ShellType::Zsh => Self::Zsh,
            ShellType::Fish => Self::Fish,
            ShellType::PowerShell => Self::PowerShell,
            ShellType::Elvish => Self::Elvish,
        }
    }
}

/// Run the completion command
pub async fn run(args: CompletionArgs) -> Result<()> {
    let mut cmd = crate::commands::Cli::command();
    let name = cmd.get_name().to_string();
    let shell: Shell = args.shell.into();

    generate(shell, &mut cmd, name, &mut io::stdout());

    // Print installation instructions to stderr
    let instructions = get_installation_instructions(args.shell);
    eprintln!();
    eprintln!("# Installation instructions:");
    eprintln!("{instructions}");

    Ok(())
}

const fn get_installation_instructions(shell: ShellType) -> &'static str {
    match shell {
        ShellType::Bash => {
            r#"# Add to ~/.bashrc or ~/.bash_profile:
# eval "$(libretto completion bash)"
# Or save to a file:
# libretto completion bash > /etc/bash_completion.d/libretto"#
        }

        ShellType::Zsh => {
            r#"# Add to ~/.zshrc:
# eval "$(libretto completion zsh)"
# Or save to a file in your fpath:
# libretto completion zsh > ~/.zfunc/_libretto"#
        }

        ShellType::Fish => {
            r"# Save to fish completions directory:
# libretto completion fish > ~/.config/fish/completions/libretto.fish"
        }

        ShellType::PowerShell => {
            r"# Add to your PowerShell profile:
# Invoke-Expression (& libretto completion powershell | Out-String)"
        }

        ShellType::Elvish => {
            r"# Add to ~/.elvish/rc.elv:
# eval (libretto completion elvish | slurp)"
        }
    }
}

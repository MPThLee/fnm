use super::command::Command;
use crate::config::FnmConfig;
use crate::directories;
use crate::fs::symlink_dir;
use crate::path_ext::PathExt;
use crate::shell::{infer_shell, Shell, AVAILABLE_SHELLS};
use crate::version_switch_strategy::VersionSwitchStrategy;
use crate::{outln, shims};
use colored::Colorize;
use std::collections::HashMap;
use std::fmt::Debug;
use thiserror::Error;

#[derive(clap::Parser, Debug, Default)]
pub struct Env {
    /// The shell syntax to use. Infers when missing.
    #[clap(long)]
    #[clap(possible_values = AVAILABLE_SHELLS)]
    shell: Option<Box<dyn Shell>>,
    /// Print JSON instead of shell commands.
    #[clap(long, conflicts_with = "shell")]
    json: bool,
    /// Deprecated. This is the default now.
    #[clap(long, hide = true)]
    multi: bool,
    /// Print the script to change Node versions every directory change
    #[clap(long)]
    use_on_cd: bool,

    /// Adds `node` shims to your PATH environment variable
    /// to allow you to use `node` commands in your shell
    /// without rehashing.
    #[structopt(long)]
    with_shims: bool,
}

fn generate_symlink_path() -> String {
    format!(
        "{}_{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis(),
    )
}

fn make_symlink(config: &FnmConfig) -> Result<std::path::PathBuf, Error> {
    let base_dir = directories::multishell_storage().ensure_exists_silently();
    let mut temp_dir = base_dir.join(generate_symlink_path());

    while temp_dir.exists() {
        temp_dir = base_dir.join(generate_symlink_path());
    }

    match symlink_dir(config.default_version_dir(), &temp_dir) {
        Ok(_) => Ok(temp_dir),
        Err(source) => Err(Error::CantCreateSymlink { source, temp_dir }),
    }
}

impl Command for Env {
    type Error = Error;

    fn apply(self, config: &FnmConfig) -> Result<(), Self::Error> {
        if self.multi {
            outln!(
                config,
                Error,
                "{} {} is deprecated. This is now the default.",
                "warning:".yellow().bold(),
                "--multi".italic()
            );
        }

        let multishell_path = make_symlink(config)?;
        let binary_path = if self.with_shims {
            shims::store_shim(config).map_err(|source| Error::CantCreateShims { source })?
        } else if cfg!(windows) {
            multishell_path.clone()
        } else {
            multishell_path.join("bin")
        };

        let fnm_dir = config.base_dir_with_default();

        let env_vars = HashMap::from([
            ("FNM_MULTISHELL_PATH", multishell_path.to_str().unwrap()),
            (
                "FNM_VERSION_FILE_STRATEGY",
                config.version_file_strategy().as_str(),
            ),
            ("FNM_DIR", fnm_dir.to_str().unwrap()),
            ("FNM_LOGLEVEL", config.log_level().as_str()),
            ("FNM_NODE_DIST_MIRROR", config.node_dist_mirror.as_str()),
            ("FNM_ARCH", config.arch.as_str()),
            (
                "FNM_VERSION_SWITCH_STRATEGY",
                if self.with_shims {
                    VersionSwitchStrategy::Shims
                } else {
                    VersionSwitchStrategy::PathSymlink
                }
                .as_str(),
            ),
        ]);

        if self.json {
            println!("{}", serde_json::to_string(&env_vars).unwrap());
            return Ok(());
        }

        let shell: Box<dyn Shell> = self
            .shell
            .or_else(infer_shell)
            .ok_or(Error::CantInferShell)?;

        println!("{}", shell.path(&binary_path)?);

        for (name, value) in &env_vars {
            println!("{}", shell.set_env_var(name, value));
        }

        if self.use_on_cd {
            println!("{}", shell.use_on_cd(config)?);
        }
        if let Some(v) = shell.rehash() {
            println!("{}", v);
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(
        "{}\n{}\n{}\n{}",
        "Can't infer shell!",
        "fnm can't infer your shell based on the process tree.",
        "Maybe it is unsupported? we support the following shells:",
        shells_as_string()
    )]
    CantInferShell,
    #[error("Can't create Node.js shims: {}", source)]
    CantCreateShims {
        #[source]
        source: std::io::Error,
    },
    #[error("Can't create the symlink for multishells at {temp_dir:?}. Maybe there are some issues with permissions for the directory? {source}")]
    CantCreateSymlink {
        #[source]
        source: std::io::Error,
        temp_dir: std::path::PathBuf,
    },
    #[error(transparent)]
    ShellError {
        #[from]
        source: anyhow::Error,
    },
}

fn shells_as_string() -> String {
    AVAILABLE_SHELLS
        .iter()
        .map(|x| format!("* {}", x))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smoke() {
        use crate::shell;
        let config = FnmConfig::default();
        let shell: Box<dyn Shell> = if cfg!(windows) {
            Box::from(shell::WindowsCmd)
        } else {
            Box::from(shell::Bash)
        };
        Env {
            shell: Some(shell),
            ..Env::default()
        }
        .call(config);
    }
}

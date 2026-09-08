use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "penv",
    version,
    about = "penv gets the right values into the right process at the right time.",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Emit JSON on stdout, whatever stdout is attached to
    #[arg(long, global = true)]
    pub json: bool,

    /// Treat this session as an agent: JSON out, values masked
    #[arg(long, global = true)]
    pub agent: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Read .env, write .env.schema, and keep .env out of the repository
    Init {
        /// Overwrite an existing .env.schema
        #[arg(long)]
        force: bool,
    },

    /// Validate, then run a command with the values in its environment only
    Run {
        /// The environment to read
        #[arg(long)]
        env: Option<String>,
        /// Leave the child's output unmasked
        #[arg(long)]
        no_mask: bool,
        /// The command to run
        #[arg(last = true, num_args = 1..)]
        command: Vec<String>,
    },

    /// Move local values to the cloud and delete .env
    Push {
        /// The environment to write to
        #[arg(long)]
        env: Option<String>,
    },

    /// Write a plain .env from the cloud
    Pull {
        /// The environment to read
        #[arg(long)]
        env: Option<String>,
        /// Confirm a person, not an agent, asked for the file
        #[arg(long)]
        i_am_human: bool,
    },

    /// Sign in with a device code; the credential goes to the OS keychain
    Login,

    /// Remove the stored credential
    Logout,

    /// Write one value without echoing it
    Set {
        /// The key to write
        key: String,
        /// The environment to write to
        #[arg(long)]
        env: Option<String>,
    },

    /// Remove one value
    Unset {
        /// The key to remove
        key: String,
        /// The environment to write to
        #[arg(long)]
        env: Option<String>,
    },

    /// List keys, types and which ones have a value
    Ls,

    /// Report schema problems and missing values
    Check {
        /// Check one key instead of all of them
        key: Option<String>,
    },

    /// Write the typed file for a language target
    Gen {
        /// The target name, such as ts or py; omit it to list the targets
        target: Option<String>,
        /// Write somewhere other than the path the target names
        #[arg(long, value_name = "PATH")]
        out: Option<std::path::PathBuf>,
        /// Compare with what is on disk instead of writing
        #[arg(long)]
        check: bool,
    },

    /// Write the harness rules that keep agents out of .env
    Guard {
        /// The harnesses to write, instead of the installed ones
        harness: Vec<String>,
        /// Write every harness penv knows, installed or not
        #[arg(long)]
        all: bool,
        /// Report coverage instead of writing
        #[arg(long)]
        check: bool,
    },

    /// Print one value after console approval
    Reveal {
        /// The key to reveal
        key: String,
        /// The environment to read
        #[arg(long)]
        env: Option<String>,
    },

    /// Machine identities
    Machine {
        #[command(subcommand)]
        command: MachineCommand,
    },

    /// Replace this binary from the signed GitHub release
    Upgrade,

    /// Print the shell completion script
    Completions {
        /// bash, zsh, fish, powershell or elvish
        shell: String,
    },

    /// Run as a harness hook
    Hook {
        /// The harness, such as claude-code
        harness: String,
    },

    /// Print the schema as JSON
    Schema,

    /// Print the command manifest
    Help {
        /// Print help for one command instead
        command: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum MachineCommand {
    /// Bind a server keypair from a one-time secret
    Enroll {
        /// The one-time enrolment secret
        secret: String,
    },
}

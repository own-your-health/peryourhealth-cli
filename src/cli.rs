//! The command tree (piekstra-cli/1).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use pk_cli_auth::LoginArgs;
use pk_cli_browser::BrowserArgs;
use pk_cli_core::CommonArgs;
use pk_cli_selfupdate::SelfUpdateArgs;

pub const BIN: &str = "peryourhealth";

/// A patient's medical bills from PerYourHealth, the billing site Optum runs
/// for many physician groups and hospitals: each account's balances, the
/// services billed by date with what insurance paid and adjusted and what the
/// patient owes, payments made, and statement PDFs. Read-only: it never pays
/// or changes anything. Unofficial.
#[derive(Parser, Debug)]
#[command(name = BIN, version, about, long_about = None)]
pub struct Cli {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Override the config file location.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Read PerYourHealth as this profile: one configured with `peryourhealth
    /// profiles add <NAME>` and signed in with `peryourhealth auth login
    /// --browser --profile <NAME>`. There is no default profile.
    #[arg(long, global = true, env = "PERYOURHEALTH_PROFILE", value_name = "NAME")]
    pub profile: Option<String>,

    /// Whether --profile was given as a flag, not through PERYOURHEALTH_PROFILE.
    #[arg(skip)]
    pub profile_flag: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// The PerYourHealth sign-in (One Healthcare ID), one per --profile.
    #[command(subcommand)]
    Auth(AuthCmd),
    /// The logins: one profile per One Healthcare ID (profile-list/v1).
    #[command(subcommand)]
    Profiles(ProfilesCmd),
    /// One read of PerYourHealth as --profile <NAME>: `GET <PATH>` on
    /// www.peryourhealth.com, printed as it answers. Read-only: the sign-in,
    /// sign-out and payment paths are refused before a request is sent.
    Api(ApiArgs),
    /// Non-secret settings: the config file and the profiles in it.
    #[command(subcommand)]
    Config(ConfigCmd),
    /// Update to the latest release from GitHub.
    SelfUpdate(SelfUpdateArgs),
    /// Print a shell completion script.
    Completions { shell: Shell },
    /// Machine-readable capability discovery (cli-info/v1).
    Info,
}

/// `peryourhealth api GET <PATH>`: cli-common's `ApiArgs` offers writes, and
/// peryourhealth only reads.
#[derive(Args, Debug, Clone)]
pub struct ApiArgs {
    /// GET (any case): the only method sent.
    #[arg(value_enum, ignore_case = true)]
    pub method: ApiMethod,
    /// A path on https://www.peryourhealth.com (leading /), or a URL on it.
    pub path: String,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiMethod {
    #[value(name = "GET")]
    Get,
}

#[derive(Subcommand, Debug)]
pub enum AuthCmd {
    /// Sign in to PerYourHealth as --profile <NAME> and keep the session in
    /// peryourhealth's vault: encrypted files under
    /// ~/.config/peryourhealth/vault/, sealed with a key held in the OS
    /// keychain (auth-login/v1).
    ///
    /// --browser opens a Chrome window on www.peryourhealth.com in the
    /// profile's own browser profile. There the patient enters the
    /// PerYourHealth ID from a statement, continues to One Healthcare ID and
    /// signs in (user name, password and the code One Healthcare ID sends).
    /// --stdin reads the Cookie header of a www.peryourhealth.com request,
    /// copied from a signed-in browser's DevTools, with --user-agent from the
    /// same request. One read checks the session before it is stored, unless
    /// --no-verify.
    Login(LoginCmd),
    /// Whether the profile's session is stored and when it was captured
    /// (auth-status/v1). Sends no request.
    Status,
    /// Remove the profile's stored session, then the browser profile its
    /// --browser sign-in keeps (auth-logout/v1).
    Logout,
    /// Remove every profile's stored session from the vault and the vault's
    /// key from the keychain (auth-reset/v1): the way out when the key is
    /// lost or damaged. Asks first; without a terminal it needs --force
    /// (exit 6).
    Reset {
        /// Do not ask.
        #[arg(long)]
        force: bool,
    },
}

// The family's login flags with `--browser` beside them (DESIGN.md §2.2).
// A plain comment: a doc comment here would replace `auth login`'s about.
#[derive(Args, Debug)]
pub struct LoginCmd {
    #[command(flatten)]
    pub login: LoginArgs,
    #[command(flatten)]
    pub browser: BrowserArgs,
    /// With --stdin or --from-env: the User-Agent of the browser the Cookie
    /// header was copied from, sent with the check and every later read.
    #[arg(long, value_name = "UA")]
    pub user_agent: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum ProfilesCmd {
    /// The configured profiles, by name (profile-list/v1).
    #[command(visible_alias = "ls")]
    List,
    /// Add a profile: 1 to 32 lowercase letters, digits, `-` or `_`,
    /// starting with a letter or a digit (profile-add/v1).
    Add { name: String },
    /// Remove a profile with its stored session and the browser profile its
    /// --browser sign-in keeps (profile-remove/v1). Asks first; without a
    /// terminal it needs --force (exit 6).
    Remove {
        name: String,
        /// Do not ask.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// Print the config file path.
    Path,
    /// Show the effective configuration.
    Show,
}

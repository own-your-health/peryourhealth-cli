//! The commands: argument checks, which profile, and output.

use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory, FromArgMatches};
use pk_cli_config::profile::{self, ProfileName, Profiles};
use pk_cli_config::ConfigStore;
use pk_cli_core::info::{AuthInfo, CliInfo};
use pk_cli_core::{output, CliError};
use pk_cli_selfupdate::Updater;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::cli::*;
use crate::portal::{Env, Site};

const REPO: &str = "own-your-health/peryourhealth-cli";
pub const PROFILE_ENV: &str = "PERYOURHEALTH_PROFILE";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// The PerYourHealth logins, one per One Healthcare ID (`peryourhealth profiles add <NAME>`).
    #[serde(default, skip_serializing_if = "Profiles::is_empty")]
    pub profiles: Profiles,
}

pub fn main() {
    // A reader that closes the pipe early (`peryourhealth api GET ... | head`)
    // ends the process quietly, as it does any Unix tool.
    #[cfg(unix)]
    // SAFETY: called first, before any thread starts; it only resets the
    // disposition Rust's runtime set for SIGPIPE back to the default.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let matches = Cli::command().get_matches();
    let mut cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    cli.profile_flag = profile_flag(&matches);
    match run(&cli) {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(e) => std::process::exit(output::fail(&e, cli.common.json)),
    }
}

/// Whether `--profile` was given on the command line, before or after the
/// subcommand, rather than through [`PROFILE_ENV`].
pub fn profile_flag(matches: &ArgMatches) -> bool {
    let mut m = matches;
    let mut on_line = m.value_source("profile") == Some(ValueSource::CommandLine);
    while let Some((_, sub)) = m.subcommand() {
        m = sub;
        on_line |= m.value_source("profile") == Some(ValueSource::CommandLine);
    }
    on_line
}

/// The profile a command signs in as, which must be configured: a missing,
/// malformed or unknown name is exit 2 listing the configured ones, and a
/// lone profile is never picked.
pub fn resolve_profile(given: Option<&str>, configured: &Profiles) -> Result<ProfileName, CliError> {
    profile::resolve(given.filter(|g| !g.is_empty()), PROFILE_ENV, configured, BIN)
}

/// For a command that needs no sign-in: `--profile` on its command line is
/// exit 2, and the variable is not read.
fn no_profile(cli: &Cli, command: &str) -> Result<(), CliError> {
    if cli.profile_flag {
        return Err(CliError::Usage(format!(
            "`{BIN} {command}` needs no sign-in and takes no --profile"
        )));
    }
    Ok(())
}

fn store(cli: &Cli) -> ConfigStore {
    ConfigStore::new(BIN).with_override(cli.config.clone())
}

fn note(cli: &Cli, msg: &str) {
    if !cli.common.quiet {
        eprintln!("{msg}");
    }
}

/// The process's exit code: 0, or an incomplete stream's error code.
pub fn run(cli: &Cli) -> Result<i32, CliError> {
    let profiles = || -> Result<Profiles, CliError> { Ok(store(cli).load::<Config>()?.profiles) };
    let signed_in = || -> Result<ProfileName, CliError> { resolve_profile(cli.profile.as_deref(), &profiles()?) };
    match &cli.command {
        Command::Auth(AuthCmd::Reset { force }) => {
            no_profile(cli, "auth reset")?;
            crate::auth::reset(*force, cli.common.interactive(), cli.common.json, cli.common.quiet)?;
        }
        Command::Auth(cmd) => {
            let profile = signed_in()?;
            let (json, quiet) = (cli.common.json, cli.common.quiet);
            match cmd {
                AuthCmd::Login(args) => crate::auth::login(args, &profile, json, quiet)?,
                AuthCmd::Status => crate::auth::status(&profile, json, quiet)?,
                AuthCmd::Logout => crate::auth::logout(&profile, json, quiet)?,
                AuthCmd::Reset { .. } => unreachable!("handled above"),
            }
        }
        Command::Profiles(cmd) => {
            no_profile(cli, "profiles")?;
            profiles_cmd(cli, cmd)?;
        }
        Command::Config(cmd) => {
            no_profile(cli, "config")?;
            config(cli, cmd)?;
        }
        Command::Api(args) => {
            let path = crate::client::api_path(&args.path)?;
            let profile = signed_in()?;
            let env = Env::for_binary()?;
            let site = Site::open(&env, &profile)?;
            let answer = site.api(&path)?;
            site.finish_quietly();
            print_api(cli, &answer);
        }
        Command::SelfUpdate(args) => {
            no_profile(cli, "self-update")?;
            Updater {
                repo: REPO.into(),
                binary: BIN.into(),
                target: env!("BUILD_TARGET").into(),
                current: env!("CARGO_PKG_VERSION").into(),
            }
            .run(args, cli.common.json, cli.common.quiet)?;
        }
        Command::Completions { shell } => {
            no_profile(cli, "completions")?;
            clap_complete::generate(*shell, &mut Cli::command(), BIN, &mut std::io::stdout());
        }
        Command::Info => {
            no_profile(cli, "info")?;
            output::json(&serde_json::to_value(info()).expect("CliInfo serializes"));
        }
    }
    Ok(0)
}

/// `info --json` (cli-info/v1).
pub fn info() -> CliInfo {
    CliInfo::new(
        BIN,
        env!("CARGO_PKG_VERSION"),
        &format!("https://github.com/{REPO}"),
        AuthInfo {
            required: true,
            method: "browser-session".into(),
            login_hint: Some(format!("{BIN} auth login --browser --profile <NAME>")),
        },
        &[
            "profiles",
            "api",
            // Every login command takes --profile.
            pk_med_records::LOGIN_PROFILES,
        ],
    )
}

/// `api`'s answer: a JSON answer as JSON, a page as its text. With --json,
/// one `api-response/v1` document: status, content type, and the body as
/// JSON or text.
fn print_api(cli: &Cli, a: &crate::client::Answer) {
    let json_body = a
        .content_type
        .as_deref()
        .is_some_and(|t| t.contains("json"))
        .then(|| serde_json::from_slice::<serde_json::Value>(&a.body).ok())
        .flatten();
    if cli.common.json {
        let body = json_body.unwrap_or_else(|| String::from_utf8_lossy(&a.body).into_owned().into());
        output::json(&json!({
            "schema": "api-response/v1",
            "status": a.status,
            "content_type": a.content_type,
            "body": body,
        }));
    } else if let Some(v) = json_body {
        output::json(&v);
    } else {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(&a.body);
        if !a.body.ends_with(b"\n") {
            let _ = out.write_all(b"\n");
        }
    }
}

// -------------------------------------------------------------- profiles

/// `profiles list|add|remove` (cli-common DESIGN.md §2.5): the logins, by
/// name, in the config. Removing one removes what peryourhealth keeps for it
/// first.
fn profiles_cmd(cli: &Cli, cmd: &ProfilesCmd) -> Result<(), CliError> {
    let store = store(cli);
    let json_mode = cli.common.json;
    match cmd {
        ProfilesCmd::List => {
            let cfg: Config = store.load()?;
            let list = cfg.profiles.list();
            if json_mode {
                output::json(&serde_json::to_value(&list).unwrap_or_default());
            } else if list.profiles.is_empty() {
                note(
                    cli,
                    &format!("no profiles: `{BIN} profiles add <NAME>`, one per One Healthcare ID login"),
                );
            } else {
                for p in &list.profiles {
                    println!("{}", p.name);
                }
            }
            Ok(())
        }
        ProfilesCmd::Add { name } => {
            let name = ProfileName::parse(name)?;
            let mut cfg: Config = store.load()?;
            let added = cfg.profiles.add(name.clone());
            if added {
                store.save(&cfg)?;
            }
            output::emit(
                json_mode,
                "profile-add",
                json!({ "profile": name.as_str(), "added": added }),
                |_| {
                    note(
                        cli,
                        &if added {
                            format!("added profile {name}; sign in with `{BIN} auth login --browser --profile {name}`")
                        } else {
                            format!("profile {name} is already configured")
                        },
                    )
                },
            );
            Ok(())
        }
        ProfilesCmd::Remove { name, force } => {
            let name = ProfileName::parse(name)?;
            // Whether it can ask, before the config or the vault is read.
            pk_cli_core::confirm::require_confirmable(*force, cli.common.interactive(), "remove a profile")?;
            let cfg: Config = store.load()?;
            if !cfg.profiles.contains(&name) {
                return Err(CliError::NotFound(format!("no profile `{name}`"))
                    .with_remedy(format!("`{BIN} profiles list` shows the configured ones")));
            }
            pk_cli_core::confirm::confirm(
                *force,
                &format!("Remove profile {name}, its stored session and its browser profile?"),
            )?;
            let done = crate::auth::forget_login(&name)?;
            let mut cfg: Config = store.load()?;
            cfg.profiles.remove(&name);
            store.save(&cfg)?;
            let mut v = json!({ "profile": name.as_str(), "session_removed": done.session_removed });
            if let Some(removed) = done.browser_profile_removed {
                v["browser_profile_removed"] = removed.into();
            }
            output::emit(json_mode, "profile-remove", v, |_| {
                note(
                    cli,
                    &format!(
                        "removed profile {name}: {}",
                        if done.session_removed {
                            "its session was removed"
                        } else {
                            "no session was stored"
                        }
                    ),
                )
            });
            Ok(())
        }
    }
}

// ---------------------------------------------------------------- config

fn config(cli: &Cli, cmd: &ConfigCmd) -> Result<(), CliError> {
    let store = store(cli);
    match cmd {
        ConfigCmd::Path => {
            let path = store.path()?;
            if cli.common.json {
                output::json(&json!({ "schema": "config-path/v1", "path": path.display().to_string() }));
            } else {
                println!("{}", path.display());
            }
        }
        ConfigCmd::Show => {
            let cfg: Config = store.load()?;
            let mut v = serde_json::to_value(&cfg).unwrap_or_default();
            if cli.common.json {
                v["schema"] = "config/v1".into();
                output::json(&v);
            } else if v.as_object().is_some_and(|o| o.is_empty()) {
                note(
                    cli,
                    &format!("no settings; profiles are added with `{BIN} profiles add <NAME>`"),
                );
            } else {
                output::render(&v);
            }
        }
    }
    Ok(())
}

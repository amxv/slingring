use crate::{
    cli::{BrowserAction, Command, ConfigAction, ProfileAction, RemoteAction, ServiceAction},
    config::{self, Config},
    daemon::{self, Request},
    login,
    model::{MAX_PROMPT, NewTask, PauseTask, SendMessage},
    service,
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{io::Read, path::Path};

fn output(value: Value) -> Result<()> {
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}
pub fn bounded_read(reader: impl Read, max: usize) -> Result<String> {
    let mut bytes = Vec::new();
    reader.take((max + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= max, "input exceeds size limit");
    String::from_utf8(bytes).context("input must be UTF-8")
}
fn read_message(
    positional: Option<String>,
    file: Option<String>,
    stdin: bool,
    label: &str,
) -> Result<String> {
    match (positional, file, stdin) {
        (Some(value), None, false) => Ok(value),
        (None, Some(path), false) if path != "-" => {
            bounded_read(std::fs::File::open(path)?, MAX_PROMPT)
        }
        (None, Some(_), false) | (None, None, true) => bounded_read(std::io::stdin(), MAX_PROMPT),
        _ => anyhow::bail!("provide a {label}, --{label}-file FILE, or --stdin"),
    }
}
async fn submit_local(
    root: &Path,
    request: Request,
    id: String,
    json: bool,
    remote: bool,
) -> Result<()> {
    ensure!(
        crate::queue::valid_job_id(&id),
        "invalid request ID: expected 1..128 ASCII letters, digits, underscore, hyphen or colon"
    );
    eprintln!(
        "Request ID: {id}. Recover with: slingring jobs --request-id {id}; do not resubmit to poll."
    );
    service::start(root, !remote).await?;
    match daemon::call(root, &request).await {
        Ok(value) => {
            if value["effort_verification"] == "unverified" && value["requested_effort"].is_string()
            {
                eprintln!(
                    "Warning: the requested reasoning effort was not verified. The follow-up was submitted; inspect the conversation setting rather than resubmitting."
                );
            }
            if json {
                output(value)
            } else {
                if let Some(status) = value["pause_status"].as_str() {
                    eprintln!(
                        "Pause verified: {status} ({}). No generated response was read.",
                        value["evidence"].as_str().unwrap_or("UI evidence")
                    );
                }
                println!("{}", value["url"].as_str().context("missing task URL")?);
                Ok(())
            }
        }
        Err(error) => {
            if json {
                let job = crate::queue::recovery_records(root)
                    .ok()
                    .and_then(|jobs| jobs.into_iter().find(|j| j.id == id));
                output(
                    json!({"job_id":id,"error":error.to_string(),"job":job.map(|j|j.public())}),
                )?;
            }
            Err(error)
        }
    }
}
async fn reload(root: &Path) -> Result<()> {
    if daemon::running(root).await {
        daemon::call(root, &Request::Reload).await?;
    }
    Ok(())
}
pub async fn status(root: &Path) -> Result<Value> {
    if let Ok(Ok(mut status)) = tokio::time::timeout(
        std::time::Duration::from_millis(800),
        daemon::call(root, &Request::Status),
    )
    .await
    {
        status["installed"] = json!(service::installed());
        return Ok(status);
    }
    let config = Config::load(root)?;
    let owner_running = crate::queue::owner_running(root)?;
    let jobs = crate::queue::recovery_records(root)?;
    let queued = jobs.iter().filter(|j| j.status == "queued").count();
    let starting = jobs.iter().filter(|j| j.status == "starting").count();
    // A new queue owner may have appeared between the lock probe and journal
    // snapshot. An active recovered record is also evidence against idle/stopped.
    let owner_running = owner_running || queued + starting > 0;
    let active_job = jobs
        .iter()
        .find(|j| j.status == "starting")
        .or_else(|| jobs.iter().find(|j| j.status == "queued"));
    Ok(
        json!({"running":owner_running,"status_available":!owner_running,"status_source":"journal",
        "installed":service::installed(),"remote_enabled":config.remote_enabled,
        "remote_state":if !config.remote_enabled {"disabled"} else if owner_running {"unknown"} else {"offline"},
        "browser_busy":if owner_running {Value::Null} else {json!(false)},
        "active_tasks":queued + starting,"queued_tasks":queued,"starting_tasks":starting,
        "active_job":active_job.map(|j|j.public()),
        "latest_job":jobs.back().map(|j|j.public()),
        "latest_finished_job":jobs.iter().rev().find(|j|!j.active()).map(|j|j.public()),
        "latest_started_job":jobs.iter().rev().find(|j|j.status == "started").map(|j|j.public()),
        "config":config.redacted()}),
    )
}
pub async fn run(command: Command) -> Result<()> {
    let root = config::root()?;
    ensure!(
        root.is_absolute(),
        "SLINGRING_HOME must be an absolute path"
    );
    match command {
        Command::NewTask {
            prompt,
            prompt_file,
            stdin,
            reasoning_effort,
            images,
            request_id,
            json,
        } => {
            let prompt = read_message(prompt, prompt_file, stdin, "prompt")?;
            let config = Config::load(&root)?;
            let task = NewTask {
                prompt,
                reasoning_effort,
                image_paths: images,
            }
            .resolve(&config)?;
            let id = request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            submit_local(
                &root,
                Request::Submit {
                    id: id.clone(),
                    task,
                },
                id,
                json,
                config.remote_enabled,
            )
            .await
        }
        Command::SendMessage {
            chatgpt_url,
            message,
            message_file,
            stdin,
            reasoning_effort,
            images,
            request_id,
            json,
        } => {
            let message = read_message(message, message_file, stdin, "message")?;
            let config = Config::load(&root)?;
            let task = SendMessage {
                chatgpt_url,
                message,
                reasoning_effort,
                image_paths: images,
            }
            .resolve(&config)?;
            let id = request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            submit_local(
                &root,
                Request::SubmitFollowup {
                    id: id.clone(),
                    task,
                },
                id,
                json,
                config.remote_enabled,
            )
            .await
        }
        Command::PauseTask { chatgpt_url, json } => {
            let task = PauseTask { chatgpt_url }.resolve()?;
            let config = Config::load(&root)?;
            let id = uuid::Uuid::new_v4().to_string();
            submit_local(
                &root,
                Request::Pause {
                    id: id.clone(),
                    task,
                },
                id,
                json,
                config.remote_enabled,
            )
            .await
        }
        Command::Setup {
            profile,
            default_effort,
        } => {
            #[cfg(not(target_os = "linux"))]
            let existed = root.join("config.json").is_file();
            let mut config = Config::load(&root)?;
            if let Some(profile) = profile.as_ref() {
                let rows = crate::profiles::list(&root, &config).await?;
                config.profile = crate::profiles::select(&rows, profile)?.directory.clone();
            }
            #[cfg(not(target_os = "linux"))]
            if !existed && profile.is_none() {
                let rows = crate::profiles::list(&root, &config).await?;
                ensure!(
                    rows.len() == 1,
                    "Choose your signed-in Chrome profile: run slingring profile list, then slingring setup --profile NAME (or use Configure > Chrome Profile in the menu bar)"
                );
                config.profile = rows[0].directory.clone();
            }
            if let Some(effort) = default_effort {
                config.default_effort = Some(crate::model::effort(&effort)?);
                config.check_effort(config.default_effort.as_deref().unwrap())?;
            }
            #[cfg(not(target_os = "linux"))]
            {
                config.browser_binary =
                    crate::dependencies::resolve(&root, &config.browser_binary)?
                        .to_string_lossy()
                        .into_owned();
            }
            #[cfg(target_os = "linux")]
            {
                config.browser_binary = crate::browser_stock::PROVIDER.into();
                config.profile = crate::browser_stock::PROFILE.into();
            }
            config.save(&root)?;
            let extension = crate::browser::install_extension(&root)?;
            reload(&root).await?;
            #[cfg(not(target_os = "linux"))]
            let ready = true;
            #[cfg(target_os = "linux")]
            let ready = crate::browser_stock::profile_prepared(&root);
            output(
                json!({"ready":ready,"home":root,"profile":config.profile,"default_effort":config.default_effort,
                "extension":extension,"browser_binary":config.browser_binary}),
            )
        }
        Command::Profile { action } => {
            let mut config = Config::load(&root)?;
            let rows = crate::profiles::list(&root, &config).await?;
            match action {
                ProfileAction::List { json } => {
                    if json {
                        output(json!({"profiles": rows, "selected": config.profile}))
                    } else {
                        for profile in rows {
                            println!(
                                "{} {:<14} {}",
                                if profile.selected { "*" } else { " " },
                                profile.directory,
                                profile.name
                            );
                        }
                        Ok(())
                    }
                }
                #[cfg(target_os = "linux")]
                ProfileAction::Open => {
                    ensure!(
                        config.profile == crate::browser_stock::PROFILE
                            && config.browser_binary == crate::browser_stock::PROVIDER,
                        "run slingring setup before opening the dedicated profile"
                    );
                    crate::browser_stock::manual_profile(&root).await?;
                    output(json!({"configured":true,"profile":crate::browser_stock::PROFILE}))
                }
                ProfileAction::Use { profile } => {
                    let selected = crate::profiles::select(&rows, &profile)?;
                    ensure!(
                        status(&root).await?["active_tasks"].as_u64().unwrap_or(0) == 0,
                        "Wait for queued tasks to finish before changing the Chrome profile"
                    );
                    config.profile = selected.directory.clone();
                    #[cfg(not(target_os = "linux"))]
                    {
                        config.browser_binary =
                            crate::dependencies::resolve(&root, &config.browser_binary)?
                                .to_string_lossy()
                                .into_owned();
                    }
                    config.save(&root)?;
                    crate::browser::install_extension(&root)?;
                    reload(&root).await?;
                    output(json!({"saved":true,"profile":config.profile,"name":selected.name}))
                }
            }
        }
        Command::Browser { action } => match action {
            BrowserAction::Install => {
                let path = crate::dependencies::install(&root)?;
                output(
                    json!({"installed":true,"version":crate::dependencies::VERSION,"binary":path}),
                )
            }
            BrowserAction::Status => {
                let config = Config::load(&root)?;
                let managed = crate::dependencies::managed(&root);
                output(
                    json!({"managed":managed,"installed":managed.is_file(),"version":crate::dependencies::VERSION,
                    "configured":config.browser_binary}),
                )
            }
        },
        Command::Config { action } => {
            let mut config = Config::load(&root)?;
            match action {
                ConfigAction::Show { .. } => output(config.redacted()),
                ConfigAction::Set { key, value } => {
                    match key.replace('_', "-").as_str() {
                        "default-effort" | "default-reasoning-effort" => {
                            config.default_effort = if value.trim().is_empty()
                                || matches!(value.trim(), "site" | "default")
                            {
                                None
                            } else {
                                let effort = crate::model::effort(&value)?;
                                config.check_effort(&effort)?;
                                Some(effort)
                            };
                        }
                        "subscription" => {
                            config.set_subscription(
                                &root,
                                crate::config::Subscription::parse(&value)?,
                            )?;
                            reload(&root).await?;
                            return output(json!({"saved":true,"config":config.redacted()}));
                        }
                        "profile" | "chrome-profile" => {
                            #[cfg(target_os = "linux")]
                            ensure!(
                                value == crate::browser_stock::PROFILE,
                                "Linux uses a single dedicated Slingring profile; run slingring profile open"
                            );
                            config.profile = value;
                        }
                        "browser-binary" => {
                            #[cfg(target_os = "linux")]
                            ensure!(
                                value == crate::browser_stock::PROVIDER,
                                "Linux uses stock Chrome Stable; no alternate binary is accepted"
                            );
                            config.browser_binary = value;
                        }
                        "browser-mode" => {
                            config.browser_headed = match value.as_str() {
                                "headed" => true,
                                "headless" => false,
                                _ => anyhow::bail!("browser-mode must be headed or headless"),
                            };
                        }
                        "browser-headed" => {
                            config.browser_headed = value
                                .parse()
                                .context("browser-headed must be true or false")?;
                        }
                        "start-timeout-secs" | "timeout-secs" => {
                            config.start_timeout_secs = value.parse()?
                        }
                        "heartbeat-secs" => config.heartbeat_secs = value.parse()?,
                        _ => anyhow::bail!("unknown configuration key"),
                    }
                    config.save(&root)?;
                    reload(&root).await?;
                    output(json!({"saved":true,"config":config.redacted()}))
                }
            }
        }
        Command::Status { .. } => output(status(&root).await?),
        Command::Jobs { request_id, .. } => {
            if let Some(request_id) = request_id {
                ensure!(
                    crate::queue::valid_job_id(&request_id),
                    "invalid request ID"
                );
                let records = crate::queue::recovery_records(&root)?;
                return output(crate::queue::recovery_status(
                    records.iter().find(|job| job.id == request_id),
                    &request_id,
                ));
            }
            let rows = if daemon::running(&root).await {
                daemon::call(&root, &Request::Queue).await?
            } else {
                json!(
                    crate::queue::recovery_records(&root)?
                        .iter()
                        .map(|j| j.public())
                        .collect::<Vec<_>>()
                )
            };
            output(rows)
        }
        Command::Daemon { ephemeral } => daemon::run(root, ephemeral).await,
        Command::Service { action } => {
            match action {
                ServiceAction::Start => service::start(&root, false).await?,
                ServiceAction::Stop => service::stop(&root).await?,
                ServiceAction::Restart => {
                    service::stop(&root).await?;
                    service::start(&root, false).await?;
                }
                ServiceAction::Install => {
                    #[cfg(target_os = "macos")]
                    service::stop(&root).await?;
                    service::install(&root)?;
                }
                ServiceAction::Uninstall => {
                    service::stop(&root).await?;
                    service::uninstall()?;
                }
                ServiceAction::Status => {}
            }
            output(status(&root).await?)
        }
        Command::Login { action } => output(login::run(&root, action)?),
        Command::Remote { action } => remote(&root, action).await,
        Command::Mcp => crate::mcp_sdk::stdio_server(&root).await,
        _ => anyhow::bail!("unsupported command"),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    device_token: String,
    mcp_url: Option<String>,
}
async fn remote(root: &Path, action: RemoteAction) -> Result<()> {
    let mut config = Config::load(root)?;
    match action {
        RemoteAction::Configure { url, mcp_url, .. } => {
            let input = bounded_read(std::io::stdin(), 16384)?;
            let credentials: Credentials = if input.trim_start().starts_with('{') {
                serde_json::from_str(&input).context("invalid credentials JSON")?
            } else {
                Credentials {
                    device_token: input.trim().into(),
                    mcp_url,
                }
            };
            config.relay_url = Some(config::secure_url(&url, true)?);
            config.device_token = Some(credentials.device_token);
            config.mcp_url = credentials.mcp_url;
            config.save(root)?;
            reload(root).await?;
            output(json!({"configured":true,"enabled":config.remote_enabled}))
        }
        RemoteAction::On => {
            config.remote_enabled = true;
            config.save(root)?;
            service::start(root, false).await?;
            reload(root).await?;
            output(json!({"enabled":true,"state":"connecting"}))
        }
        RemoteAction::Off => {
            config.remote_enabled = false;
            config.save(root)?;
            reload(root).await?;
            output(json!({"enabled":false,"idle_service_will_exit":true}))
        }
        RemoteAction::Status => output(status(root).await?),
        RemoteAction::Url => {
            println!(
                "{}",
                config
                    .mcp_url
                    .context("remote MCP URL has not been configured")?
            );
            Ok(())
        }
    }
}

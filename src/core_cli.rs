use crate::{
    cli::{Command, ConfigAction, RemoteAction, ServiceAction},
    config::{self, Config},
    daemon::{self, Request},
    model::{MAX_JSON, MAX_PROMPT, NewTask},
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
    let jobs = crate::queue::records(root)?;
    Ok(
        json!({"running":false,"installed":service::installed(),"remote_enabled":config.remote_enabled,
        "remote_state":if config.remote_enabled {"offline"} else {"disabled"},"browser_busy":false,
        "active_tasks":0,"latest_job":jobs.back().map(|j|j.public()),"config":config.redacted()}),
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
            let prompt = match (prompt, prompt_file, stdin) {
                (Some(p), None, false) => p,
                (None, Some(p), false) if p != "-" => {
                    bounded_read(std::fs::File::open(p)?, MAX_PROMPT)?
                }
                (None, Some(_), false) | (None, None, true) => {
                    bounded_read(std::io::stdin(), MAX_PROMPT)?
                }
                _ => anyhow::bail!("provide a prompt, --prompt-file FILE, or --stdin"),
            };
            let config = Config::load(&root)?;
            let task = NewTask {
                prompt,
                reasoning_effort,
                image_paths: images,
            }
            .resolve(&config)?;
            let id = request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            service::start(&root, !config.remote_enabled).await?;
            match daemon::call(
                &root,
                &Request::Submit {
                    id: id.clone(),
                    task,
                },
            )
            .await
            {
                Ok(value) => {
                    if json {
                        output(value)
                    } else {
                        println!("{}", value["url"].as_str().context("missing task URL")?);
                        Ok(())
                    }
                }
                Err(error) => {
                    if json {
                        let job = crate::queue::records(&root)
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
        Command::Setup {
            profile,
            default_effort,
        } => {
            let mut config = Config::load(&root)?;
            if let Some(profile) = profile {
                config.profile = profile;
            }
            if let Some(effort) = default_effort {
                config.default_effort = Some(crate::model::effort(&effort)?);
            }
            if !Path::new(&config.browser_binary).is_absolute() {
                config.browser_binary =
                    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                        .map(|directory| directory.join(&config.browser_binary))
                        .find(|path| path.is_file())
                        .context("agent-browser-browse is not installed in PATH")?
                        .to_string_lossy()
                        .into_owned();
            }
            config.save(&root)?;
            let extension = crate::browser::install_extension(&root)?;
            reload(&root).await?;
            output(
                json!({"ready":true,"home":root,"profile":config.profile,"default_effort":config.default_effort,
                "extension":extension,"browser_binary":config.browser_binary}),
            )
        }
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
                                Some(crate::model::effort(&value)?)
                            };
                        }
                        "profile" | "chrome-profile" => config.profile = value,
                        "browser-binary" => config.browser_binary = value,
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
        Command::Jobs { .. } => {
            let rows = if daemon::running(&root).await {
                daemon::call(&root, &Request::Queue).await?
            } else {
                json!(
                    crate::queue::records(&root)?
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
        Command::Remote { action } => remote(&root, action).await,
        Command::Mcp => stdio_mcp(&root).await,
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
async fn stdio_mcp(root: &Path) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    loop {
        if input.fill_buf().await?.is_empty() {
            break;
        }
        let bytes = daemon::line(&mut input, MAX_JSON).await?;
        let response = match serde_json::from_slice::<Value>(&bytes) {
            Err(_) => Some(crate::mcp::error(Value::Null, -32700, "Parse error")),
            Ok(request) => match crate::mcp::metadata(&request) {
                Some(response) => response,
                None => {
                    service::start(root, true).await?;
                    let reply = daemon::call(
                        root,
                        &Request::Mcp {
                            body: request,
                            key: uuid::Uuid::new_v4().to_string(),
                        },
                    )
                    .await?;
                    reply.get("response").filter(|v| !v.is_null()).cloned()
                }
            },
        };
        if let Some(value) = response {
            let mut bytes = serde_json::to_vec(&value)?;
            bytes.push(b'\n');
            output.write_all(&bytes).await?;
            output.flush().await?;
        }
    }
    Ok(())
}

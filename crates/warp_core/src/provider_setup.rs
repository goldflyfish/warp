//! Local provider configuration shared by native Settings and attended chat.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Write as _};
use std::os::windows::io::AsRawHandle as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use futures::channel::oneshot;
use instant::Instant;
use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
use reqwest::Method;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::windows_package;

#[path = "provider_verification.rs"]
mod verification;

pub const CONNECTION_CHECK_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn offload<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    let (send, receive) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = send.send(work());
    });
    receive
        .await
        .context("The provider operation was interrupted")?
}

pub fn profile_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    if let Some(profile) = windows_package::profile_dir() {
        return Ok(profile.to_owned());
    }
    let base =
        directories::BaseDirs::new().context("Your application data folder is unavailable")?;
    Ok(base.data_local_dir().join("MyWarp"))
}

pub fn configuration_file(profile: &Path) -> PathBuf {
    windows_package::provider_configuration_file(profile)
}

fn configuration(profile: &Path) -> Result<Value> {
    let text = fs::read_to_string(configuration_file(profile))?;
    let value = jsonc_parser::parse_to_serde_value::<Value>(&text, &ParseOptions::default())?;
    if !value.is_object() {
        bail!("The provider configuration must be an object");
    }
    Ok(value)
}

fn patch_configuration(profile: &Path, patch: Value) -> Result<()> {
    let path = configuration_file(profile);
    let original = fs::read_to_string(&path)?;
    let root = CstRootNode::parse(&original, &ParseOptions::default())?;
    let object = root
        .object_value()
        .context("The provider configuration must be an object")?;
    let patch = patch
        .as_object()
        .context("The provider update must be an object")?;
    patch_object(&object, patch);
    let mut temporary = tempfile::NamedTempFile::new_in(
        path.parent()
            .context("The provider folder is unavailable")?,
    )?;
    temporary.write_all(root.to_string().as_bytes())?;
    if fs::read_to_string(&path)? != original {
        bail!("The configuration changed during editing. Refresh and try again.");
    }
    temporary
        .persist(path)
        .context("The provider configuration could not be saved")?;
    Ok(())
}

fn patch_object(object: &CstObject, patch: &serde_json::Map<String, Value>) {
    for (name, value) in patch {
        if let Value::Object(properties) = value {
            patch_object(&object.object_value_or_set(name), properties);
        } else if value.is_null() {
            if let Some(property) = object.get(name) {
                property.remove();
            }
        } else if let Some(property) = object.get(name) {
            property.set_value(config_value(value));
        } else {
            object.append(name, config_value(value));
        }
    }
}

fn config_value(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(*value),
        Value::Number(value) => CstInputValue::Number(value.to_string()),
        Value::String(value) => CstInputValue::String(value.clone()),
        Value::Array(values) => CstInputValue::Array(values.iter().map(config_value).collect()),
        Value::Object(values) => CstInputValue::Object(
            values
                .iter()
                .map(|(name, value)| (name.clone(), config_value(value)))
                .collect(),
        ),
    }
}

fn read_api_auth(profile: &Path) -> Value {
    fs::read(windows_package::runtime_dir(profile).join("providers/data/opencode/auth.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn has_api_auth(auth: &Value, id: &str) -> bool {
    auth[id]["type"] == "api"
        && auth[id]["key"]
            .as_str()
            .is_some_and(|key| !key.trim().is_empty())
}

pub fn api_key_configuration_file(profile: &Path, id: &str) -> Result<PathBuf> {
    validate_provider_id(id)?;
    let config = configuration_file(profile);
    let text =
        fs::read_to_string(&config).context("The provider configuration could not be opened")?;
    let root = CstRootNode::parse(&text, &ParseOptions::default())
        .context("The provider configuration could not be read")?;
    let key = root
        .object_value()
        .and_then(|value| value.object_value("provider"))
        .and_then(|value| value.object_value(id))
        .and_then(|value| value.object_value("options"))
        .and_then(|value| value.get("apiKey"))
        .and_then(|value| value.value())
        .and_then(|value| value.as_string_lit())
        .and_then(|value| value.decoded_value().ok());
    if let Some(key) = key.filter(|key| !key.trim().is_empty()) {
        if let Some(path) = key
            .strip_prefix("{file:")
            .and_then(|value| value.strip_suffix('}'))
        {
            let path = PathBuf::from(path);
            let path = if path.is_absolute() {
                path
            } else {
                config
                    .parent()
                    .context("The provider folder is unavailable")?
                    .join(path)
            };
            if !path.is_file() {
                bail!("The saved API key file is missing. Save a replacement key in AI providers.");
            }
            return Ok(path);
        }
        return Ok(config);
    }
    if has_api_auth(&read_api_auth(profile), id) {
        return Ok(windows_package::runtime_dir(profile).join("providers/data/opencode/auth.json"));
    }
    Ok(config)
}

pub fn runtime_executable() -> PathBuf {
    #[cfg(windows)]
    if let Some(runtime) = crate::paths::bundled_resources_dir()
        .and_then(|resources| {
            resources
                .parent()
                .map(|p| p.join("tools/opencode/opencode.exe"))
        })
        .filter(|path| path.is_file())
    {
        return runtime;
    }
    PathBuf::from("opencode")
}

#[derive(Clone, Default)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub connected: bool,
    pub has_api_key: bool,
    pub models: BTreeMap<String, String>,
    pub endpoint: String,
    pub credential: String,
    pub oauth: Vec<(usize, String)>,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub verified: BTreeSet<(String, String)>,
    pub providers: Vec<ProviderInfo>,
    pub default_model: String,
    pub agent: String,
    pub agents: BTreeMap<String, String>,
    pub custom_agents: BTreeMap<String, Value>,
    pub mcp: BTreeMap<String, Value>,
    pub instructions: Vec<String>,
}

pub struct ProviderServer {
    client: reqwest::blocking::Client,
    base: String,
    password: String,
    profile: PathBuf,
    child: parking_lot::Mutex<std::process::Child>,
    executable: PathBuf,
    port: u16,
}

impl Drop for ProviderServer {
    fn drop(&mut self) {
        let child = self.child.get_mut();
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl ProviderServer {
    pub fn start(profile: PathBuf, executable: PathBuf) -> Result<Arc<Self>> {
        Self::start_inner(profile, executable, true)
    }

    fn start_inner(
        profile: PathBuf,
        executable: PathBuf,
        import_existing: bool,
    ) -> Result<Arc<Self>> {
        windows_package::prepare_opencode_profile(&profile)?;
        // An unavailable old key must not prevent opening Settings to replace it.
        if import_existing {
            let _ = windows_package::import_existing_opencode_profile(&profile);
        }
        let password = Uuid::new_v4().to_string();
        let socket = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let port = socket.local_addr()?.port();
        drop(socket);
        let child = Self::launch(&profile, &executable, port, &password)?;
        let server = Arc::new(Self {
            client: reqwest::blocking::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(90))
                .pool_max_idle_per_host(0)
                .build()?,
            base: format!("http://127.0.0.1:{port}"),
            password,
            profile,
            child: parking_lot::Mutex::new(child),
            executable,
            port,
        });
        server.request(Method::GET, "/global/health", None)?;
        Ok(server)
    }

    fn launch(
        profile: &Path,
        executable: &Path,
        port: u16,
        password: &str,
    ) -> Result<std::process::Child> {
        let runtime = windows_package::runtime_dir(profile);
        let workspace = runtime.join("provider-check");
        fs::create_dir_all(&workspace)?;
        fs::create_dir_all(runtime.join("logs"))?;
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(runtime.join("logs/provider-runtime.log"))?;
        let mut command = command::blocking::Command::new(executable);
        command.creation_flags(0);
        command
            .args([
                "serve",
                "--hostname",
                "127.0.0.1",
                "--port",
                &port.to_string(),
            ])
            .envs(windows_package::opencode_environment(profile))
            .env("OPENCODE_SERVER_USERNAME", "mywarp")
            .env("OPENCODE_SERVER_PASSWORD", password)
            .env(
                "OPENCODE_CONFIG_CONTENT",
                r#"{"autoupdate":false,"share":"disabled"}"#,
            )
            .current_dir(&workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(log);
        let mut child = command.spawn().context("The bundled AI runtime could not start. Check that tools/opencode/opencode.exe is present in your MyWarp folder.")?;

        let job = match command::windows::JobObject::new()
            .kill_children_on_close()
            .assign_process(child.as_raw_handle() as isize)
            .create_scoped()
        {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error)
                    .context("Could not manage the provider runtime process lifetime");
            }
        };
        let output = child
            .stdout
            .take()
            .context("The AI runtime did not provide startup output")?;
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let _job = job;
            for line in BufReader::new(output)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if line.contains(&format!("http://127.0.0.1:{port}")) {
                    let _ = send.send(());
                }
            }
        });
        if receive.recv_timeout(Duration::from_secs(45)).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "The AI runtime did not start. Its diagnostic log is in the profile's logs/provider-runtime.log file. You can retry after correcting the provider configuration."
            );
        }
        Ok(child)
    }

    pub fn restart(&self) -> Result<()> {
        let mut child = self.child.lock();
        let _ = child.kill();
        let _ = child.wait();
        *child = Self::launch(&self.profile, &self.executable, self.port, &self.password)?;
        Ok(())
    }

    fn request(&self, method: Method, route: &str, body: Option<&Value>) -> Result<Value> {
        self.request_with_timeout(method, route, body, Duration::from_secs(90))
    }

    fn request_with_timeout(
        &self,
        method: Method,
        route: &str,
        body: Option<&Value>,
        timeout: Duration,
    ) -> Result<Value> {
        let mut request = self
            .client
            .request(method, format!("{}{route}", self.base))
            .timeout(timeout)
            .basic_auth("mywarp", Some(&self.password));
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .context("The local AI runtime stopped responding. Retry the connection.")?;
        let status = response.status();
        if !status.is_success() {
            bail!(
                "The provider operation failed (HTTP {}). Check the credentials, endpoint and model, then try again.",
                status.as_u16()
            );
        }
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        response
            .json()
            .context("The AI runtime returned an unreadable response")
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let providers = self.request(Method::GET, "/provider", None)?;
        let config = configuration(&self.profile)?;
        let methods = self.request(Method::GET, "/provider/auth", None)?;
        let mut result = snapshot(&providers, &config, &methods)?;
        result.verified = verification::load(&self.profile, &config, &providers);
        if let Some(mode) = runtime_overrides(&self.profile)?["default_agent"].as_str() {
            result.agent = mode.into();
        }
        let auth = read_api_auth(&self.profile);
        for provider in &mut result.providers {
            if provider.has_api_key {
                continue;
            }
            if has_api_auth(&auth, &provider.id) {
                provider.has_api_key = true;
                provider.credential = "API key in the provider auth file".into();
            } else if auth[&provider.id]["type"] == "oauth" {
                provider.credential =
                    "Provider sign-in in the auth file; connection not tested".into();
            }
        }
        let agents = self.request(Method::GET, "/agent", None)?;
        result.custom_agents = config["agent"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        result.agents = agents
            .as_array()
            .into_iter()
            .flatten()
            .filter(|agent| agent["mode"] != "subagent" && agent["hidden"] != true)
            .filter_map(|agent| {
                agent["name"].as_str().map(|name| {
                    (
                        name.to_owned(),
                        agent["description"].as_str().unwrap_or_default().to_owned(),
                    )
                })
            })
            .collect();
        Ok(result)
    }

    pub fn patch(&self, patch: Value) -> Result<Snapshot> {
        patch_configuration(&self.profile, patch)?;
        self.restart()?;
        self.snapshot()
    }

    pub fn remove_entry(&self, section: &str, name: &str) -> Result<Snapshot> {
        if !matches!(section, "agent" | "mcp") {
            bail!("This configuration section cannot be removed here.");
        }
        validate_provider_id(name)?;
        let path = configuration_file(&self.profile);
        let original = fs::read_to_string(&path)?;
        let root = CstRootNode::parse(&original, &ParseOptions::default())
            .context("The provider configuration could not be read for editing")?;
        let object = root
            .object_value()
            .context("The provider configuration must be an object")?;
        if let Some(entry) = object
            .object_value(section)
            .and_then(|value| value.get(name))
        {
            entry.remove();
            let parent = path
                .parent()
                .context("The provider configuration folder is unavailable")?;
            let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
            temporary.write_all(root.to_string().as_bytes())?;
            if fs::read_to_string(&path)? != original {
                bail!("The configuration changed during editing. Refresh and try again.");
            }
            temporary
                .persist(&path)
                .context("The configuration change could not be saved")?;
            self.restart()?;
        }
        self.snapshot()
    }

    pub fn save_provider(
        &self,
        id: &str,
        key: &str,
        endpoint: &str,
        custom_model: &str,
    ) -> Result<Snapshot> {
        validate_provider_id(id)?;
        validate_endpoint(endpoint)?;
        let mut config = configuration(&self.profile)?;
        let mut provider = json!({"options":{}});
        if !endpoint.trim().is_empty() {
            provider["options"]["baseURL"] = endpoint.trim().into();
        } else {
            provider["options"]["baseURL"] = Value::Null;
        }
        if !key.trim().is_empty() && (key.trim().contains('\n') || key.trim().contains('\r')) {
            bail!("The API key must be a single line.");
        }
        if id.starts_with("custom-")
            && (endpoint.trim().is_empty() || custom_model.trim().is_empty())
        {
            bail!("Custom providers need an endpoint and a model ID.");
        }
        let previous_key = self.stored_key_reference(id)?;
        let mut created = None;
        if !key.trim().is_empty() {
            let directory =
                windows_package::configuration_dir(&self.profile).join("providers/keys");
            fs::create_dir_all(&directory)?;
            let name = format!("mywarp-{id}-{}.key", Uuid::new_v4());
            let path = directory.join(&name);
            fs::write(&path, key.trim())
                .context("The API key could not be saved to this profile")?;
            provider["options"]["apiKey"] = format!("{{file:./keys/{name}}}").into();
            created = Some(path);
        }
        if id.starts_with("custom-") {
            if endpoint.trim().is_empty() || custom_model.trim().is_empty() {
                bail!("Custom providers need an endpoint and a model ID.");
            }
            provider["npm"] = "@ai-sdk/openai-compatible".into();
            provider["name"] = "Custom endpoint".into();
            provider["models"] = json!({custom_model.trim(): {"name":custom_model.trim()}});
        }
        let disabled = config["disabled_providers"]
            .as_array_mut()
            .map(|items| {
                items.retain(|item| item.as_str() != Some(id));
                items.clone()
            })
            .unwrap_or_default();
        let result = self.patch(json!({"provider":{id:provider},"disabled_providers":disabled}));
        if let Err(error) = result {
            if let Some(path) = created {
                let current = fs::read_to_string(
                    self.profile.join("Configurations/providers/opencode.jsonc"),
                )
                .unwrap_or_default();
                if path
                    .file_name()
                    .is_some_and(|name| !current.contains(&*name.to_string_lossy()))
                {
                    let _ = fs::remove_file(path);
                }
            }
            return Err(error);
        }
        if created.is_some() {
            self.remove_owned_key(previous_key.as_deref());
        }
        self.snapshot()
    }

    fn stored_key_reference(&self, id: &str) -> Result<Option<String>> {
        let text = fs::read_to_string(configuration_file(&self.profile))?;
        let root = CstRootNode::parse(&text, &ParseOptions::default())
            .context("The provider configuration could not be read")?;
        Ok(root
            .object_value()
            .and_then(|v| v.object_value("provider"))
            .and_then(|v| v.object_value(id))
            .and_then(|v| v.object_value("options"))
            .and_then(|v| v.get("apiKey"))
            .and_then(|v| v.value())
            .and_then(|v| v.as_string_lit())
            .and_then(|v| v.decoded_value().ok()))
    }

    pub fn disconnect(&self, id: &str) -> Result<Snapshot> {
        validate_provider_id(id)?;
        let config = configuration(&self.profile)?;
        let mut disabled = config["disabled_providers"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if !disabled.iter().any(|item| item.as_str() == Some(id)) {
            disabled.push(id.into());
        }
        let previous_key = self.stored_key_reference(id)?;
        self.request(Method::DELETE, &format!("/auth/{id}"), None)?;
        let result = self.patch(
            json!({"provider":{id:{"options":{"apiKey":null}}},"disabled_providers":disabled}),
        );
        if result.is_ok() {
            self.remove_owned_key(previous_key.as_deref());
        }
        result
    }

    fn remove_owned_key(&self, reference: Option<&str>) {
        let Some(name) = reference
            .and_then(|r| r.strip_prefix("{file:./keys/"))
            .and_then(|r| r.strip_suffix('}'))
        else {
            return;
        };
        if name.starts_with("mywarp-")
            && name.ends_with(".key")
            && name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.'))
        {
            let _ = fs::remove_file(
                self.profile
                    .join("Configurations/providers/keys")
                    .join(name),
            );
        }
    }

    pub fn authorize(&self, id: &str, method: usize) -> Result<Value> {
        validate_provider_id(id)?;
        self.request(
            Method::POST,
            &format!("/provider/{id}/oauth/authorize"),
            Some(&json!({"method":method})),
        )
    }

    pub fn complete_authorization(&self, id: &str, method: usize, code: &str) -> Result<Snapshot> {
        validate_provider_id(id)?;
        self.request(
            Method::POST,
            &format!("/provider/{id}/oauth/callback"),
            Some(&json!({"method":method,"code":code})),
        )?;
        let config = configuration(&self.profile)?;
        let disabled: Vec<_> = config["disabled_providers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|value| value.as_str() != Some(id))
            .cloned()
            .collect();
        let previous_key = self.stored_key_reference(id)?;
        let result = self.patch(
            json!({"provider":{id:{"options":{"apiKey":null}}},"disabled_providers":disabled}),
        );
        if result.is_ok() {
            self.remove_owned_key(previous_key.as_deref());
        }
        result
    }

    pub fn test_connection(&self, id: &str, model: &str) -> Result<()> {
        self.test_connection_cancellable(id, model, &AtomicBool::new(false))
    }

    pub fn test_connection_cancellable(
        &self,
        id: &str,
        model: &str,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        validate_provider_id(id)?;
        if model.trim().is_empty() {
            bail!("Choose a model before testing this connection.");
        }
        verification::save(&self.profile, id, model, None)?;
        let before = self.verification_fingerprint(id, model)?;
        let deadline = Instant::now() + CONNECTION_CHECK_TIMEOUT;
        let request = |method, route: &str, body: Option<&Value>| {
            if cancelled.load(Ordering::Relaxed) {
                bail!("Connection test cancelled.");
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "The connection test timed out after 30 seconds. Check your network, endpoint and model availability, then retry."
                );
            }
            self.request_with_timeout(method, route, body, remaining.min(Duration::from_secs(3)))
        };
        let session = request(
            Method::POST,
            "/session",
            Some(
                &json!({"title":"MyWarp connection check","permission":[{"permission":"*","pattern":"*","action":"deny"}]}),
            ),
        )?;
        let session_id = session["id"]
            .as_str()
            .context("The runtime could not create a connection check")?;
        let result = (|| {
            request(
                Method::POST,
                &format!("/session/{session_id}/prompt_async"),
                Some(
                    &json!({"model":{"providerID":id,"modelID":model},"agent":"plan","parts":[{"type":"text","text":"Reply with exactly MYWARP_CONNECTION_OK. Do not use any tools."}]}),
                ),
            )?;
            loop {
                let statuses = request(Method::GET, "/session/status", None)?;
                // Billing failures can arrive as 429s and otherwise trigger long runtime retries.
                if statuses[session_id]["type"] == "retry" {
                    return Err(connection_failure(&statuses[session_id], id));
                }
                let messages =
                    request(Method::GET, &format!("/session/{session_id}/message"), None)?;
                for message in messages.as_array().into_iter().flatten().rev() {
                    if message["info"]["role"] != "assistant" {
                        continue;
                    }
                    if !message["info"]["error"].is_null() {
                        return Err(connection_failure(&message["info"]["error"], id));
                    }
                    if !message["info"]["time"]["completed"].is_null() {
                        return validate_check_response(message);
                    }
                }
                std::thread::park_timeout(Duration::from_millis(150));
            }
        })();
        self.cleanup_connection_check(session_id);
        result?;
        if cancelled.load(Ordering::Relaxed) {
            bail!("Connection test cancelled.");
        }
        let after = self.verification_fingerprint(id, model)?;
        if before != after {
            bail!(
                "Configuration changed during the connection test. Test the updated connection again."
            );
        }
        verification::save(&self.profile, id, model, Some(after))
    }

    fn verification_fingerprint(&self, id: &str, model: &str) -> Result<String> {
        let providers = self.request(Method::GET, "/provider", None)?;
        let provider = providers["all"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|provider| provider["id"] == id)
            .context("The selected provider is unavailable")?;
        verification::fingerprint(
            &self.profile,
            id,
            model,
            &configuration(&self.profile)?,
            provider,
        )
    }

    pub fn background_model(&self) -> Result<String> {
        let config = configuration(&self.profile)?;
        Ok(config["model"].as_str().unwrap_or_default().to_owned())
    }

    pub fn generate_text(
        &self,
        id: &str,
        model: &str,
        title: &str,
        system: &str,
        prompt: &str,
        cancelled: &AtomicBool,
    ) -> Result<String> {
        validate_provider_id(id)?;
        if model.trim().is_empty() {
            bail!("Choose a default model in AI providers before using background AI.");
        }
        let deadline = Instant::now() + CONNECTION_CHECK_TIMEOUT;
        let request = |method, route: &str, body: Option<&Value>| {
            if cancelled.load(Ordering::Relaxed) {
                bail!("Background generation cancelled.");
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "Background generation timed out after 30 seconds. Check your network, endpoint and model availability, then retry."
                );
            }
            self.request_with_timeout(method, route, body, remaining.min(Duration::from_secs(3)))
        };
        let session = request(
            Method::POST,
            "/session",
            Some(
                &json!({"title":title,"permission":[{"permission":"*","pattern":"*","action":"deny"}]}),
            ),
        )?;
        let session_id = session["id"]
            .as_str()
            .context("The runtime could not create a background session")?;
        let result = (|| {
            request(
                Method::POST,
                &format!("/session/{session_id}/prompt_async"),
                Some(
                    &json!({"model":{"providerID":id,"modelID":model},"agent":"plan","system":system,"tools":{"*":false},"parts":[{"type":"text","text":prompt}]}),
                ),
            )?;
            loop {
                let statuses = request(Method::GET, "/session/status", None)?;
                // Billing failures can arrive as 429s and otherwise trigger long runtime retries.
                if statuses[session_id]["type"] == "retry" {
                    return Err(connection_failure(&statuses[session_id], id));
                }
                let messages =
                    request(Method::GET, &format!("/session/{session_id}/message"), None)?;
                for message in messages.as_array().into_iter().flatten().rev() {
                    if message["info"]["role"] != "assistant" {
                        continue;
                    }
                    if !message["info"]["error"].is_null() {
                        return Err(connection_failure(&message["info"]["error"], id));
                    }
                    if !message["info"]["time"]["completed"].is_null() {
                        let text = message["parts"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|part| part["type"] == "text")
                            .filter_map(|part| part["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        if text.trim().is_empty() {
                            bail!("The provider returned an empty response.");
                        }
                        return Ok(text);
                    }
                }
                std::thread::park_timeout(Duration::from_millis(150));
            }
        })();
        self.cleanup_connection_check(session_id);
        result
    }

    fn cleanup_connection_check(&self, session_id: &str) {
        let client = self.client.clone();
        let base = self.base.clone();
        let password = self.password.clone();
        let session_id = session_id.to_owned();
        std::thread::spawn(move || {
            for (method, suffix) in [(Method::POST, "/abort"), (Method::DELETE, "")] {
                let _ = client
                    .request(method, format!("{base}/session/{session_id}{suffix}"))
                    .basic_auth("mywarp", Some(&password))
                    .timeout(Duration::from_secs(2))
                    .send();
            }
        });
    }

    pub fn save_instructions(&self, text: &str) -> Result<Snapshot> {
        let file = windows_package::configuration_dir(&self.profile).join("agent-instructions.md");
        fs::write(&file, text).context("The agent instructions could not be saved")?;
        let config = configuration(&self.profile)?;
        let mut paths = config["instructions"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let file = windows_package::PERSONAL_INSTRUCTIONS_REFERENCE;
        if !paths.iter().any(|p| p.as_str() == Some(file)) {
            paths.push(file.into());
        }
        self.patch(json!({"instructions":paths}))
    }
}

fn validate_check_response(response: &Value) -> Result<()> {
    let has_text = response["parts"].as_array().is_some_and(|parts| {
        parts.iter().any(|p| {
            p["type"] == "text"
                && p["text"]
                    .as_str()
                    .is_some_and(|text| !text.trim().is_empty())
        })
    });
    if !has_text {
        bail!("The model did not return a response. Check its availability and try again.");
    }
    Ok(())
}

fn connection_failure(error: &Value, provider: &str) -> anyhow::Error {
    let description = error.to_string().to_ascii_lowercase();
    let status = error["data"]["statusCode"].as_u64();
    let message = if description.contains("insufficient balance")
        || description.contains("no resource package")
        || description.contains("1113")
    {
        if provider == "zai-coding-plan" {
            "Z.AI reports insufficient balance or no eligible resource package (code 1113). Check that this key belongs to an active Coding Plan with access to the selected model. Create a Coding Plan key under Personal Coding Plan > Plan Overview."
        } else {
            "The provider reports insufficient balance or no eligible resource package. Check its billing and the API key's plan. Z.AI Coding Plan subscribers should select Z.AI Coding Plan rather than Z.AI Standard API."
        }
    } else {
        match status {
            Some(401 | 403) => {
                "Authentication was rejected. Replace this provider's API key or sign in again."
            }
            Some(402) => "The provider requires available credit or a different billing plan.",
            Some(429) => {
                "The provider is rate limiting this connection. Wait briefly and try again."
            }
            Some(404) => "The selected model was not found. Check the model ID and endpoint.",
            _ => {
                "The provider could not complete the connection test. Check the selected model, credentials and quota, then retry. Automatic retries have been stopped."
            }
        }
    };
    anyhow::anyhow!(message)
}

fn snapshot(providers: &Value, config: &Value, methods: &Value) -> Result<Snapshot> {
    let all = providers["all"]
        .as_array()
        .context("The provider directory is unavailable")?;
    let connected = providers["connected"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut result = Snapshot {
        default_model: config["model"].as_str().unwrap_or_default().into(),
        agent: config["default_agent"].as_str().unwrap_or("build").into(),
        instructions: config["instructions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        mcp: config["mcp"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        ..Default::default()
    };
    for provider in all {
        let Some(id) = provider["id"].as_str() else {
            continue;
        };
        let options = &config["provider"][id]["options"];
        result.providers.push(ProviderInfo {
            id: id.into(),
            name: match id {
                "zai-coding-plan" => "Z.AI Coding Plan",
                "zai" => "Z.AI Standard API",
                _ => provider["name"].as_str().unwrap_or(id),
            }
            .into(),
            connected: connected.iter().any(|value| value.as_str() == Some(id)),
            has_api_key: options["apiKey"]
                .as_str()
                .is_some_and(|key| !key.trim().is_empty()),
            models: provider["models"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(id, model)| (id.clone(), model["name"].as_str().unwrap_or(id).into()))
                .collect(),
            endpoint: options["baseURL"]
                .as_str()
                .unwrap_or(match id {
                    "zai-coding-plan" => "https://api.z.ai/api/coding/paas/v4",
                    "zai" => "https://api.z.ai/api/paas/v4",
                    _ => "",
                })
                .into(),
            credential: if options["apiKey"]
                .as_str()
                .is_some_and(|key| !key.trim().is_empty())
            {
                "API key in the provider configuration"
            } else if connected.iter().any(|v| v.as_str() == Some(id)) {
                "Detected by the runtime; no API key saved in this profile"
            } else {
                "No credentials connected"
            }
            .into(),
            oauth: methods[id]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .filter(|(_, v)| {
                    v["type"] == "oauth" && v["prompts"].as_array().is_none_or(Vec::is_empty)
                })
                .map(|(index, value)| (index, value["label"].as_str().unwrap_or("Sign in").into()))
                .collect(),
        });
    }
    result
        .providers
        .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(result)
}

pub fn validate_provider_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 100
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        bail!("Choose a valid provider identifier.");
    }
    Ok(())
}

pub fn validate_endpoint(endpoint: &str) -> Result<()> {
    if endpoint.trim().is_empty() {
        return Ok(());
    }
    let url = url::Url::parse(endpoint.trim()).context("Enter a complete endpoint URL")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("Use an endpoint URL without embedded credentials, a query or a fragment.");
    }
    let local = matches!(
        url.host_str(),
        Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
    );
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        bail!("Use HTTPS for providers, or HTTP for a local model server.");
    }
    if crate::mywarp::is_warp_service(&url) {
        bail!("Warp service endpoints are disabled in MyWarp.");
    }
    Ok(())
}

pub fn read_tool_policy() -> Result<String> {
    Ok(read_agent_overrides(&profile_dir()?)?["tool_policy"]
        .as_str()
        .unwrap_or("ask")
        .into())
}

pub fn write_agent_override(name: &str, value: Value) -> Result<()> {
    let profile = profile_dir()?;
    let mut config = read_agent_overrides(&profile)?;
    config[name] = value;
    let directory = windows_package::configuration_dir(&profile);
    fs::create_dir_all(&directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;

    temporary.write_all(&serde_json::to_vec_pretty(&config)?)?;
    temporary
        .persist(directory.join("agent-preferences.json"))
        .context("Could not save the agent preferences")?;
    Ok(())
}

pub fn write_tool_policy(policy: &str) -> Result<Value> {
    let permission = tool_permissions(policy)?;
    write_agent_override("tool_policy", policy.into())?;
    Ok(permission)
}

fn tool_permissions(policy: &str) -> Result<Value> {
    match policy {
        "ask" => Ok(json!("ask")),
        "review" => {
            Ok(json!({"*":"ask","read":"allow","glob":"allow","grep":"allow","list":"allow"}))
        }
        "readonly" => {
            Ok(json!({"*":"deny","read":"allow","glob":"allow","grep":"allow","list":"allow"}))
        }
        _ => bail!("Choose a supported tool permission policy."),
    }
}

fn read_agent_overrides(profile: &Path) -> Result<Value> {
    match fs::read(windows_package::configuration_dir(profile).join("agent-preferences.json")) {
        Ok(bytes)=>serde_json::from_slice(&bytes).context("The agent preferences are invalid. Correct agent-preferences.json before reconnecting."),
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(json!({})),
        Err(error)=>Err(error).context("Could not read the agent preferences"),
    }
}

pub fn runtime_overrides(profile: &Path) -> Result<Value> {
    let preferences = read_agent_overrides(profile)?;
    let mut config = json!({"permission":tool_permissions(preferences["tool_policy"].as_str().unwrap_or("ask"))?,"autoupdate":false,"share":"disabled"});
    if let Some(mode) = preferences["default_agent"].as_str() {
        validate_provider_id(mode).context("The saved agent profile is invalid")?;
        config["default_agent"] = mode.into();
    }
    Ok(config)
}

#[cfg(test)]
#[path = "provider_setup_tests.rs"]
mod tests;

use crate::{
    activity,
    api::{Api, DownloadRequest},
    config::{self, Config, Installation},
    discovery,
    installer::{self, PreparedInstall},
    model::{Account, Game, Package, Progress, Region, Session},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{Emitter, State};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

type CommandResult<T> = std::result::Result<T, String>;
fn error(error: anyhow::Error) -> String {
    format!("{error:#}")
}

pub struct AppState {
    api: Api,
    config_path: PathBuf,
    activity_path: PathBuf,
    data: AsyncMutex<Data>,
    operation: AsyncMutex<()>,
    cancellation: Mutex<Option<CancellationToken>>,
}
struct Data {
    config: Config,
    session: Option<Session>,
    packages: Vec<Package>,
    warning: Option<String>,
}

#[derive(Serialize)]
pub struct InstallationView {
    #[serde(flatten)]
    installation: Installation,
    title: String,
    installed_version: Option<String>,
    exists: bool,
    product: String,
}
#[derive(Serialize)]
pub struct Snapshot {
    config: Config,
    account: Option<Account>,
    packages: Vec<Package>,
    installations: Vec<InstallationView>,
    warning: Option<String>,
}

impl AppState {
    pub fn new(config_path: PathBuf) -> Result<Self> {
        let config = config::load(&config_path)?;
        #[cfg(feature = "e2e")]
        let api = {
            let endpoint = std::env::var("OPENZYGOR_TEST_ENDPOINT")
                .context("An e2e build requires OPENZYGOR_TEST_ENDPOINT; use a normal build for your account")?;
            ensure!(
                endpoint.starts_with("http://127.0.0.1:"),
                "The e2e service must use IPv4 loopback"
            );
            Api::new(&endpoint)?
        };
        #[cfg(not(feature = "e2e"))]
        let api = Api::production()?;
        Ok(Self {
            api,
            activity_path: config_path.with_file_name("activity.json"),
            config_path,
            data: AsyncMutex::new(Data {
                config,
                session: None,
                packages: Vec::new(),
                warning: None,
            }),
            operation: AsyncMutex::new(()),
            cancellation: Mutex::new(None),
        })
    }
    async fn snapshot(&self) -> Result<Snapshot> {
        let data = self.data.lock().await;
        let config = data.config.clone();
        let packages = data.packages.clone();
        let account = data.session.as_ref().map(|s| s.account.clone());
        let warning = data.warning.clone();
        drop(data);
        let installations = config.installations.clone();
        let products = packages.clone();
        let views = tokio::task::spawn_blocking(move || {
            installations
                .into_iter()
                .map(|installation| {
                    let product = products
                        .iter()
                        .find(|p| p.game == installation.game.id())
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| installation.game.package().into());
                    let installed_version =
                        installer::version(&installation.addons().join(&product), &product);
                    InstallationView {
                        title: installation.game.title().into(),
                        exists: installation.path.join(installation.game.exe()).is_file(),
                        installation,
                        product,
                        installed_version,
                    }
                })
                .collect()
        })
        .await?;
        Ok(Snapshot {
            config,
            account,
            packages,
            installations: views,
            warning,
        })
    }
    async fn save(&self, config: Config) -> Result<()> {
        let path = self.config_path.clone();
        let next = config.clone();
        tokio::task::spawn_blocking(move || config::save(&path, &next)).await??;
        self.data.lock().await.config = config;
        Ok(())
    }
    async fn set_session(&self, session: Session) -> Result<()> {
        let mut config = self.data.lock().await.config.clone();
        config.username = session.account.username.clone();
        if config.remember
            && let Err(e) = store_token(&session.account.username, &session.token).await
        {
            config.remember = false;
            self.data.lock().await.warning = Some(format!(
                "Signed in for this session. The desktop keyring could not save your login: {e}"
            ));
        }
        self.save(config).await?;
        self.data.lock().await.session = Some(session);
        Ok(())
    }
    async fn refresh(&self) -> Result<()> {
        let token = self
            .data
            .lock()
            .await
            .session
            .as_ref()
            .context("Sign in to your Zygor account first")?
            .token
            .clone();
        let session = self.api.validate(&token).await?;
        self.set_session(session).await?;
        let snapshot = self.snapshot().await?;
        let mut installed: BTreeMap<String, Option<String>> = Game::ALL
            .into_iter()
            .map(|g| (g.id().into(), None))
            .collect();
        for installation in snapshot.installations {
            installed
                .entry(installation.installation.game.id().into())
                .and_modify(|value| {
                    if value.is_none() {
                        *value = installation.installed_version.clone();
                    }
                });
        }
        let token = self
            .data
            .lock()
            .await
            .session
            .as_ref()
            .context("Sign in first")?
            .token
            .clone();
        let catalogue = self
            .api
            .catalogue(&token, snapshot.config.region, &installed)
            .await?;
        let mut data = self.data.lock().await;
        if let Some(access) = catalogue.access
            && let Some(session) = data.session.as_mut()
        {
            session.account.access = access;
        }
        data.packages = catalogue.packages;
        Ok(())
    }
}

const KEYRING_SERVICE: &str = "io.github.openzygor.client";
async fn store_token(username: &str, token: &str) -> Result<()> {
    let username = username.to_owned();
    let token = Zeroizing::new(token.to_owned());
    tokio::task::spawn_blocking(move || {
        keyring::Entry::new(KEYRING_SERVICE, &username)?.set_password(&token)
    })
    .await??;
    Ok(())
}
async fn read_token(username: String) -> Result<Zeroizing<String>> {
    Ok(Zeroizing::new(
        tokio::task::spawn_blocking(move || {
            keyring::Entry::new(KEYRING_SERVICE, &username)?.get_password()
        })
        .await??,
    ))
}
async fn forget_token(username: String) -> Result<()> {
    if username.is_empty() {
        return Ok(());
    }
    let result = tokio::task::spawn_blocking(move || {
        keyring::Entry::new(KEYRING_SERVICE, &username)?.delete_credential()
    })
    .await?;
    match result {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[tauri::command]
async fn initialize(state: State<'_, Arc<AppState>>) -> CommandResult<Snapshot> {
    let _op = state
        .operation
        .try_lock()
        .map_err(|_| "Another operation is in progress")?;
    let config = state.data.lock().await.config.clone();
    if config.remember && !config.username.is_empty() && state.data.lock().await.session.is_none() {
        let result: Result<()> = async {
            let token = read_token(config.username).await?;
            state.set_session(state.api.validate(&token).await?).await?;
            state.refresh().await
        }
        .await;
        if let Err(e) = result {
            state.data.lock().await.warning = Some(format!("Could not restore your session: {e}"));
        }
    }
    state.snapshot().await.map_err(error)
}

#[tauri::command]
async fn snapshot(state: State<'_, Arc<AppState>>) -> CommandResult<Snapshot> {
    state.snapshot().await.map_err(error)
}

#[tauri::command]
async fn load_activity(state: State<'_, Arc<AppState>>) -> CommandResult<Vec<activity::Entry>> {
    let path = state.activity_path.clone();
    tokio::task::spawn_blocking(move || activity::load(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(error)
}

#[tauri::command]
async fn append_activity(
    state: State<'_, Arc<AppState>>,
    message: String,
    failed: bool,
) -> CommandResult<Vec<activity::Entry>> {
    let path = state.activity_path.clone();
    tokio::task::spawn_blocking(move || activity::append(&path, &message, failed))
        .await
        .map_err(|e| e.to_string())?
        .map_err(error)
}

#[tauri::command]
async fn clear_activity(state: State<'_, Arc<AppState>>) -> CommandResult<Vec<activity::Entry>> {
    let path = state.activity_path.clone();
    tokio::task::spawn_blocking(move || activity::clear(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(error)
}

#[tauri::command]
async fn login(
    state: State<'_, Arc<AppState>>,
    username: String,
    password: String,
    remember: bool,
) -> CommandResult<Snapshot> {
    let password = Zeroizing::new(password);
    let _op = state
        .operation
        .try_lock()
        .map_err(|_| "Another operation is in progress")?;
    let session = state.api.login(&username, &password).await.map_err(error)?;
    let mut config = state.data.lock().await.config.clone();
    if !remember && config.remember {
        forget_token(config.username.clone()).await.map_err(error)?;
    }
    config.remember = remember;
    state.save(config).await.map_err(error)?;
    state.data.lock().await.warning = None;
    state.set_session(session).await.map_err(error)?;
    if let Err(e) = state.refresh().await {
        state.data.lock().await.warning =
            Some(format!("Signed in, but could not load guides: {e}"));
    }
    state.snapshot().await.map_err(error)
}

#[tauri::command]
async fn logout(state: State<'_, Arc<AppState>>) -> CommandResult<Snapshot> {
    let _op = state
        .operation
        .try_lock()
        .map_err(|_| "Wait for the current operation to finish")?;
    let mut config = state.data.lock().await.config.clone();
    let forgotten = if config.remember {
        forget_token(config.username.clone()).await
    } else {
        Ok(())
    };
    config.remember = false;
    state.save(config).await.map_err(error)?;
    let mut data = state.data.lock().await;
    data.session = None;
    data.packages.clear();
    data.warning = forgotten
        .err()
        .map(|e| format!("Signed out. Could not remove the saved keyring entry: {e}"));
    drop(data);
    state.snapshot().await.map_err(error)
}

#[tauri::command]
async fn refresh(state: State<'_, Arc<AppState>>) -> CommandResult<Snapshot> {
    let _op = state
        .operation
        .try_lock()
        .map_err(|_| "Another operation is in progress")?;
    state.refresh().await.map_err(error)?;
    state.data.lock().await.warning = None;
    state.snapshot().await.map_err(error)
}

#[tauri::command]
async fn detect_installations(state: State<'_, Arc<AppState>>) -> CommandResult<Snapshot> {
    let _op = state
        .operation
        .try_lock()
        .map_err(|_| "Another operation is in progress")?;
    let found =
        tokio::task::spawn_blocking(|| discovery::discover(&dirs::home_dir().unwrap_or_default()))
            .await
            .map_err(|e| e.to_string())?;
    let mut config = state.data.lock().await.config.clone();
    for installation in found {
        if !config.installations.iter().any(|i| i.id == installation.id) {
            config.installations.push(installation);
        }
    }
    state.save(config).await.map_err(error)?;
    state.snapshot().await.map_err(error)
}

#[tauri::command]
async fn add_installation(
    state: State<'_, Arc<AppState>>,
    game: Game,
    path: String,
) -> CommandResult<Snapshot> {
    let _op = state
        .operation
        .try_lock()
        .map_err(|_| "Another operation is in progress")?;
    let installation = discovery::validate(game, &PathBuf::from(path)).map_err(error)?;
    let mut config = state.data.lock().await.config.clone();
    if !config.installations.iter().any(|i| i.id == installation.id) {
        config.installations.push(installation);
    }
    state.save(config).await.map_err(error)?;
    state.snapshot().await.map_err(error)
}

#[tauri::command]
async fn remove_installation(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> CommandResult<Snapshot> {
    let _op = state
        .operation
        .try_lock()
        .map_err(|_| "Another operation is in progress")?;
    let mut config = state.data.lock().await.config.clone();
    config.installations.retain(|i| i.id != id);
    state.save(config).await.map_err(error)?;
    state.snapshot().await.map_err(error)
}

#[tauri::command]
async fn settings(
    state: State<'_, Arc<AppState>>,
    region: Region,
    update_automatically: bool,
) -> CommandResult<Snapshot> {
    let _op = state
        .operation
        .try_lock()
        .map_err(|_| "Another operation is in progress")?;
    let mut config = state.data.lock().await.config.clone();
    config.region = region;
    config.update_automatically = update_automatically;
    state.save(config).await.map_err(error)?;
    state.data.lock().await.packages.clear();
    state.snapshot().await.map_err(error)
}

#[tauri::command]
async fn choose_directory() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("Choose your World of Warcraft folder")
        .pick_folder()
        .await
        .map(|f| f.path().to_string_lossy().into_owned())
}

#[tauri::command]
fn open_link(link: String) -> CommandResult<()> {
    let url = match link.as_str() {
        "account" => "https://zygorguides.com/members/manage/",
        "register" => "https://zygorguides.com/members/subscription#1m",
        "password" => "https://zygorguides.com/#forgot",
        "support" => "https://zygorguides.com/support/",
        _ => return Err("Unknown link".into()),
    };
    open::that(url).map_err(|_| "Could not open your browser".into())
}

#[tauri::command]
fn cancel_download(state: State<'_, Arc<AppState>>) {
    if let Ok(guard) = state.cancellation.lock()
        && let Some(token) = guard.as_ref()
    {
        token.cancel();
    }
}

#[tauri::command]
async fn install(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
    id: String,
) -> CommandResult<Snapshot> {
    let _op = state
        .operation
        .try_lock()
        .map_err(|_| "Another operation is in progress")?;
    let cancel = CancellationToken::new();
    *state
        .cancellation
        .lock()
        .map_err(|_| "Cancellation state is unavailable")? = Some(cancel.clone());
    let result: Result<()> = async {
        emit(&app, &id, "Checking account access", 0, None);
        tokio::select! { _ = cancel.cancelled() => anyhow::bail!("Download cancelled"), value = state.refresh() => value? }
        let data = state.data.lock().await;
        let installation = data.config.installations.iter().find(|i| i.id == id).context("Choose a game installation first")?.clone();
        let package = data.packages.iter().find(|p| p.game == installation.game.id()).context("This edition is not offered by your account")?.clone();
        ensure!(package.status.available && !package.status.hidden, "This package is not available to your account");
        let token = data.session.as_ref().context("Sign in first")?.token.clone();
        let region = data.config.region;
        drop(data);
        emit(&app, &id, "Checking local files", 0, None);
        let work_installation = installation.clone(); let product = package.name.clone(); let work_cancel = cancel.clone();
        let prepared = tokio::task::spawn_blocking(move || PreparedInstall::prepare(&work_installation, &product, &work_cancel)).await??;
        emit(&app, &id, "Requesting guides", 0, None);
        let bytes = state.api.download(DownloadRequest { token: &token, game: installation.game, region,
            from: prepared.installed_version.as_deref(), manifest: &prepared.manifest }, &cancel,
            |current, total| emit(&app, &id, "Downloading guides", current, total)).await?;
        emit(&app, &id, "Updating guide licence", 0, None);
        let license = tokio::select! { _ = cancel.cancelled() => anyhow::bail!("Download cancelled"), value = state.api.license(&token, installation.game) => value? };
        let work_app = app.clone(); let work_id = id.clone(); let work_cancel = cancel.clone();
        tokio::task::spawn_blocking(move || prepared.apply(&bytes, &license, package.ver.as_deref(), &work_cancel,
            |current, total| emit(&work_app, &work_id, "Installing guides", current, total))).await??;
        emit(&app, &id, "Guides installed. Reload WoW to use the update.", 1, Some(1));
        Ok(())
    }.await;
    *state
        .cancellation
        .lock()
        .map_err(|_| "Cancellation state is unavailable")? = None;
    result.map_err(error)?;
    state.snapshot().await.map_err(error)
}

fn emit(app: &tauri::AppHandle, id: &str, phase: &str, current: u64, total: Option<u64>) {
    let _ = app.emit(
        "download-progress",
        Progress {
            installation_id: id.to_owned(),
            phase: phase.to_owned(),
            current,
            total,
        },
    );
}

#[tauri::command]
async fn uninstall(state: State<'_, Arc<AppState>>, id: String) -> CommandResult<Snapshot> {
    let _op = state
        .operation
        .try_lock()
        .map_err(|_| "Another operation is in progress")?;
    let view = state
        .snapshot()
        .await
        .map_err(error)?
        .installations
        .into_iter()
        .find(|i| i.installation.id == id)
        .ok_or("Unknown installation")?;
    tokio::task::spawn_blocking(move || installer::uninstall(&view.installation, &view.product))
        .await
        .map_err(|e| e.to_string())?
        .map_err(error)?;
    state.snapshot().await.map_err(error)
}

pub fn run() -> Result<()> {
    let root = std::env::var_os("OPENZYGOR_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::config_dir().map(|p| p.join("openzygor")))
        .context("Could not find the user config directory")?;
    let state = Arc::new(AppState::new(root.join("settings.json"))?);
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            initialize,
            snapshot,
            load_activity,
            append_activity,
            clear_activity,
            login,
            logout,
            refresh,
            detect_installations,
            add_installation,
            remove_installation,
            settings,
            choose_directory,
            open_link,
            cancel_download,
            install,
            uninstall
        ])
        .run(tauri::generate_context!())
        .context("Could not start OpenZygor")?;
    Ok(())
}

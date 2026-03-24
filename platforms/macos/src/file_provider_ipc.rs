use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use anyhow::{Context, Result};
use cloudreve_platforms_api::{
    FetchDataRequest, FetchDataWriter, FileProviderItemState, MountedDriveCallback, MountSession,
    PlaceholderEntry,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};

use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};

const MAX_FETCH_DATA_BYTES: u64 = 8 * 1024 * 1024; // 8MB

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum IpcRequest {
    #[serde(rename = "get_item_state")]
    GetItemState { mount_id: String, path: String },

    #[serde(rename = "fetch_placeholders")]
    FetchPlaceholders { mount_id: String, path: String },

    #[serde(rename = "fetch_data")]
    FetchData {
        mount_id: String,
        path: String,
        range_start: u64,
        range_end: u64,
    },
}

#[derive(Debug, Serialize)]
struct IpcError {
    error: String,
}

#[derive(Debug, Serialize)]
struct GetItemStateResponse {
    category: String,
}

#[derive(Debug, Serialize)]
struct PlaceholderEntryIpc {
    relative_path: String,
    is_directory: bool,
    size: u64,
    created_unix: i64,
    modified_unix: i64,
    blob_base64: String,
    mark_in_sync: bool,
    overwrite: bool,
}

#[derive(Debug, Serialize)]
struct FetchPlaceholdersResponse {
    placeholders: Vec<PlaceholderEntryIpc>,
}

#[derive(Debug, Serialize)]
struct FetchDataResponse {
    data_base64: String,
}

#[derive(Debug)]
struct InMemoryFetchDataWriter {
    buf: Mutex<Vec<u8>>,
    base_offset: u64,
}

impl InMemoryFetchDataWriter {
    fn new(total_bytes: u64, base_offset: u64) -> Result<Self> {
        let total_bytes_usize: usize =
            total_bytes.try_into().context("fetch range too large for buffer allocation")?;
        Ok(Self {
            buf: Mutex::new(vec![0u8; total_bytes_usize]),
            base_offset,
        })
    }

    fn into_bytes(self) -> Vec<u8> {
        self.buf.into_inner().unwrap_or_default()
    }
}

impl FetchDataWriter for InMemoryFetchDataWriter {
    fn write_at(&self, data: &[u8], offset: u64) -> Result<()> {
        let start = offset
            .checked_sub(self.base_offset)
            .context("offset underflow in fetch writer")? as usize;
        let end = start + data.len();

        let mut guard = self.buf.lock().expect("fetch writer mutex poisoned");
        if end > guard.len() {
            anyhow::bail!("fetch writer overflow: {} > {}", end, guard.len());
        }
        guard[start..end].copy_from_slice(data);
        Ok(())
    }

    fn report_progress(&self, _total_bytes: u64, _transferred_bytes: u64) -> Result<()> {
        Ok(())
    }
}

struct ServerState {
    socket_path: PathBuf,
    handlers: Mutex<HashMap<String, Arc<dyn MountedDriveCallback>>>,
    started: Mutex<bool>,
}

static SERVER_STATE: OnceLock<Arc<ServerState>> = OnceLock::new();

fn socket_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME env is required")?;
    Ok(PathBuf::from(home)
        .join(".cloudreve")
        .join("macos-file-provider")
        .join("xpc.sock"))
}

fn ensure_server_started() -> Result<Arc<ServerState>> {
    if let Some(state) = SERVER_STATE.get() {
        return Ok(state.clone());
    }

    let socket_path = socket_path()?;
    let state = Arc::new(ServerState {
        socket_path,
        handlers: Mutex::new(HashMap::new()),
        started: Mutex::new(false),
    });

    SERVER_STATE
        .set(state.clone())
        .map_err(|_| anyhow::anyhow!("IPC server state already initialized"))?;

    // Start accept loop once; Subsequent calls only register handlers.
    {
        let mut started = state.started.lock().expect("mutex poisoned");
        if *started {
            return Ok(state.clone());
        }
        *started = true;
    }

    // Spawn accept loop.
    let state_clone = state.clone();
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            if let Err(e) = accept_loop(state_clone).await {
                tracing::error!(
                    target: "platforms::macos::fpe-ipc",
                    error = ?e,
                    "IPC server failed"
                );
            }
        });
    } else {
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for IPC server");
            rt.block_on(async move {
                if let Err(e) = accept_loop(state_clone).await {
                    tracing::error!(
                        target: "platforms::macos::fpe-ipc",
                        error = ?e,
                        "IPC server failed"
                    );
                }
            });
        });
    }

    Ok(state)
}

async fn accept_loop(state: Arc<ServerState>) -> Result<()> {
    // Clean stale socket file if any.
    if state.socket_path.exists() {
        let _ = std::fs::remove_file(&state.socket_path);
    }
    if let Some(parent) = state.socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(&state.socket_path)
        .with_context(|| format!("failed to bind {}", state.socket_path.display()))?;
    tracing::info!(
        target: "platforms::macos::fpe-ipc",
        socket = %state.socket_path.display(),
        "IPC server listening"
    );

    loop {
        let (mut stream, _addr) = listener.accept().await?;

        // Process connection in background to keep accept loop responsive.
        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(&mut stream, state_clone).await {
                tracing::warn!(target: "platforms::macos::fpe-ipc", error = ?e, "IPC connection failed");
            }
        });
    }
}

async fn handle_connection(stream: &mut tokio::net::UnixStream, state: Arc<ServerState>) -> Result<()> {
    // Expect a single newline-delimited JSON request.
    let mut buf = vec![];
    let mut tmp = [0u8; 1024];
    loop {
        let n = stream.read(&mut tmp).await.context("failed to read IPC request bytes")?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.last() == Some(&b'\n') {
            break;
        }
    }

    if buf.is_empty() {
        return Ok(());
    }

    let req: IpcRequest = serde_json::from_slice(&buf).context("failed to parse IPC JSON")?;
    let mount_id = match &req {
        IpcRequest::GetItemState { mount_id, .. } => mount_id.clone(),
        IpcRequest::FetchPlaceholders { mount_id, .. } => mount_id.clone(),
        IpcRequest::FetchData { mount_id, .. } => mount_id.clone(),
    };

    let handler = {
        let guard = state.handlers.lock().expect("mutex poisoned");
        guard.get(&mount_id).cloned()
    }
    .context("unknown mount_id in IPC request")?;

    match req {
        IpcRequest::GetItemState { path, .. } => {
            let state = handler.get_item_state(PathBuf::from(path))?;
            let category = match state {
                FileProviderItemState::CloudOnly => "CloudOnly",
                FileProviderItemState::Syncing => "Syncing",
                FileProviderItemState::Synced => "Synced",
                FileProviderItemState::Error => "Error",
            }
            .to_string();
            let resp = GetItemStateResponse { category };
            send_json(stream, &resp).await?;
        }
        IpcRequest::FetchPlaceholders { path, .. } => {
            let entries = handler.fetch_placeholders(PathBuf::from(path))?;
            let placeholders = entries
                .into_iter()
                .map(placeholder_entry_to_ipc)
                .collect::<Vec<_>>();
            let resp = FetchPlaceholdersResponse { placeholders };
            send_json(stream, &resp).await?;
        }
        IpcRequest::FetchData {
            path,
            range_start,
            range_end,
            ..
        } => {
            if range_end <= range_start {
                anyhow::bail!("invalid fetch range: end <= start");
            }
            let total_bytes = range_end - range_start;
            if total_bytes > MAX_FETCH_DATA_BYTES {
                anyhow::bail!("fetch range too large: {} bytes", total_bytes);
            }

            let writer = Arc::new(InMemoryFetchDataWriter::new(total_bytes, range_start)?);
            handler.fetch_data(FetchDataRequest {
                path: PathBuf::from(path),
                range: range_start..range_end,
                writer: writer.clone(),
            })?;

            let bytes = writer
                .buf
                .lock()
                .expect("fetch writer mutex poisoned")
                .clone();
            let resp = FetchDataResponse {
                data_base64: general_purpose::STANDARD.encode(bytes),
            };
            send_json(stream, &resp).await?;
        }
    }

    Ok(())
}

fn placeholder_entry_to_ipc(entry: PlaceholderEntry) -> PlaceholderEntryIpc {
    PlaceholderEntryIpc {
        relative_path: entry.relative_path.to_string_lossy().to_string(),
        is_directory: entry.is_directory,
        size: entry.size,
        created_unix: entry.created_unix,
        modified_unix: entry.modified_unix,
        blob_base64: general_purpose::STANDARD.encode(entry.blob),
        mark_in_sync: entry.mark_in_sync,
        overwrite: entry.overwrite,
    }
}

async fn send_json<T: Serialize>(stream: &mut tokio::net::UnixStream, value: &T) -> Result<()> {
    let json = serde_json::to_vec(value)?;
    stream.write_all(&json).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;
    Ok(())
}

/// Register one drive handler for a mount_id; the returned session only controls handler lifetime.
pub fn register_handler_for_mount(
    mount_id: String,
    handler: Arc<dyn MountedDriveCallback>,
) -> Result<Box<dyn MountSession>> {
    let state = ensure_server_started()?;
    {
        let mut guard = state.handlers.lock().expect("mutex poisoned");
        guard.insert(mount_id.clone(), handler);
    }

    Ok(Box::new(FileProviderMountSession { mount_id }))
}

struct FileProviderMountSession {
    mount_id: String,
}

impl MountSession for FileProviderMountSession {
    fn disconnect(&self) -> Result<()> {
        if let Some(state) = SERVER_STATE.get() {
            let mut guard = state.handlers.lock().expect("mutex poisoned");
            guard.remove(&self.mount_id);
        }
        Ok(())
    }
}


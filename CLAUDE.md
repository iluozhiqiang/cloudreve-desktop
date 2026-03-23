# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Cloudreve Desktop is a Tauri-based desktop application that synchronizes files with a Cloudreve cloud drive server.

Current state: the shipped product is Windows-first and uses the Windows Cloud Files API (CFAPI) plus Windows shell integration.

Direction: the workspace is being split into a platform-agnostic sync core plus platform-specific crates, so future platform work stays outside `cloudreve-sync`.

It provides:
- Real-time bidirectional file synchronization
- On-demand file hydration (files appear locally but are downloaded only when accessed)
- Windows shell integration (context menus, thumbnails, custom states)
- Multiple storage provider support for uploads (S3, OneDrive, Qiniu, Upyun, local)
- System tray application with React-based UI

## Build Commands

```bash
# Cross-platform core checks - run from project root
cargo check -p cloudreve-platforms-api -p cloudreve-platforms-macos -p cloudreve-sync -p cloudreve-desktop
cargo test --package cloudreve-sync --lib inventory::db::tests
cargo test --package cloudreve-api

# Full workspace / platform-heavy checks
cargo build                    # Build all workspace crates
cargo build --release          # Release build
cargo check                    # Check for compilation errors
cargo test                     # Run all tests

# Frontend (React/TypeScript) - run from ui/ directory
cd ui
yarn install                   # Install dependencies
yarn dev                       # Start Vite dev server (localhost:5173)
yarn build                     # Build for production
yarn lint                      # Run ESLint

# Full Tauri Application - run from project root
cargo tauri dev                # Development mode with hot reload
cargo tauri build              # Production build
```

Use the cross-platform core checks when working on `cloudreve-sync`, `platforms/api`, `platforms/macos`, `src-tauri`, or the frontend from a non-Windows machine. Use the full workspace / Windows packaging flows when validating CFAPI or shell integration behavior.

## Architecture

### Workspace Structure

```
├── src-tauri/           # Desktop assembly layer
├── crates/
│   ├── app-config/               # Shared config loading/persistence
│   ├── cloudreve-sync/           # Platform-agnostic sync core
│   ├── cloudreve-api/            # Async REST client for Cloudreve server
│   └── platforms/
│       ├── api/                  # Minimal platform interfaces consumed by sync core
│       ├── macos/                # macOS stub / future integration entry
│       └── windows/              # Windows implementations (cfapi, shell, notif)
└── ui/                  # React frontend (Vite + MUI)
```

### Tauri Layer (`src-tauri/`)

- `lib.rs`: Application entry, initializes sync service, selects platform provider, sets up system tray, spawns event bridge
- `commands.rs`: Tauri IPC commands exposed to frontend (`list_drives`, `add_drive`, `remove_drive`, etc.)
- `event_handler.rs`: Bridges `EventBroadcaster` events to Tauri frontend events

**Role**: `src-tauri` is the desktop assembly boundary. It wires `cloudreve-sync` to the selected platform crate and owns platform-specific startup glue that should not leak into the sync core.

**Initialization Flow**: App starts → system tray setup → async `init_sync_service()` spawned → platform provider selected → DriveManager created → Windows shell services initialized when applicable → event bridge connects EventBroadcaster to Tauri

### Core Sync Module (`crates/cloudreve-sync/`)

**Drive Management:**
- `drive/manager.rs`: Central `DriveManager` coordinating all mounted drives via command channel
- `drive/mounts.rs`: Individual mount point (`Mount`) handling
- `drive/callback.rs`: Platform mount callback adapter consumed through abstract interfaces
- `drive/sync.rs`: Sync logic and placeholder/metadata conversion
- `drive/remote_events.rs`: SSE handling for server-pushed changes

`cloudreve-sync` should remain free of direct platform implementation code. It depends on `cloudreve-platforms-api` traits and delegates all concrete shell / virtual file / mount behavior to the selected platform crate.

**Platform Crates:**
- `crates/platforms/api/`: Minimal shared traits and neutral data structures
- `crates/platforms/windows/`: Windows platform provider, shell integration, notifications
- `crates/platforms/windows/cfapi/`: Windows CFAPI wrapper and sync root primitives
- `crates/platforms/macos/`: macOS stub provider for compile-time separation and future expansion

**Persistence (`inventory/`):**
- SQLite via Diesel ORM at `~/.cloudreve/meta.db`
- Stores file metadata, task queue, upload sessions, drive properties
- Migrations in `migrations/inventory/`

**Uploads (`uploader/`):**
- Chunked upload with provider backends: S3, OneDrive, Qiniu, Upyun, local
- Encryption and resumable upload support

### Frontend (`ui/`)

React 19 + TypeScript + MUI + Vite application:
- `src/pages/popup/`: Main tray popup (drive list, task progress)
- `src/pages/AddDrive.tsx`: Add drive wizard
- `src/pages/settings/`: Settings pages
- i18n via react-i18next, translations in `ui/public/locales/`

### Key Patterns

**Command Channels**: `DriveManager` and `Mount` use `mpsc::UnboundedSender` for async command dispatch from platform glue and Tauri commands.

**Callback Threading**: Platform mount callbacks may run on OS threads, using `blocking_recv()` on oneshot channels to await async operations.

**Event Broadcasting**: `EventBroadcaster` (tokio broadcast channel) pushes events to both the Tauri frontend (via event bridge) and any SSE subscribers.

**Global State**: `APP_STATE` (tokio `OnceCell`) holds initialized `DriveManager`, `EventBroadcaster`, and service handles for the application lifetime.

## Windows-Specific Notes

- Requires Windows Cloud Files API (`windows` crate with extensive feature flags in Cargo.toml)
- COM shell services for Explorer integration live under `crates/platforms/windows/`
- Deep link protocol: `cloudreve://`
- Single instance enforcement via `tauri-plugin-single-instance`

## Database Migrations

Migrations are embedded and run automatically on startup. To add a new migration:
1. Create folder in `migrations/inventory/` (e.g., `0005_new_table/`)
2. Add `up.sql` and `down.sql` files
3. Use idempotent SQL (`IF NOT EXISTS`) for clean upgrades

## Localization

- Backend: `rust-i18n` macro with translations in `locales/`
- Frontend: `react-i18next` with translations in `ui/public/locales/{locale}/common.json`

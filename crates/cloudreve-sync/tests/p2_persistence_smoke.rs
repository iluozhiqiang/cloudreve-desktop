//! P2：不依赖网络与 DriveManager 的持久化边界冒烟（多 drive 配置、库存 DB 初始化、
//! 与真实 `drives.json` 读写路径一致的 `DriveState::read_from_path` / `write_to_path`）。

use cloudreve_sync::drive::manager::DriveState;
use cloudreve_sync::inventory::InventoryDb;
use cloudreve_sync::{Credentials, DriveConfig};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn sample_drive(id: &str, sync_path: PathBuf) -> DriveConfig {
    DriveConfig {
        id: id.into(),
        name: format!("Drive {id}"),
        instance_url: "https://example.com".into(),
        remote_path: "/".into(),
        credentials: Credentials {
            access_token: None,
            refresh_token: "r".into(),
            refresh_expires: "2099-01-01T00:00:00Z".into(),
            access_expires: None,
        },
        sync_path,
        icon_path: None,
        raw_icon_path: None,
        enabled: true,
        user_id: "u".into(),
        mount_id: Some(format!("mount-{id}")),
        ignore_patterns: vec![],
        extra: HashMap::new(),
    }
}

#[test]
fn drive_state_multi_drive_json_roundtrip() {
    let base = if cfg!(windows) {
        PathBuf::from(r"C:\tmp\p2-smoke")
    } else {
        PathBuf::from("/tmp/p2-smoke")
    };
    let state = DriveState {
        drives: vec![
            sample_drive("a", base.join("a")),
            sample_drive("b", base.join("b")),
        ],
    };

    let json = serde_json::to_string_pretty(&state).expect("serialize DriveState");
    let back: DriveState = serde_json::from_str(&json).expect("deserialize DriveState");
    assert_eq!(back.drives.len(), 2);
    assert_eq!(back.drives[0].id, "a");
    assert_eq!(back.drives[1].id, "b");
    assert_eq!(back.drives[0].sync_path, state.drives[0].sync_path);
    assert_eq!(
        back.drives[0].mount_id.as_deref(),
        state.drives[0].mount_id.as_deref()
    );
}

#[test]
fn drive_state_preserves_extra_on_drive_config() {
    let sync = if cfg!(windows) {
        PathBuf::from(r"C:\tmp\p2-smoke-x")
    } else {
        PathBuf::from("/tmp/p2-smoke-x")
    };
    let mut d = sample_drive("x", sync);
    d.extra
        .insert("custom".into(), json!({"nested": [1, 2]}));
    let state = DriveState { drives: vec![d] };
    let json = serde_json::to_string(&state).unwrap();
    let back: DriveState = serde_json::from_str(&json).unwrap();
    assert_eq!(
        back.drives[0].extra.get("custom"),
        Some(&json!({"nested": [1, 2]}))
    );
}

#[test]
fn inventory_db_opens_and_migrates_in_temp_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("meta.db");
    {
        let _db = InventoryDb::with_path(db_path.clone()).expect("open inventory");
    }
    // 再次打开应成功（迁移已应用）
    let _db2 = InventoryDb::with_path(db_path).expect("reopen inventory");
}

#[test]
fn drive_state_roundtrip_matches_manager_on_disk_format() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("drives.json");
    let base = dir.path().join("sync");
    let state = DriveState {
        drives: vec![
            sample_drive("a", base.join("a")),
            sample_drive("b", base.join("b")),
        ],
    };
    state.write_to_path(&path).expect("write drives.json");
    let loaded = DriveState::read_from_path(&path).expect("read drives.json");
    assert_eq!(loaded.drives.len(), state.drives.len());
    assert_eq!(loaded.drives[0].id, "a");
    assert_eq!(loaded.drives[1].id, "b");
}

#[test]
fn drives_json_on_disk_accepts_legacy_sync_root_id_alias() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path: PathBuf = dir.path().join("drives.json");
    let sync_path = if cfg!(windows) {
        r"C:\legacy\sync"
    } else {
        "/legacy/sync"
    };
    let doc = json!({
        "drives": [{
            "id": "legacy-1",
            "name": "L",
            "instance_url": "https://example.com",
            "remote_path": "/",
            "credentials": { "refresh_token": "r", "refresh_expires": "" },
            "sync_path": sync_path,
            "enabled": true,
            "user_id": "u",
            "sync_root_id": "from-old-config"
        }]
    });
    fs::write(
        &path,
        serde_json::to_string_pretty(&doc).expect("serialize fixture"),
    )
    .expect("write raw drives.json");
    let state = DriveState::read_from_path(&path).expect("read drives.json");
    assert_eq!(state.drives.len(), 1);
    assert_eq!(state.drives[0].mount_id.as_deref(), Some("from-old-config"));
}

#[test]
fn drive_state_empty_drives_roundtrip_on_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("drives.json");
    let state = DriveState::default();
    state.write_to_path(&path).expect("write empty drives.json");
    let loaded = DriveState::read_from_path(&path).expect("read empty drives.json");
    assert!(loaded.drives.is_empty());
}

#[test]
fn read_from_path_errors_when_file_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("no-drives.json");
    let err = DriveState::read_from_path(&path).unwrap_err();
    assert!(
        err.to_string().contains("Failed to read drive config file")
            || err.to_string().contains("no-drives.json")
            || err.chain().any(|e| e.to_string().contains("No such file")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn read_from_path_errors_on_invalid_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("drives.json");
    fs::write(&path, "not json {{{").expect("write bad json");
    let err = DriveState::read_from_path(&path).unwrap_err();
    assert!(
        err.to_string().contains("Failed to parse drive config")
            || err.chain().any(|e| e.to_string().contains("expected")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn read_minimal_valid_empty_drives_fixture() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("drives.json");
    fs::write(&path, r#"{"drives":[]}"#).expect("write minimal");
    let state = DriveState::read_from_path(&path).expect("read");
    assert!(state.drives.is_empty());
}

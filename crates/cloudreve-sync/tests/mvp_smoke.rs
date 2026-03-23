//! MVP 级冒烟：不拉起网络与平台 provider，仅校验核心类型与序列化边界，
//! 供 CI / `scripts/smoke_mvp.sh` 做快速回归。

use cloudreve_sync::{Credentials, DriveConfig};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn drive_config_json_roundtrip() {
    let cfg = DriveConfig {
        id: "drive-1".into(),
        name: "Test".into(),
        instance_url: "https://example.com".into(),
        remote_path: "/my".into(),
        credentials: Credentials {
            access_token: Some("a".into()),
            refresh_token: "r".into(),
            refresh_expires: "2099-01-01T00:00:00Z".into(),
            access_expires: Some("2099-01-01T00:00:00Z".into()),
        },
        sync_path: PathBuf::from("/tmp/cloudreve-sync-test"),
        icon_path: None,
        raw_icon_path: None,
        enabled: true,
        user_id: "user-1".into(),
        mount_id: Some("mount-xyz".into()),
        ignore_patterns: vec![".DS_Store".into()],
        extra: HashMap::from([("k".into(), json!(1))]),
    };

    let json = serde_json::to_string(&cfg).expect("serialize");
    let back: DriveConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.id, cfg.id);
    assert_eq!(back.sync_path, cfg.sync_path);
    assert_eq!(back.mount_id, cfg.mount_id);
    assert_eq!(back.ignore_patterns, cfg.ignore_patterns);
}

#[test]
fn drive_config_deserialize_legacy_sync_root_id_alias() {
    let j = r#"{
        "id": "1",
        "name": "n",
        "instance_url": "https://x.com",
        "remote_path": "/",
        "credentials": { "refresh_token": "r", "refresh_expires": "" },
        "sync_path": "/a",
        "enabled": true,
        "user_id": "u",
        "sync_root_id": "legacy-id"
    }"#;
    let cfg: DriveConfig = serde_json::from_str(j).expect("deserialize alias");
    assert_eq!(cfg.mount_id.as_deref(), Some("legacy-id"));
}

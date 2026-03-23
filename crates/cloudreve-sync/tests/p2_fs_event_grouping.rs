//! P2：`group_fs_events` 对 watcher 事件按归一化 `EventKind` 分桶（与 `Mount::process_fs_events` 分支一致）。

use cloudreve_sync::drive::sync::group_fs_events;
use notify_debouncer_full::notify::event::{
    CreateKind, DataChange, EventKind, ModifyKind, RemoveKind, RenameMode,
};
use notify_debouncer_full::notify::Event;
use notify_debouncer_full::DebouncedEvent;
use std::path::PathBuf;
use std::time::Instant;

fn debounced(kind: EventKind, paths: Vec<PathBuf>) -> DebouncedEvent {
    DebouncedEvent::new(
        Event {
            kind,
            paths,
            attrs: Default::default(),
        },
        Instant::now(),
    )
}

#[test]
fn normalize_remove_variants_into_one_bucket() {
    let grouped = group_fs_events(vec![
        debounced(
            EventKind::Remove(RemoveKind::File),
            vec![PathBuf::from("/sync/a.txt")],
        ),
        debounced(
            EventKind::Remove(RemoveKind::Folder),
            vec![PathBuf::from("/sync/b")],
        ),
    ]);
    let key = EventKind::Remove(RemoveKind::Any);
    assert_eq!(grouped.len(), 1);
    assert_eq!(grouped[&key].len(), 2);
}

#[test]
fn rename_both_keeps_distinct_modify_name_kind() {
    let grouped = group_fs_events(vec![debounced(
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
        vec![PathBuf::from("/sync/old"), PathBuf::from("/sync/new")],
    )]);
    let key = EventKind::Modify(ModifyKind::Name(RenameMode::Both));
    assert_eq!(grouped.len(), 1);
    assert_eq!(grouped[&key].len(), 1);
}

#[test]
fn modify_non_rename_collapses_to_modify_any() {
    let grouped = group_fs_events(vec![debounced(
        EventKind::Modify(ModifyKind::Data(DataChange::Any)),
        vec![PathBuf::from("/sync/f.bin")],
    )]);
    let key = EventKind::Modify(ModifyKind::Any);
    assert_eq!(grouped.len(), 1);
    assert_eq!(grouped[&key].len(), 1);
}

#[test]
fn create_events_share_create_any_bucket() {
    let grouped = group_fs_events(vec![
        debounced(
            EventKind::Create(CreateKind::File),
            vec![PathBuf::from("/sync/n.txt")],
        ),
        debounced(
            EventKind::Create(CreateKind::Folder),
            vec![PathBuf::from("/sync/sub")],
        ),
    ]);
    let key = EventKind::Create(CreateKind::Any);
    assert_eq!(grouped[&key].len(), 2);
}

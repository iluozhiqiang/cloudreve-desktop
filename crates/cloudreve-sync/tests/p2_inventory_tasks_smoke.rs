//! P2：库存库任务队列在无网络环境下的行为回归（去重插入、更新、按路径取消、近期任务查询）。

use cloudreve_sync::inventory::{
    InventoryDb, NewTaskRecord, TaskStatus, TaskUpdate,
};

fn open_db(dir: &tempfile::TempDir) -> InventoryDb {
    InventoryDb::with_path(dir.path().join("meta.db")).expect("open inventory db")
}

#[test]
fn insert_task_dedupes_active_same_drive_type_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = open_db(&dir);
    let path = dir.path().join("sync").join("a.txt");
    let path_str = path.to_string_lossy().to_string();

    let t1 = NewTaskRecord::new("task-1", "drive-1", "upload", &path_str);
    assert!(db.insert_task_if_not_exist(&t1).expect("first insert"));
    let t2 = NewTaskRecord::new("task-2", "drive-1", "upload", &path_str);
    assert!(
        !db.insert_task_if_not_exist(&t2).expect("dedupe"),
        "second pending upload for same path should be skipped"
    );

    db.update_task(
        "task-1",
        TaskUpdate {
            status: Some(TaskStatus::Completed),
            ..Default::default()
        },
    )
    .expect("complete task");

    assert!(
        db.insert_task_if_not_exist(&t2).expect("after complete"),
        "after previous task finished, same path may enqueue again"
    );
}

#[test]
fn cancel_tasks_by_path_cancels_exact_and_descendants() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = open_db(&dir);
    let root = dir.path().join("mount");
    let sub = root.join("deep");
    std::fs::create_dir_all(&sub).expect("mkdir");

    let root_s = root.to_string_lossy().to_string();
    let child = sub.join("f.txt").to_string_lossy().to_string();

    let at_root = NewTaskRecord::new("r1", "d-x", "sync", &root_s);
    let at_child = NewTaskRecord::new("r2", "d-x", "sync", &child);
    assert!(db.insert_task_if_not_exist(&at_root).unwrap());
    assert!(db.insert_task_if_not_exist(&at_child).unwrap());

    let cancelled = db
        .cancel_tasks_by_path("d-x", &root_s)
        .expect("cancel by root path");
    assert_eq!(cancelled.len(), 2);

    assert_eq!(
        db.get_task_status("r1").expect("status").unwrap(),
        TaskStatus::Cancelled
    );
    assert_eq!(
        db.get_task_status("r2").expect("status").unwrap(),
        TaskStatus::Cancelled
    );
}

#[test]
fn query_recent_tasks_splits_active_and_finished() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = open_db(&dir);
    let p = dir.path().join("x.bin").to_string_lossy().to_string();

    let pending = NewTaskRecord::new("p1", "drv", "download", &p);
    assert!(db.insert_task_if_not_exist(&pending).unwrap());

    let done = NewTaskRecord::new("p2", "drv", "download", &(p + ".done"))
        .with_status(TaskStatus::Completed);
    assert!(db.insert_task_if_not_exist(&done).unwrap());

    let recent = db.query_recent_tasks(Some("drv")).expect("recent");
    assert_eq!(recent.active.len(), 1);
    assert_eq!(recent.active[0].id, "p1");
    assert!(recent
        .finished
        .iter()
        .any(|t| t.id == "p2" && t.status == TaskStatus::Completed));
}

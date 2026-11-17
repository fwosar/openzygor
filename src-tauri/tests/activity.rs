use openzygor::activity::{self, HISTORY_LIMIT};
use std::{collections::HashSet, fs, sync::Arc, thread};

#[test]
fn entries_and_clear_survive_reopening_the_store() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config/activity.json");
    assert!(activity::load(&path).unwrap().is_empty());
    assert!(!path.exists());
    let first = activity::append(&path, "Updated guides in /games/Édition", false).unwrap();
    let second = activity::append(&path, "Download cancelled", true).unwrap();
    assert_eq!(second[1], first[0]);
    assert!(second[0].failed);
    assert!(second[0].timestamp_ms >= first[0].timestamp_ms);
    assert_eq!(activity::load(&path).unwrap(), second);
    assert!(activity::clear(&path).unwrap().is_empty());
    assert!(activity::load(&path).unwrap().is_empty());
    activity::append(&path, "New entry", false).unwrap();
    assert_eq!(activity::load(&path).unwrap().len(), 1);
}

#[test]
fn history_and_individual_messages_are_bounded() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("activity.json");
    for n in 0..HISTORY_LIMIT + 3 {
        activity::append(&path, &n.to_string(), false).unwrap();
    }
    let entries = activity::load(&path).unwrap();
    assert_eq!(entries.len(), HISTORY_LIMIT);
    assert_eq!(
        entries.first().unwrap().message,
        (HISTORY_LIMIT + 2).to_string()
    );
    assert_eq!(entries.last().unwrap().message, "3");
    activity::append(&path, &"é".repeat(5000), false).unwrap();
    let entries = activity::load(&path).unwrap();
    assert_eq!(entries[0].message.chars().count(), 4096);
    assert!(entries[0].message.chars().all(|c| c == 'é'));
}

#[test]
fn concurrent_writers_do_not_overwrite_each_others_entries() {
    let root = tempfile::tempdir().unwrap();
    let path = Arc::new(root.path().join("activity.json"));
    let threads: Vec<_> = (0..8)
        .map(|writer| {
            let path = path.clone();
            thread::spawn(move || {
                for item in 0..10 {
                    activity::append(&path, &format!("{writer}/{item}"), false).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let entries = activity::load(&path).unwrap();
    assert_eq!(entries.len(), 80);
    assert_eq!(
        entries
            .iter()
            .map(|e| &e.message)
            .collect::<HashSet<_>>()
            .len(),
        80
    );
}

#[test]
fn damaged_history_is_preserved_until_explicitly_cleared() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("activity.json");
    for contents in [
        "broken json",
        r#"[{"message":"bad date","failed":false,"timestamp_ms":18446744073709551615}]"#,
    ] {
        fs::write(&path, contents).unwrap();
        assert!(activity::load(&path).is_err());
        assert!(activity::append(&path, "New entry", false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        activity::clear(&path).unwrap();
        assert!(activity::load(&path).unwrap().is_empty());
    }
}

#[cfg(unix)]
#[test]
fn saved_history_and_lock_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("activity.json");
    activity::append(&path, "Entry", false).unwrap();
    for file in [&path, &path.with_extension("lock")] {
        assert_eq!(
            fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

use std::path::Path;

use elysiandb::durable::{self, Fs, KvRecord, filepath};

use super::memfs_test::new_mem_fs;

#[test]
fn test_kv_file_round_trip_and_corruption() {
    let fs = new_mem_fs();
    let _ = fs.mkdir_all(Path::new("/d"), 0o755);
    let path = filepath::join(Path::new("/d"), "kv-1.ckpt");
    let records = vec![
        KvRecord {
            key: b"a".to_vec(),
            lsn: 1,
            expires_at: 0,
            segment: 1,
            offset: 16,
            length: 5,
        },
        KvRecord {
            key: "é:long".as_bytes().to_vec(),
            lsn: 1 << 40,
            expires_at: -3,
            segment: 9,
            offset: 1 << 20,
            length: 0,
        },
    ];

    if let Err(e) = durable::write_kv_file(&fs, &path, &records) {
        panic!("{e}");
    }

    match durable::read_kv_file(&fs, &path) {
        Ok(got) if got == records => {}
        other => panic!("round trip: {other:?}"),
    }

    fs.mutate(&path, |n| n.data[12] ^= 0xff);
    if durable::read_kv_file(&fs, &path).is_ok() {
        panic!("a corrupt file must be detected");
    }
}

#[test]
fn test_atomic_writes_leave_the_old_file_until_they_are_complete() {
    let fs = new_mem_fs();
    let _ = fs.mkdir_all(Path::new("/d"), 0o755);
    let path = filepath::join(Path::new("/d"), "file");

    let write = |s: &str| durable::write_file_atomic(&fs, &path, |w| w.write(s.as_bytes()));

    if let Err(e) = write("first") {
        panic!("{e}");
    }

    let base = fs.ops();
    for at in 1..6 {
        let crashed = fs.after_process_crash();
        crashed.set_ops(0);
        crashed.set_crash_at(at);
        let _ = durable::write_file_atomic(&crashed, &path, |w| w.write(b"second"));

        let image = crashed.after_power_loss();
        let res = durable::read_file(&image, &path);
        let whole = matches!(&res, Ok(data) if data == b"first" || data == b"second");
        if !whole {
            panic!(
                "crash at operation {at} (after {base}): the file must be whole, got {:?}",
                res.as_ref()
                    .map(|d| String::from_utf8_lossy(d).into_owned())
            );
        }
    }
}

#[test]
fn test_manifest_generations() {
    let fs = new_mem_fs();
    let _ = fs.mkdir_all(Path::new("/d"), 0o755);
    let shared: std::sync::Arc<dyn Fs> = std::sync::Arc::new(fs.clone());
    let mut m = durable::open_manifests(Some(shared.clone()), Path::new("/d"));

    match m.stored() {
        Ok(stored) if stored.is_empty() => {}
        other => panic!("a new folder has no manifest: {other:?}"),
    }

    for content in ["one", "two", "three"] {
        if let Err(e) = m.commit(content.as_bytes()) {
            panic!("{e}");
        }
    }

    match m.stored() {
        Ok(stored)
            if stored.len() == 2 && stored[0].data == b"three" && stored[1].data == b"two" => {}
        other => panic!("the last two generations are kept, newest first: {other:?}"),
    }

    let _ = fs.remove(&filepath::join(Path::new("/d"), durable::MANIFEST_FILE));
    match durable::open_manifests(Some(shared), Path::new("/d")).stored() {
        Ok(stored) if stored.len() == 1 && stored[0].data == b"two" => {}
        other => panic!("the previous generation stays readable alone: {other:?}"),
    }
}

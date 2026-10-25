//! Synthetic-only MP-8L2 diagnostic: cargo run --release -p mind-place-archive --example mp8l2_archive_profile -- <synthetic archive-v1 directory>
use mind_place_archive::ArchiveStore;
use std::{env, path::PathBuf, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(env::args().nth(1).ok_or("missing isolated fixture path")?);
    let lower = path.to_string_lossy().to_ascii_lowercase();
    if !lower.starts_with("d:\\projects\\mp8l2-native-runs\\")
        || path.file_name().is_none_or(|name| name != "archive-v1")
    {
        return Err("refusing to read non-MP8L2 synthetic data".into());
    }
    let store = ArchiveStore::open(path)?;
    for i in 0..3 {
        let started = Instant::now();
        let records = store.list("")?;
        println!(
            "pass={} sources={} elapsed_ms={}",
            i + 1,
            records.len(),
            started.elapsed().as_millis()
        );
    }
    Ok(())
}

use crate::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    thread,
    time::{Duration, Instant},
};

pub const MAX_CAPTURE_BYTES: usize = 32 * 1024 * 1024;
const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct StoredMeta {
    summary: ConversationSummary,
    latest_snapshot: String,
}

fn invalid(text: impl Into<String>) -> ArchiveError {
    ArchiveError::Invalid(text.into())
}
pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn hex_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

pub(crate) fn no_link(path: &Path) -> Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(invalid(
                    "Archive paths cannot be symbolic links or reparse points",
                ));
            }
        }
        if meta.file_type().is_symlink() {
            return Err(invalid("Archive paths cannot be symbolic links"));
        }
    }
    Ok(())
}

pub(crate) fn checked_dir(path: &Path) -> Result<()> {
    no_link(path)?;
    fs::create_dir_all(path)?;
    no_link(path)?;
    Ok(())
}

pub(crate) fn bounded_read(path: &Path, max: usize) -> Result<Vec<u8>> {
    no_link(path)?;
    let file = File::open(path)?;
    if file.metadata()?.len() > max as u64 {
        return Err(invalid("Archive file exceeds its size limit"));
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(invalid("Archive file exceeds its size limit"));
    }
    Ok(bytes)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8], immutable: bool) -> Result<()> {
    no_link(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Missing archive parent"))?;
    checked_dir(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    if immutable {
        file.persist_noclobber(path).map_err(|e| e.error)?;
    } else {
        file.persist(path).map_err(|e| e.error)?;
    }
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

pub fn conversation_id_from_url(source: &str) -> Result<String> {
    let url = url::Url::parse(source)
        .map_err(|_| invalid("Open a saved ChatGPT conversation before capturing"))?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("chatgpt.com" | "chat.openai.com"))
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid("Capture must belong to the official ChatGPT site"));
    }
    for (k, v) in url.query_pairs() {
        if k.to_ascii_lowercase().contains("temporary") && !matches!(v.as_ref(), "false" | "0") {
            return Err(invalid("Temporary Chat is excluded from capture"));
        }
    }
    let parts: Vec<_> = url
        .path()
        .trim_end_matches('/')
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    let id = match parts.as_slice() {
        ["c", id] => *id,
        ["g", project, "c", id]
            if project.len() <= 160
                && project
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') =>
        {
            *id
        }
        _ => {
            return Err(invalid(
                "Open a saved ChatGPT conversation before capturing",
            ))
        }
    };
    if !uuid(id) {
        return Err(invalid("Invalid ChatGPT conversation identity"));
    }
    Ok(id.to_ascii_lowercase())
}

fn gemini_conversation_id_from_url(source: &str) -> Result<String> {
    let url = url::Url::parse(source)
        .map_err(|_| invalid("Open a saved Gemini conversation before capturing"))?;
    if url.scheme() != "https"
        || url.host_str() != Some("gemini.google.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid("Capture must belong to the official Gemini site"));
    }
    for (key, value) in url.query_pairs() {
        let key = key.to_ascii_lowercase();
        if ["temporary", "incognito", "private"]
            .iter()
            .any(|needle| key.contains(needle))
            && !matches!(value.to_ascii_lowercase().as_str(), "false" | "0" | "no")
        {
            return Err(invalid("Private Gemini conversations are excluded from capture"));
        }
    }
    let parts: Vec<_> = url.path().split('/').filter(|part| !part.is_empty()).collect();
    let id = match parts.as_slice() {
        ["app", id] => *id,
        ["u", account, "app", id]
            if !account.is_empty()
                && account.len() <= 3
                && account.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            *id
        }
        _ => return Err(invalid("Open a saved Gemini conversation before capturing")),
    };
    if !(8..=128).contains(&id.len())
        || !id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(invalid("Invalid Gemini conversation identity"));
    }
    Ok(id.to_owned())
}

fn validate(c: &mut Capture) -> Result<()> {
    if c.schema_version != 1 || !matches!(c.provider.as_str(), "chatgpt" | "gemini") {
        return Err(invalid("Unsupported archive schema or provider"));
    }
    if c.provider == "gemini" && c.capture_method != "manual-dom" {
        return Err(invalid("Gemini capture is explicit manual DOM only"));
    }
    if !matches!(
        c.capture_method.as_str(),
        "manual-dom" | "automatic-dom" | "legacy-import"
    ) {
        return Err(invalid("Unsupported capture method"));
    }
    if !matches!(c.completeness.as_str(), "partial" | "complete")
        || (matches!(c.capture_method.as_str(), "manual-dom" | "automatic-dom")
            && c.completeness != "partial")
    {
        return Err(invalid(
            "A DOM read cannot prove complete conversation history",
        ));
    }
    if c.source_url.len() > 32768 {
        return Err(invalid("Capture URL exceeds its size limit"));
    }
    let canonical_id = match c.provider.as_str() {
        "chatgpt" => conversation_id_from_url(&c.source_url)?,
        "gemini" => gemini_conversation_id_from_url(&c.source_url)?,
        _ => return Err(invalid("Unsupported archive provider")),
    };
    let supplied_id = if c.provider == "chatgpt" {
        c.conversation_id.to_ascii_lowercase()
    } else {
        c.conversation_id.clone()
    };
    if canonical_id != supplied_id {
        return Err(invalid("Capture URL and conversation identity do not match"));
    }
    c.conversation_id = canonical_id;
    if c.title.trim().is_empty() || c.title.len() > 1024 {
        return Err(invalid("Conversation title is missing or too long"));
    }
    chrono::DateTime::parse_from_rfc3339(&c.captured_at)
        .map_err(|_| invalid("Invalid capture timestamp"))?;
    for timestamp in [
        c.conversation_created_at.as_deref(),
        c.conversation_updated_at.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        chrono::DateTime::parse_from_rfc3339(timestamp)
            .map_err(|_| invalid("Invalid provider conversation timestamp"))?;
    }
    if c.messages.is_empty() || c.messages.len() > 10_000 {
        return Err(invalid(
            "Capture must contain between 1 and 10000 loaded messages",
        ));
    }
    if c.warnings.len() > 32 || c.warnings.iter().any(|w| w.len() > 2048) {
        return Err(invalid("Too many or oversized capture warnings"));
    }
    if c.completeness == "partial" && c.warnings.is_empty() {
        c.warnings
            .push("Only loaded messages were captured; unloaded history may be missing.".into());
    }
    let mut ids = HashSet::new();
    for m in &c.messages {
        if m.id.is_empty() || m.id.len() > 256 || !ids.insert(&m.id) {
            return Err(invalid("Missing or duplicate message identity"));
        }
        if !matches!(m.role.as_str(), "user" | "assistant" | "system" | "tool")
            || !matches!(m.format.as_str(), "plain" | "markdown")
        {
            return Err(invalid("Unsupported message role or text format"));
        }
        if m.text.len() > MAX_MESSAGE_BYTES
            || m.model.as_ref().is_some_and(|s| s.len() > 256)
            || m.attachments.len() > 100
        {
            return Err(invalid("Message exceeds its size limit"));
        }
        if let Some(time) = &m.authored_at {
            chrono::DateTime::parse_from_rfc3339(time)
                .map_err(|_| invalid("Invalid message timestamp"))?;
        }
        for a in &m.attachments {
            if a.id.is_empty()
                || a.id.len() > 256
                || a.name.len() > 1024
                || a.mime_type.len() > 128
                || a.source_url.as_ref().is_some_and(|s| s.len() > 32768)
                || a.unavailable_reason
                    .as_ref()
                    .is_some_and(|s| s.len() > 2048)
            {
                return Err(invalid("Attachment metadata exceeds its size limit"));
            }
            if a.data_url.is_some() && a.asset_path.is_some() {
                return Err(invalid(
                    "An attachment cannot supply both inline and archive paths",
                ));
            }
        }
    }
    if serde_json::to_vec(c)?.len() > MAX_CAPTURE_BYTES {
        return Err(invalid("Capture exceeds the 32 MiB limit"));
    }
    Ok(())
}

fn image_kind(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some(("image/jpeg", "jpg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("image/gif", "gif"))
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}

fn asset_name(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(hash, ext)| hex_id(hash) && matches!(ext, "png" | "jpg" | "gif" | "webp"))
}
fn observation_time(time: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(time)
        .map(|t| t.timestamp_millis())
        .unwrap_or(i64::MIN)
}

fn parsed_timestamp(time: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(time).ok()
}

fn original_conversation_date(capture: &Capture) -> (Option<String>, Option<String>) {
    if let Some(updated_at) = capture
        .conversation_updated_at
        .as_deref()
        .filter(|value| parsed_timestamp(value).is_some())
    {
        return (Some(updated_at.to_owned()), Some("provider-updated".into()));
    }

    if let Some((_, authored_at)) = capture
        .messages
        .iter()
        .filter_map(|message| {
            let authored_at = message.authored_at.as_deref()?;
            Some((parsed_timestamp(authored_at)?, authored_at))
        })
        .max_by(|(left, _), (right, _)| left.cmp(right))
    {
        return (
            Some(authored_at.to_owned()),
            Some("message-authored".into()),
        );
    }

    if let Some(created_at) = capture
        .conversation_created_at
        .as_deref()
        .filter(|value| parsed_timestamp(value).is_some())
    {
        return (Some(created_at.to_owned()), Some("provider-created".into()));
    }

    (None, None)
}

fn summary_date_order(a: &ConversationSummary, b: &ConversationSummary) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let a_date = a.conversation_date.as_deref().and_then(parsed_timestamp);
    let b_date = b.conversation_date.as_deref().and_then(parsed_timestamp);
    let date_order = match (a_date, b_date) {
        (Some(a_date), Some(b_date)) => b_date.cmp(&a_date),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    date_order
        .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        .then_with(|| a.title.cmp(&b.title))
        .then_with(|| a.id.cmp(&b.id))
}

impl ArchiveStore {
    pub(crate) fn initialize(&self) -> Result<()> {
        checked_dir(&self.root)?;
        checked_dir(&self.root.join("conversations"))
    }

    pub(crate) fn lock(&self) -> Result<File> {
        let path = self.root.join(".archive.lock");
        no_link(&path)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        let start = Instant::now();
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(file),
                Err(e) if e.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
                    if start.elapsed() > Duration::from_secs(15) {
                        return Err(invalid("Archive is busy; try saving again"));
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    pub(crate) fn conversation_dir(&self, id: &str) -> Result<PathBuf> {
        if !hex_id(id) {
            return Err(invalid("Invalid archive identity"));
        }
        let dir = self.root.join("conversations").join(id);
        no_link(&dir)?;
        Ok(dir)
    }

    pub fn save(&self, mut capture: Capture) -> Result<SaveReceipt> {
        validate(&mut capture)?;
        let _lock = self.lock()?;
        let id = digest(format!("{}:{}", capture.provider, capture.conversation_id).as_bytes());
        let dir = self.conversation_dir(&id)?;
        checked_dir(&dir)?;
        checked_dir(&dir.join("assets"))?;
        checked_dir(&dir.join("captures"))?;
        // Validate all supplied images before writing any; references are never fetched.
        let mut assets = Vec::new();
        for m in &mut capture.messages {
            for a in &mut m.attachments {
                if let Some(data) = a.data_url.take() {
                    if data.len() > MAX_IMAGE_BYTES * 4 / 3 + 128 {
                        return Err(invalid("Inline image exceeds the 8 MiB limit"));
                    }
                    let (prefix, encoded) = data
                        .split_once(",")
                        .ok_or_else(|| invalid("Invalid inline image"))?;
                    let bytes = STANDARD
                        .decode(encoded)
                        .map_err(|_| invalid("Invalid inline image encoding"))?;
                    let (mime, ext) = image_kind(&bytes).ok_or_else(|| {
                        invalid("Only PNG, JPEG, GIF and WebP image data may be stored inline")
                    })?;
                    if prefix != format!("data:{mime};base64")
                        || a.mime_type != mime
                        || bytes.len() > MAX_IMAGE_BYTES
                    {
                        return Err(invalid("Invalid inline image type or size"));
                    }
                    let name = format!("{}.{}", digest(&bytes), ext);
                    a.asset_path = Some(name.clone());
                    assets.push((name, bytes));
                } else if let Some(name) = &a.asset_path {
                    if !asset_name(name) {
                        return Err(invalid("Invalid archive asset path"));
                    }
                    self.asset(&id, name)?;
                }
            }
        }
        for (name, bytes) in assets {
            let path = dir.join("assets").join(name);
            if !path.exists() {
                atomic_write(&path, &bytes, true)?;
            } else {
                no_link(&path)?;
            }
        }
        // The observation time does not make an otherwise identical manual capture a new revision.
        let mut identity = capture.clone();
        identity.captured_at = String::new();
        let snapshot_id = digest(&serde_json::to_vec(&identity)?);
        let raw_path = dir.join("captures").join(format!("{snapshot_id}.jsonl"));
        let unchanged = raw_path.exists();
        if unchanged {
            capture = serde_json::from_slice(&bounded_read(&raw_path, MAX_CAPTURE_BYTES + 1)?)?;
        } else {
            let mut raw = serde_json::to_vec(&capture)?;
            raw.push(b'\n');
            atomic_write(&raw_path, &raw, true)?;
        }
        // Importing old evidence must not replace the current manual capture in the reader.
        let prior = if capture.capture_method == "legacy-import" && dir.join("meta.json").exists() {
            Some(self.read(&id, None)?)
        } else {
            None
        };
        let keep_prior = prior.as_ref().is_some_and(|p| {
            matches!(
                p.capture.capture_method.as_str(),
                "manual-dom" | "automatic-dom"
            ) || observation_time(&p.capture.captured_at) >= observation_time(&capture.captured_at)
        });
        let (current, current_snapshot) = if keep_prior {
            let p = prior.unwrap();
            (p.capture, p.snapshot_id)
        } else {
            (capture.clone(), snapshot_id.clone())
        };
        let (mut conversation_date, mut date_source) = original_conversation_date(&current);
        // A smaller automatic DOM window must not discard an already evidenced source date.
        // Raw revisions retain only the exact loaded window; selected revision dates stay local.
        if current.capture_method == "automatic-dom" && dir.join("meta.json").exists() {
            let prior_meta: StoredMeta =
                serde_json::from_slice(&bounded_read(&dir.join("meta.json"), 128 * 1024)?)?;
            let prior_date = prior_meta
                .summary
                .conversation_date
                .as_deref()
                .and_then(parsed_timestamp);
            let current_date = conversation_date.as_deref().and_then(parsed_timestamp);
            if prior_date.is_some() && (current_date.is_none() || prior_date > current_date) {
                conversation_date = prior_meta.summary.conversation_date;
                date_source = prior_meta.summary.date_source;
            }
        }
        let summary = ConversationSummary {
            id: id.clone(),
            conversation_id: current.conversation_id.clone(),
            provider: current.provider.clone(),
            title: current.title.clone(),
            source_url: current.source_url.clone(),
            conversation_date,
            date_source,
            captured_at: current.captured_at.clone(),
            message_count: current.messages.len(),
            snapshot_count: self.snapshots(&id)?.len(),
            completeness: current.completeness.clone(),
            warnings: current.warnings.clone(),
        };
        atomic_write(
            &dir.join("conversation.md"),
            markdown(&current).as_bytes(),
            false,
        )?;
        let searchable = format!(
            "{}\n{}",
            current.title,
            current
                .messages
                .iter()
                .map(|m| m.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
        atomic_write(&dir.join("search.txt"), searchable.as_bytes(), false)?;
        atomic_write(
            &dir.join("meta.json"),
            &serde_json::to_vec(&StoredMeta {
                summary,
                latest_snapshot: current_snapshot,
            })?,
            false,
        )?;
        Ok(SaveReceipt {
            ok: true,
            id,
            snapshot_id,
            message_count: capture.messages.len(),
            unchanged,
            completeness: capture.completeness,
            warnings: capture.warnings,
        })
    }

    pub fn read(&self, id: &str, snapshot: Option<&str>) -> Result<Conversation> {
        let dir = self.conversation_dir(id)?;
        let meta: StoredMeta =
            serde_json::from_slice(&bounded_read(&dir.join("meta.json"), 128 * 1024)?)?;
        let snapshot = snapshot.unwrap_or(&meta.latest_snapshot);
        let capture = self.read_capture(id, snapshot)?;
        let mut summary = meta.summary;
        summary.captured_at = capture.captured_at.clone();
        summary.title = capture.title.clone();
        (summary.conversation_date, summary.date_source) = original_conversation_date(&capture);
        summary.message_count = capture.messages.len();
        summary.completeness = capture.completeness.clone();
        summary.warnings = capture.warnings.clone();
        Ok(Conversation {
            summary,
            capture,
            snapshot_id: snapshot.to_owned(),
        })
    }

    fn read_capture(&self, id: &str, snapshot: &str) -> Result<Capture> {
        if !hex_id(snapshot) {
            return Err(invalid("Invalid capture snapshot identity"));
        }
        let dir = self.conversation_dir(id)?;
        no_link(&dir.join("captures"))?;
        serde_json::from_slice(&bounded_read(
            &dir.join("captures").join(format!("{snapshot}.jsonl")),
            MAX_CAPTURE_BYTES + 1,
        )?)
        .map_err(Into::into)
    }

    fn backfill_summary_date(
        &self,
        id: &str,
        latest_snapshot: &str,
        summary: &mut ConversationSummary,
    ) -> Result<()> {
        let has_date = summary
            .conversation_date
            .as_deref()
            .and_then(parsed_timestamp)
            .is_some();
        if has_date && summary.date_source.is_some() {
            return Ok(());
        }
        let capture = self.read_capture(id, latest_snapshot)?;
        let (date, source) = original_conversation_date(&capture);
        if !has_date {
            summary.conversation_date = date.clone();
        }
        if !has_date || summary.date_source.is_none() {
            summary.date_source = source;
        }
        Ok(())
    }

    pub fn snapshots(&self, id: &str) -> Result<Vec<SnapshotSummary>> {
        let dir = self.conversation_dir(id)?.join("captures");
        no_link(&dir)?;
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut out = vec![];
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| invalid("Invalid snapshot filename"))?;
            if !hex_id(name) {
                continue;
            }
            let c: Capture = serde_json::from_slice(&bounded_read(&path, MAX_CAPTURE_BYTES + 1)?)?;
            out.push(SnapshotSummary {
                snapshot_id: name.into(),
                captured_at: c.captured_at,
                message_count: c.messages.len(),
                completeness: c.completeness,
            });
        }
        out.sort_by(|a, b| {
            b.captured_at
                .cmp(&a.captured_at)
                .then_with(|| a.snapshot_id.cmp(&b.snapshot_id))
        });
        Ok(out)
    }

    pub fn list(&self, query: &str) -> Result<Vec<ConversationSummary>> {
        if query.len() > 1024 {
            return Err(invalid("Search query is too long"));
        }
        let query = query.trim().to_lowercase();
        let mut out = vec![];
        no_link(&self.root.join("conversations"))?;
        for entry in fs::read_dir(self.root.join("conversations"))? {
            let path = entry?.path();
            let Some(id) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !hex_id(id) {
                continue;
            }
            no_link(&path)?;
            if !path.join("meta.json").exists() {
                continue;
            }
            let meta: StoredMeta =
                serde_json::from_slice(&bounded_read(&path.join("meta.json"), 128 * 1024)?)?;
            if !query.is_empty() && !meta.summary.title.to_lowercase().contains(&query) {
                let text = String::from_utf8(bounded_read(
                    &path.join("search.txt"),
                    MAX_CAPTURE_BYTES + 2048,
                )?)
                .map_err(|_| invalid("Search index is not UTF-8"))?;
                if !text.to_lowercase().contains(&query) {
                    continue;
                }
            }
            let mut summary = meta.summary;
            self.backfill_summary_date(id, &meta.latest_snapshot, &mut summary)?;
            out.push(summary);
        }
        out.sort_by(summary_date_order);
        Ok(out)
    }

    pub fn export(&self, id: &str) -> Result<ExportResult> {
        self.export_snapshot(id, None)
    }

    pub fn export_snapshot(&self, id: &str, snapshot: Option<&str>) -> Result<ExportResult> {
        let _lock = self.lock()?;
        let read = self.read(id, snapshot)?;
        let markdown = markdown(&read.capture);
        let dir = self.root.join("exports");
        checked_dir(&dir)?;
        for message in &read.capture.messages {
            for attachment in &message.attachments {
                if let Some(name) = &attachment.asset_path {
                    self.asset(id, name)?;
                    let bytes = bounded_read(
                        &self.conversation_dir(id)?.join("assets").join(name),
                        MAX_IMAGE_BYTES,
                    )?;
                    let assets = dir.join("assets");
                    checked_dir(&assets)?;
                    let target = assets.join(name);
                    if !target.exists() {
                        atomic_write(&target, &bytes, true)?;
                    } else {
                        no_link(&target)?;
                    }
                }
            }
        }
        let name = match snapshot {
            Some(snapshot) => format!("{id}-{snapshot}.md"),
            None => format!("{id}.md"),
        };
        let path = dir.join(name);
        atomic_write(&path, markdown.as_bytes(), false)?;
        Ok(ExportResult {
            path: path.display().to_string(),
            markdown,
        })
    }

    pub fn asset(&self, id: &str, name: &str) -> Result<AssetResult> {
        if !asset_name(name) {
            return Err(invalid("Invalid archive asset path"));
        }
        let dir = self.conversation_dir(id)?.join("assets");
        no_link(&dir)?;
        let bytes = bounded_read(&dir.join(name), MAX_IMAGE_BYTES)?;
        let (mime, ext) =
            image_kind(&bytes).ok_or_else(|| invalid("Invalid archive image type"))?;
        if name != format!("{}.{}", digest(&bytes), ext) {
            return Err(invalid("Archive image digest does not match its identity"));
        }
        Ok(AssetResult {
            mime_type: mime.into(),
            data_url: format!("data:{mime};base64,{}", STANDARD.encode(bytes)),
        })
    }
}

pub fn markdown(c: &Capture) -> String {
    let title = c.title.replace(['\r', '\n'], " ");
    let (conversation_date, date_source) = original_conversation_date(c);
    let mut out = format!("# {title}\n\nSource: {}\n", c.source_url);
    if let (Some(date), Some(source)) = (conversation_date.as_deref(), date_source.as_deref()) {
        let label = match source {
            "provider-updated" => "Updated by provider",
            "message-authored" => "Last message authored",
            _ => "Created by provider",
        };
        out.push_str(&format!("\nOriginal conversation date ({label}): {date}\n"));
    }
    if let Some(created_at) = c
        .conversation_created_at
        .as_deref()
        .filter(|_| date_source.as_deref() != Some("provider-created"))
    {
        out.push_str(&format!(
            "\nConversation created by provider: {created_at}\n"
        ));
    }
    out.push_str(&format!(
        "\nCaptured: {}\n\nCapture: {} ({})\n",
        c.captured_at, c.capture_method, c.completeness
    ));
    for warning in &c.warnings {
        out.push_str(&format!("\n> {}\n", warning.replace(['\r', '\n'], " ")));
    }
    for m in &c.messages {
        let label = match m.role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            "system" => "System",
            _ => "Tool",
        };
        out.push_str(&format!("\n## {label}\n\n{}\n", m.text));
        for a in &m.attachments {
            let name = a.name.replace(['\r', '\n', '[', ']'], " ");
            if let Some(path) = &a.asset_path {
                out.push_str(&format!("\n![{name}](assets/{path})\n"));
            } else {
                out.push_str(&format!(
                    "\nAttachment: {name} — {}\n",
                    a.unavailable_reason
                        .as_deref()
                        .unwrap_or("original media not stored locally")
                ));
            }
        }
    }
    out
}

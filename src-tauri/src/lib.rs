use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use chrono::Utc;
use image::{DynamicImage, ImageFormat, RgbaImage};
use notify::{
    event::{EventKind, ModifyKind},
    RecommendedWatcher, RecursiveMode, Watcher,
};
use pulldown_cmark::{html, Options, Parser};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Cursor,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};
use tauri::{AppHandle, Emitter, Manager, State};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NoteMetadata {
    path: String,
    title: String,
    snippet: String,
    modified: Option<u64>,
    size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NotesSnapshot {
    root: Option<String>,
    notes: Vec<NoteMetadata>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenNote {
    note: NoteMetadata,
    content: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateOrOpenResult {
    created: bool,
    note: OpenNote,
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct AppConfig {
    notes_root: Option<String>,
}

#[derive(Default)]
struct InnerState {
    root: Option<PathBuf>,
    notes: BTreeMap<String, NoteMetadata>,
    watcher: Option<RecommendedWatcher>,
}

#[derive(Clone, Default)]
struct AppState {
    inner: Arc<Mutex<InnerState>>,
}

impl AppState {
    fn configure_root(&self, root: PathBuf, app: &AppHandle) -> Result<(), String> {
        fs::create_dir_all(&root)
            .map_err(|error| format!("Could not create notes root: {error}"))?;
        let notes = scan_notes(&root)?;
        let state_for_watcher = self.clone();
        let app_for_watcher = app.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.as_ref().is_ok_and(should_refresh_for_event) {
                    let _ = state_for_watcher.refresh_index();
                    let _ = app_for_watcher.emit("notes-changed", ());
                }
            })
            .map_err(|error| format!("Could not start file watcher: {error}"))?;

        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|error| format!("Could not watch notes root: {error}"))?;

        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "App state lock was poisoned".to_string())?;
        inner.root = Some(root);
        inner.notes = notes;
        inner.watcher = Some(watcher);
        Ok(())
    }

    fn refresh_index(&self) -> Result<(), String> {
        let root = {
            let inner = self
                .inner
                .lock()
                .map_err(|_| "App state lock was poisoned".to_string())?;
            inner.root.clone()
        };

        if let Some(root) = root {
            let notes = scan_notes(&root)?;
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "App state lock was poisoned".to_string())?;
            inner.notes = notes;
        }

        Ok(())
    }

    fn snapshot(&self) -> Result<NotesSnapshot, String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| "App state lock was poisoned".to_string())?;
        let mut notes = inner.notes.values().cloned().collect::<Vec<_>>();
        sort_notes_by_modified(&mut notes);

        Ok(NotesSnapshot {
            root: inner
                .root
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            notes,
        })
    }

    fn root(&self) -> Result<PathBuf, String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| "App state lock was poisoned".to_string())?;
        inner
            .root
            .clone()
            .ok_or_else(|| "Choose a notes root before working with notes.".to_string())
    }
}

fn should_refresh_for_event(event: &notify::Event) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Name(_))
    )
}

#[tauri::command]
fn get_notes(state: State<'_, AppState>) -> Result<NotesSnapshot, String> {
    state.snapshot()
}

#[tauri::command]
fn set_notes_root(
    path: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<NotesSnapshot, String> {
    let root = PathBuf::from(path);
    state.configure_root(root.clone(), &app)?;
    save_config(&AppConfig {
        notes_root: Some(root.to_string_lossy().to_string()),
    })?;
    state.snapshot()
}

#[tauri::command]
fn search_notes(query: String, state: State<'_, AppState>) -> Result<Vec<NoteMetadata>, String> {
    let snapshot = state.snapshot()?;
    Ok(search_notes_in_snapshot(query.trim(), snapshot.notes))
}

fn search_notes_in_snapshot(query: &str, notes: Vec<NoteMetadata>) -> Vec<NoteMetadata> {
    let normalized_query = query.trim();
    if normalized_query.is_empty() {
        let mut notes = notes;
        sort_notes_by_modified(&mut notes);
        return notes;
    }

    let mut scored = notes
        .into_iter()
        .filter_map(|note| {
            let haystack = format!("{} {} {}", note.path, note.title, note.snippet);
            fuzzy_score(normalized_query, &haystack).map(|score| (score, note))
        })
        .collect::<Vec<_>>();

    scored.sort_by(|(left_score, left_note), (right_score, right_note)| {
        right_score
            .cmp(left_score)
            .then_with(|| right_note.modified.cmp(&left_note.modified))
            .then_with(|| left_note.path.cmp(&right_note.path))
    });

    scored.into_iter().map(|(_, note)| note).collect()
}

fn sort_notes_by_modified(notes: &mut [NoteMetadata]) {
    notes.sort_by(|left, right| {
        right
            .modified
            .cmp(&left.modified)
            .then_with(|| left.path.cmp(&right.path))
    });
}

#[tauri::command]
fn open_note(path: String, state: State<'_, AppState>) -> Result<OpenNote, String> {
    open_note_from_path(&path, &state)
}

#[tauri::command]
fn save_note(
    path: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<NoteMetadata, String> {
    let root = state.root()?;
    let note_path = resolve_note_path(&root, &path)?;
    if let Some(parent) = note_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create note folder: {error}"))?;
    }
    fs::write(&note_path, content).map_err(|error| format!("Could not save note: {error}"))?;
    state.refresh_index()?;
    metadata_for_file(&root, &note_path)
}

#[tauri::command]
fn delete_note(path: String, state: State<'_, AppState>) -> Result<NotesSnapshot, String> {
    let root = state.root()?;
    let note_path = resolve_note_path(&root, &path)?;

    if !note_path.exists() {
        return Err("Note does not exist.".to_string());
    }
    if !note_path.is_file() || !is_markdown_file(&note_path) {
        return Err("Only Markdown note files can be deleted.".to_string());
    }

    delete_note_attachments(&note_path)?;
    fs::remove_file(&note_path).map_err(|error| format!("Could not delete note: {error}"))?;
    state.refresh_index()?;
    state.snapshot()
}

fn delete_note_attachments(note_path: &Path) -> Result<(), String> {
    let Some(note_dir) = note_path.parent() else {
        return Ok(());
    };
    let attachments_dir = note_dir.join("attachments");
    if !attachments_dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(&attachments_dir)
        .map_err(|error| format!("Could not read attachments folder: {error}"))?
    {
        let entry = entry.map_err(|error| format!("Could not read attachment entry: {error}"))?;
        let path = entry.path();
        if path.is_file() && is_image_file(&path) {
            fs::remove_file(&path).map_err(|error| {
                format!("Could not delete attachment {}: {error}", path.display())
            })?;
        }
    }

    let mut remaining = fs::read_dir(&attachments_dir)
        .map_err(|error| format!("Could not re-read attachments folder: {error}"))?;
    if remaining.next().is_none() {
        fs::remove_dir(&attachments_dir)
            .map_err(|error| format!("Could not delete empty attachments folder: {error}"))?;
    }

    Ok(())
}

#[tauri::command]
fn create_or_open_note(
    query: String,
    state: State<'_, AppState>,
) -> Result<CreateOrOpenResult, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("Type a note name before creating a note.".to_string());
    }

    if let Some(existing) = find_openable_match(query, &state)? {
        return Ok(CreateOrOpenResult {
            created: false,
            note: existing,
        });
    }

    let root = state.root()?;
    let relative_path = note_name_from_query(query)?;
    let note_path = resolve_note_path(&root, &relative_path)?;
    if let Some(parent) = note_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create note folder: {error}"))?;
    }
    if !note_path.exists() {
        fs::write(&note_path, "").map_err(|error| format!("Could not create note: {error}"))?;
    }

    state.refresh_index()?;
    Ok(CreateOrOpenResult {
        created: true,
        note: open_note_from_path(&relative_path, &state)?,
    })
}

#[tauri::command]
fn render_markdown(content: String) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(&content, options);
    let mut rendered = String::new();
    html::push_html(&mut rendered, parser);
    rendered
}

#[tauri::command]
fn process_image_paste(
    note_path: String,
    image_bytes: Vec<u8>,
    extension: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    save_pasted_image(note_path, image_bytes, extension, state)
}

#[tauri::command]
fn process_image_paste_base64(
    note_path: String,
    image_base64: String,
    extension: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let image_bytes = BASE64_STANDARD
        .decode(image_base64)
        .map_err(|error| format!("Could not decode pasted image: {error}"))?;
    save_pasted_image(note_path, image_bytes, extension, state)
}

#[tauri::command]
fn process_clipboard_image_paste(
    note_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("Could not access clipboard: {error}"))?;
    let image = clipboard
        .get_image()
        .map_err(|error| format!("Clipboard does not contain an image: {error}"))?;
    let rgba = RgbaImage::from_raw(
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    )
    .ok_or_else(|| "Clipboard image data was invalid.".to_string())?;

    let mut png = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(rgba)
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|error| format!("Could not encode clipboard image: {error}"))?;

    save_pasted_image(note_path, png.into_inner(), Some("png".to_string()), state)
}

#[tauri::command]
fn process_image_file_paste(
    note_path: String,
    image_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let path = PathBuf::from(&image_path);
    if !path.is_absolute() {
        return Err("Pasted image file path must be absolute.".to_string());
    }
    if !path.is_file() {
        return Err("Pasted image file does not exist.".to_string());
    }

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_string());
    let image_bytes =
        fs::read(&path).map_err(|error| format!("Could not read pasted image file: {error}"))?;

    save_pasted_image(note_path, image_bytes, extension, state)
}

fn save_pasted_image(
    note_path: String,
    image_bytes: Vec<u8>,
    extension: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    if image_bytes.is_empty() {
        return Err("Pasted image was empty.".to_string());
    }

    let root = state.root()?;
    let absolute_note_path = resolve_note_path(&root, &note_path)?;
    let note_dir = absolute_note_path
        .parent()
        .ok_or_else(|| "Could not determine note folder.".to_string())?;
    let attachments_dir = note_dir.join("attachments");
    fs::create_dir_all(&attachments_dir)
        .map_err(|error| format!("Could not create attachments folder: {error}"))?;

    let extension = normalize_extension(extension.as_deref());
    let mut hasher = Sha256::new();
    hasher.update(&image_bytes);
    let hash = format!("{:x}", hasher.finalize());
    let short_hash = &hash[..12];
    let timestamp = Utc::now().format("%Y%m%d%H%M%S%3f");
    let filename = format!("{timestamp}_{short_hash}.{extension}");
    let attachment_path = attachments_dir.join(&filename);

    fs::write(&attachment_path, image_bytes)
        .map_err(|error| format!("Could not save pasted image: {error}"))?;

    state.refresh_index()?;
    Ok(format!("![](attachments/{filename})"))
}

#[tauri::command]
fn resolve_note_asset(
    note_path: String,
    asset_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let root = state.root()?;
    let absolute_note_path = resolve_note_path(&root, &note_path)?;
    let note_dir = absolute_note_path
        .parent()
        .ok_or_else(|| "Could not determine note folder.".to_string())?;
    let asset = normalize_relative_path(&asset_path)?;
    let absolute_asset = note_dir.join(asset);

    if !absolute_asset.starts_with(&root) {
        return Err("Asset path escapes the notes root.".to_string());
    }

    Ok(absolute_asset.to_string_lossy().to_string())
}

fn find_openable_match(query: &str, state: &AppState) -> Result<Option<OpenNote>, String> {
    let snapshot = state.snapshot()?;
    let results = search_notes_in_snapshot(query, snapshot.notes);
    if let Some(note) = results.first() {
        let normalized = query.to_lowercase();
        let exactish = note.path.to_lowercase() == normalized
            || note.title.to_lowercase() == normalized
            || note.path.to_lowercase() == format!("{normalized}.md");
        if exactish || fuzzy_score(query, &note.path).unwrap_or(0) > 80 {
            return open_note_from_path(&note.path, state).map(Some);
        }
    }

    Ok(None)
}

fn open_note_from_path(path: &str, state: &AppState) -> Result<OpenNote, String> {
    let root = state.root()?;
    let note_path = resolve_note_path(&root, path)?;
    let content =
        fs::read_to_string(&note_path).map_err(|error| format!("Could not read note: {error}"))?;
    let note = metadata_for_file(&root, &note_path)?;
    Ok(OpenNote { note, content })
}

fn scan_notes(root: &Path) -> Result<BTreeMap<String, NoteMetadata>, String> {
    let mut notes = BTreeMap::new();
    if !root.exists() {
        return Ok(notes);
    }

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.path();
        if is_markdown_file(path) {
            let metadata = metadata_for_file(root, path)?;
            notes.insert(metadata.path.clone(), metadata);
        }
    }

    Ok(notes)
}

fn metadata_for_file(root: &Path, path: &Path) -> Result<NoteMetadata, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Note path is outside the notes root.".to_string())?;
    let relative_path = relative.to_string_lossy().replace('\\', "/");
    let title = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| relative_path.clone());
    let file_metadata =
        fs::metadata(path).map_err(|error| format!("Could not read note metadata: {error}"))?;
    let modified = file_metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());
    let content = fs::read_to_string(path).unwrap_or_default();
    let snippet = content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .chars()
        .take(180)
        .collect();

    Ok(NoteMetadata {
        path: relative_path,
        title,
        snippet,
        modified,
        size: file_metadata.len(),
    })
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown"))
        .unwrap_or(false)
}

fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp"
            )
        })
        .unwrap_or(false)
}

fn note_name_from_query(query: &str) -> Result<String, String> {
    let normalized = query.trim().replace('\\', "/");
    if normalized.is_empty() {
        return Err("Note name cannot be empty.".to_string());
    }
    let with_extension = if Path::new(&normalized).extension().is_some() {
        normalized
    } else {
        format!("{normalized}.md")
    };
    normalize_relative_path(&with_extension)
}

fn resolve_note_path(root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let normalized = normalize_relative_path(relative_path)?;
    let path = root.join(normalized);
    if !path.starts_with(root) {
        return Err("Note path escapes the notes root.".to_string());
    }
    Ok(path)
}

fn normalize_relative_path(relative_path: &str) -> Result<String, String> {
    let path = Path::new(relative_path);
    if path.is_absolute() {
        return Err("Absolute paths are not allowed.".to_string());
    }

    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let text = part.to_string_lossy();
                if text.is_empty() {
                    return Err("Path contains an empty segment.".to_string());
                }
                parts.push(text.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => return Err("Parent path segments are not allowed.".to_string()),
            Component::RootDir | Component::Prefix(_) => {
                return Err("Absolute paths are not allowed.".to_string());
            }
        }
    }

    if parts.is_empty() {
        return Err("Path cannot be empty.".to_string());
    }

    Ok(parts.join("/"))
}

fn normalize_extension(extension: Option<&str>) -> String {
    extension
        .and_then(|extension| extension.split('/').last())
        .map(|extension| extension.trim_start_matches('.').to_ascii_lowercase())
        .filter(|extension| matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp"))
        .unwrap_or_else(|| "png".to_string())
}

fn fuzzy_score(query: &str, text: &str) -> Option<i64> {
    let query = query.to_lowercase();
    let text = text.to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    if text.contains(&query) {
        return Some(1000 - text.find(&query).unwrap_or(0) as i64);
    }

    let mut score = 0;
    let mut query_chars = query.chars();
    let mut current = query_chars.next()?;
    let mut streak = 0;
    for text_char in text.chars() {
        if text_char == current {
            streak += 1;
            score += 10 + streak * 3;
            if let Some(next) = query_chars.next() {
                current = next;
            } else {
                return Some(score);
            }
        } else {
            streak = 0;
            score -= 1;
        }
    }

    None
}

fn config_path() -> Result<PathBuf, String> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| "Could not determine the user config directory.".to_string())?
        .join("PureType");
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("Could not create config directory: {error}"))?;
    Ok(config_dir.join("config.json"))
}

fn load_config() -> AppConfig {
    config_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path()?;
    let content = serde_json::to_string_pretty(config)
        .map_err(|error| format!("Could not serialize config: {error}"))?;
    fs::write(path, content).map_err(|error| format!("Could not save config: {error}"))
}

pub fn run() {
    let mut context = tauri::generate_context!();
    context.set_default_window_icon(None);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            let state = app.state::<AppState>();
            if let Some(root) = load_config().notes_root {
                if let Err(error) = state.configure_root(PathBuf::from(root), app.handle()) {
                    eprintln!("Could not restore notes root: {error}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_notes,
            set_notes_root,
            search_notes,
            open_note,
            save_note,
            delete_note,
            create_or_open_note,
            render_markdown,
            process_image_paste,
            process_image_paste_base64,
            process_clipboard_image_paste,
            process_image_file_paste,
            resolve_note_asset
        ])
        .run(context)
        .expect("error while running PureType");
}

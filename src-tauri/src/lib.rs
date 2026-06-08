use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use chrono::{DateTime, Local, Utc};
use image::{DynamicImage, ImageFormat, RgbaImage};
use notify::{
    event::{EventKind, ModifyKind},
    RecommendedWatcher, RecursiveMode, Watcher,
};
use pulldown_cmark::{html, Options, Parser};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    fs,
    io::Cursor,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, UNIX_EPOCH},
};
use sublime_fuzzy::FuzzySearch;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Size, State, WebviewWindow,
    WindowEvent,
};
use walkdir::WalkDir;

const SEARCH_TIER_TITLE: i64 = 100_000;
const SEARCH_TIER_PATH: i64 = 10_000;
const SEARCH_TIER_SNIPPET: i64 = 1_000;
const SEARCH_MIN_NOTE_SCORE: i64 = 1_000;
const SEARCH_LITERAL_EXACT: i64 = 50_000;
const SEARCH_LITERAL_PREFIX: i64 = 40_000;
const SEARCH_LITERAL_CONTAINS: i64 = 30_000;
const SEARCH_FUZZY_MIN_ABSOLUTE: isize = 50;
const SEARCH_EXCERPT_MAX_CHARS: usize = 500;

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RenameNoteResult {
    old_path: String,
    new_path: String,
    snapshot: NotesSnapshot,
}

const DEFAULT_UI_SCALE: i32 = 0;
const DEFAULT_THEME: &str = "dark";
const DEFAULT_WINDOW_WIDTH: u32 = 1280;
const DEFAULT_WINDOW_HEIGHT: u32 = 820;
const MIN_WINDOW_WIDTH: u32 = 900;
const MIN_WINDOW_HEIGHT: u32 = 600;
const WINDOW_VIEWPORT_RESIZE_IGNORE_MS: u64 = 2500;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowConfig {
    x: Option<i32>,
    y: Option<i32>,
    width: Option<u32>,
    height: Option<u32>,
    maximized: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppConfig {
    notes_root: Option<String>,
    ui_scale: Option<i32>,
    theme: Option<String>,
    window: Option<WindowConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            notes_root: None,
            ui_scale: Some(DEFAULT_UI_SCALE),
            theme: Some(DEFAULT_THEME.to_string()),
            window: None,
        }
    }
}

#[derive(Default)]
struct WindowPersistState {
    target_logical_size: Mutex<Option<(u32, u32)>>,
    ignore_viewport_resize_until: Mutex<Option<Instant>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WorkArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl WorkArea {
    fn contains_point(self, px: i32, py: i32) -> bool {
        px >= self.x
            && py >= self.y
            && px < self.x + self.width as i32
            && py < self.y + self.height as i32
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppConfigResponse {
    notes_root: Option<String>,
    ui_scale: Option<i32>,
    theme: Option<String>,
    first_run: bool,
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
    let mut config = load_config();
    config.notes_root = Some(root.to_string_lossy().to_string());
    save_config(&config)?;
    state.snapshot()
}

#[tauri::command]
fn get_app_config() -> AppConfigResponse {
    let first_run = !config_file_exists();
    let config = load_config();
    AppConfigResponse {
        notes_root: config.notes_root,
        ui_scale: config.ui_scale,
        theme: config.theme,
        first_run,
    }
}

#[tauri::command]
fn save_ui_scale(ui_scale: i32) -> Result<AppConfig, String> {
    let mut config = load_config();
    config.ui_scale = Some(ui_scale);
    save_config(&config)?;
    Ok(config)
}

#[tauri::command]
fn save_theme(theme: String) -> Result<AppConfig, String> {
    let normalized = theme.trim().to_ascii_lowercase();
    if !matches!(
        normalized.as_str(),
        "dark" | "light" | "solarized" | "hacker" | "orange-hacker" | "vga-437" | "vga-blue" | "speccy" | "vt" | "mf-3270"
    ) {
        return Err("Unknown theme.".to_string());
    }
    let mut config = load_config();
    config.theme = Some(normalized);
    save_config(&config)?;
    Ok(config)
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
        .filter_map(|note| note_search_score(normalized_query, &note).map(|score| (score, note)))
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

    fs::remove_file(&note_path).map_err(|error| format!("Could not delete note: {error}"))?;
    state.refresh_index()?;
    state.snapshot()
}

#[tauri::command]
fn rename_note(
    path: String,
    new_filename: String,
    state: State<'_, AppState>,
) -> Result<RenameNoteResult, String> {
    let root = state.root()?;
    let note_path = resolve_note_path(&root, &path)?;

    if !note_path.exists() {
        return Err("Note does not exist.".to_string());
    }
    if !note_path.is_file() || !is_markdown_file(&note_path) {
        return Err("Only Markdown note files can be renamed.".to_string());
    }

    let filename = normalize_note_filename_stem(&new_filename)?;
    let extension = note_path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("md");
    let new_file_name = format!("{filename}.{extension}");
    let new_note_path = note_path
        .parent()
        .ok_or_else(|| "Could not determine note folder.".to_string())?
        .join(new_file_name);

    if new_note_path == note_path {
        let snapshot = state.snapshot()?;
        return Ok(RenameNoteResult {
            old_path: path.clone(),
            new_path: path,
            snapshot,
        });
    }
    if new_note_path.exists() {
        return Err("A note with that name already exists.".to_string());
    }
    if !new_note_path.starts_with(&root) {
        return Err("Renamed note path escapes the notes root.".to_string());
    }

    fs::rename(&note_path, &new_note_path)
        .map_err(|error| format!("Could not rename note: {error}"))?;
    state.refresh_index()?;
    let new_path = relative_note_path(&root, &new_note_path)?;
    let snapshot = state.snapshot()?;

    Ok(RenameNoteResult {
        old_path: path,
        new_path,
        snapshot,
    })
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

fn quick_note_stem(now: DateTime<Local>) -> String {
    now.format("QN %Y-%m-%d %H-%M").to_string()
}

fn unique_quick_note_path(root: &Path, stem: &str) -> Result<String, String> {
    let mut candidate = format!("{stem}.md");
    if !resolve_note_path(root, &candidate)?.exists() {
        return Ok(candidate);
    }

    let mut suffix = 2;
    loop {
        candidate = format!("{stem} ({suffix}).md");
        if !resolve_note_path(root, &candidate)?.exists() {
            return Ok(candidate);
        }
        suffix += 1;
    }
}

#[tauri::command]
fn create_quick_note(state: State<'_, AppState>) -> Result<CreateOrOpenResult, String> {
    let root = state.root()?;
    let stem = quick_note_stem(Local::now());
    let relative_path = unique_quick_note_path(&root, &stem)?;
    let note_path = resolve_note_path(&root, &relative_path)?;
    fs::write(&note_path, "").map_err(|error| format!("Could not create note: {error}"))?;
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

#[tauri::command]
fn copy_image_to_clipboard(
    note_path: String,
    asset_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
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
    if !absolute_asset.is_file() || !is_image_file(&absolute_asset) {
        return Err("Only local image files can be copied.".to_string());
    }

    let image_bytes =
        fs::read(&absolute_asset).map_err(|error| format!("Could not read image: {error}"))?;
    let image = image::load_from_memory(&image_bytes)
        .map_err(|error| format!("Could not decode image: {error}"))?
        .to_rgba8();
    let width = image.width() as usize;
    let height = image.height() as usize;
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("Could not access clipboard: {error}"))?;
    clipboard
        .set_image(arboard::ImageData {
            width,
            height,
            bytes: Cow::Owned(image.into_raw()),
        })
        .map_err(|error| format!("Could not copy image: {error}"))
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
        if note_opens_from_search_query(query.trim(), note) {
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
    let relative_path = relative_note_path(root, path)?;
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
    let snippet = build_search_excerpt(&content);

    Ok(NoteMetadata {
        path: relative_path,
        title,
        snippet,
        modified,
        size: file_metadata.len(),
    })
}

fn relative_note_path(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Note path is outside the notes root.".to_string())?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
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

fn normalize_note_filename_stem(filename: &str) -> Result<String, String> {
    let filename = filename.trim();
    if filename.is_empty() {
        return Err("Note name cannot be empty.".to_string());
    }
    if filename == "." || filename == ".." {
        return Err("Note name is not valid.".to_string());
    }
    if filename.contains('/') || filename.contains('\\') {
        return Err("Only the note filename can be changed.".to_string());
    }
    let lowercase = filename.to_ascii_lowercase();
    if lowercase.ends_with(".md") || lowercase.ends_with(".markdown") {
        return Err("Do not include the file extension.".to_string());
    }

    Ok(filename.to_string())
}

fn normalize_extension(extension: Option<&str>) -> String {
    extension
        .and_then(|extension| extension.split('/').last())
        .map(|extension| extension.trim_start_matches('.').to_ascii_lowercase())
        .filter(|extension| matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp"))
        .unwrap_or_else(|| "png".to_string())
}

fn build_search_excerpt(content: &str) -> String {
    let mut excerpt = String::new();
    for line in content.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if !excerpt.is_empty() {
            excerpt.push(' ');
        }
        excerpt.push_str(line);
        if excerpt.chars().count() >= SEARCH_EXCERPT_MAX_CHARS {
            break;
        }
    }
    excerpt.chars().take(SEARCH_EXCERPT_MAX_CHARS).collect()
}

fn field_search_score(query: &str, text: &str, tier_boost: i64) -> Option<i64> {
    if query.is_empty() {
        return Some(tier_boost);
    }

    let text_lower = text.to_lowercase();
    let query_lower = query.to_lowercase();

    if text_lower == query_lower {
        return Some(tier_boost + SEARCH_LITERAL_EXACT);
    }
    if text_lower.starts_with(&query_lower) {
        return Some(tier_boost + SEARCH_LITERAL_PREFIX);
    }
    if let Some(position) = text_lower.find(&query_lower) {
        return Some(tier_boost + SEARCH_LITERAL_CONTAINS - position as i64);
    }

    let min_fuzzy = std::cmp::max(
        SEARCH_FUZZY_MIN_ABSOLUTE,
        query.chars().count() as isize * 8,
    );
    let matched = FuzzySearch::new(query, text).best_match()?;
    if matched.score() < min_fuzzy {
        return None;
    }

    Some(tier_boost + matched.score() as i64)
}

fn note_search_score(query: &str, note: &NoteMetadata) -> Option<i64> {
    [
        field_search_score(query, &note.title, SEARCH_TIER_TITLE),
        field_search_score(query, &note.path, SEARCH_TIER_PATH),
        field_search_score(query, &note.snippet, SEARCH_TIER_SNIPPET),
    ]
    .into_iter()
    .flatten()
    .max()
    .filter(|score| *score >= SEARCH_MIN_NOTE_SCORE)
}

fn note_opens_from_search_query(query: &str, note: &NoteMetadata) -> bool {
    let normalized = query.to_lowercase();
    if normalized.is_empty() {
        return false;
    }

    let path_lower = note.path.to_lowercase();
    let title_lower = note.title.to_lowercase();
    let exactish = title_lower == normalized
        || path_lower == normalized
        || path_lower == format!("{normalized}.md")
        || path_lower.ends_with(&format!("/{normalized}.md"));

    if exactish {
        return true;
    }

    note_search_score(query, note)
        .is_some_and(|score| score >= SEARCH_TIER_PATH + SEARCH_LITERAL_CONTAINS)
}

fn is_wayland_session() -> bool {
    if std::env::var("XDG_SESSION_TYPE").is_ok_and(|session| session.eq_ignore_ascii_case("wayland"))
    {
        return true;
    }

    std::env::var_os("WAYLAND_DISPLAY").is_some()
        && !std::env::var("XDG_SESSION_TYPE").is_ok_and(|session| session.eq_ignore_ascii_case("x11"))
}

fn can_persist_window_position() -> bool {
    !is_wayland_session()
}

fn logical_inner_size(window: &WebviewWindow) -> Result<(u32, u32), String> {
    let physical = window.inner_size().map_err(|error| error.to_string())?;
    let scale = window.scale_factor().map_err(|error| error.to_string())?;
    let logical = physical.to_logical::<f64>(scale);
    Ok((logical.width.round() as u32, logical.height.round() as u32))
}

fn apply_logical_inner_size(
    window: &WebviewWindow,
    width: u32,
    height: u32,
) -> Result<(), String> {
    window
        .set_size(Size::Logical(LogicalSize::new(width as f64, height as f64)))
        .map_err(|error| error.to_string())
}

fn minimum_physical_window_size(scale: f64) -> (u32, u32) {
    (
        (MIN_WINDOW_WIDTH as f64 * scale).round() as u32,
        (MIN_WINDOW_HEIGHT as f64 * scale).round() as u32,
    )
}

fn work_area_from_monitor(monitor: &tauri::Monitor) -> WorkArea {
    let area = monitor.work_area();
    WorkArea {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width,
        height: area.size.height,
    }
}

fn constrain_window_bounds(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    monitors: &[WorkArea],
    primary: Option<WorkArea>,
    min_width: u32,
    min_height: u32,
) -> (i32, i32, u32, u32) {
    if monitors.is_empty() {
        let width = width
            .max(min_width)
            .min(DEFAULT_WINDOW_WIDTH);
        let height = height
            .max(min_height)
            .min(DEFAULT_WINDOW_HEIGHT);
        return (x, y, width, height);
    }

    let center_x = x + width as i32 / 2;
    let center_y = y + height as i32 / 2;

    let monitor = monitors
        .iter()
        .copied()
        .find(|monitor| monitor.contains_point(center_x, center_y))
        .or_else(|| {
            monitors
                .iter()
                .copied()
                .find(|monitor| monitor.contains_point(x, y))
        })
        .or(primary)
        .or_else(|| monitors.first().copied())
        .expect("monitors is not empty");

    let min_width = min_width.min(monitor.width);
    let min_height = min_height.min(monitor.height);
    let width = width.clamp(min_width, monitor.width);
    let height = height.clamp(min_height, monitor.height);

    let max_x = monitor.x + monitor.width as i32 - width as i32;
    let max_y = monitor.y + monitor.height as i32 - height as i32;
    let x = x.clamp(monitor.x, max_x.max(monitor.x));
    let y = y.clamp(monitor.y, max_y.max(monitor.y));

    (x, y, width, height)
}

fn set_target_logical_size(persist_state: &WindowPersistState, width: u32, height: u32) {
    if let Ok(mut target) = persist_state.target_logical_size.lock() {
        *target = Some((width, height));
    }
}

fn begin_viewport_resize_ignore(persist_state: &WindowPersistState) {
    if let Ok(mut ignore_until) = persist_state.ignore_viewport_resize_until.lock() {
        *ignore_until =
            Some(Instant::now() + Duration::from_millis(WINDOW_VIEWPORT_RESIZE_IGNORE_MS));
    }
}

fn viewport_resize_updates_allowed(persist_state: &WindowPersistState) -> bool {
    persist_state
        .ignore_viewport_resize_until
        .lock()
        .ok()
        .and_then(|ignore_until| *ignore_until)
        .is_none_or(|until| Instant::now() >= until)
}

fn persisted_logical_size(
    window: &WebviewWindow,
    persist_state: &WindowPersistState,
) -> Result<(u32, u32), String> {
    if let Ok(target) = persist_state.target_logical_size.lock() {
        if let Some(size) = *target {
            return Ok(size);
        }
    }
    logical_inner_size(window)
}

fn persist_window_state(
    window: &WebviewWindow,
    persist_state: &WindowPersistState,
) -> Result<(), String> {
    let maximized = window.is_maximized().map_err(|error| error.to_string())?;
    let mut config = load_config();
    let window_config = config.window.get_or_insert_with(WindowConfig::default);

    if maximized {
        window_config.maximized = Some(true);
    } else {
        let (width, height) = persisted_logical_size(window, persist_state)?;
        if can_persist_window_position() {
            let position = window.outer_position().map_err(|error| error.to_string())?;
            window_config.x = Some(position.x);
            window_config.y = Some(position.y);
        } else {
            window_config.x = None;
            window_config.y = None;
        }
        window_config.width = Some(width);
        window_config.height = Some(height);
        window_config.maximized = Some(false);
    }

    save_config(&config)
}

fn restore_window_from_config(
    window: &WebviewWindow,
    config: &WindowConfig,
) -> Result<(u32, u32), String> {
    let mut width = config
        .width
        .ok_or_else(|| "Saved window width is missing.".to_string())?;
    let mut height = config
        .height
        .ok_or_else(|| "Saved window height is missing.".to_string())?;
    let scale = window.scale_factor().map_err(|error| error.to_string())?;
    let (min_width, min_height) = minimum_physical_window_size(scale);

    let monitors = window
        .available_monitors()
        .map_err(|error| error.to_string())?
        .iter()
        .map(work_area_from_monitor)
        .collect::<Vec<_>>();
    let primary = window
        .primary_monitor()
        .map_err(|error| error.to_string())?
        .as_ref()
        .map(work_area_from_monitor);

    let physical_width = (width as f64 * scale).round() as u32;
    let physical_height = (height as f64 * scale).round() as u32;
    let (_, _, physical_width, physical_height) = constrain_window_bounds(
        0,
        0,
        physical_width,
        physical_height,
        &monitors,
        primary,
        min_width,
        min_height,
    );
    width = (physical_width as f64 / scale).round() as u32;
    height = (physical_height as f64 / scale).round() as u32;

    apply_logical_inner_size(window, width, height)?;

    if can_persist_window_position() {
        if let (Some(x), Some(y)) = (config.x, config.y) {
            let (x, y, _, _) = constrain_window_bounds(
                x,
                y,
                physical_width,
                physical_height,
                &monitors,
                primary,
                min_width,
                min_height,
            );
            window
                .set_position(PhysicalPosition::new(x, y))
                .map_err(|error| error.to_string())?;
        }
    }

    if config.maximized.unwrap_or(false) {
        window.maximize().map_err(|error| error.to_string())?;
    }

    Ok((width, height))
}

#[tauri::command]
fn note_viewport_logical_size(
    app: AppHandle,
    width: u32,
    height: u32,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Ok(());
    }

    let persist_state = app.state::<WindowPersistState>();
    if !viewport_resize_updates_allowed(persist_state.inner()) {
        return Ok(());
    }

    set_target_logical_size(persist_state.inner(), width, height);
    Ok(())
}

#[tauri::command]
fn restore_window_state(app: AppHandle) -> Result<(), String> {
    let config = load_config();
    let Some(window_config) = config.window else {
        return Ok(());
    };
    if window_config.width.is_none() || window_config.height.is_none() {
        return Ok(());
    }

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window is not available.".to_string())?;
    let (width, height) = restore_window_from_config(&window, &window_config)?;
    let persist_state = app.state::<WindowPersistState>();
    set_target_logical_size(persist_state.inner(), width, height);
    begin_viewport_resize_ignore(persist_state.inner());
    Ok(())
}

const CONFIG_DIR_NAME: &str = "echo";

fn config_path() -> Result<PathBuf, String> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| "Could not determine the user config directory.".to_string())?
        .join(CONFIG_DIR_NAME);
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("Could not create config directory: {error}"))?;
    Ok(config_dir.join("config.json"))
}

fn config_file_exists() -> bool {
    config_path().is_ok_and(|path| path.exists())
}

fn load_config() -> AppConfig {
    config_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn apply_config_defaults(config: &mut AppConfig) {
    if config.ui_scale.is_none() {
        config.ui_scale = Some(DEFAULT_UI_SCALE);
    }
    if config.theme.is_none() {
        config.theme = Some(DEFAULT_THEME.to_string());
    }
}

fn save_config(config: &AppConfig) -> Result<(), String> {
    let mut config = config.clone();
    apply_config_defaults(&mut config);
    let path = config_path()?;
    let content = serde_json::to_string_pretty(&config)
        .map_err(|error| format!("Could not serialize config: {error}"))?;
    fs::write(path, content).map_err(|error| format!("Could not save config: {error}"))
}

#[cfg(test)]
mod search_tests {
    use super::*;

    fn note(path: &str, title: &str, snippet: &str) -> NoteMetadata {
        NoteMetadata {
            path: path.to_string(),
            title: title.to_string(),
            snippet: snippet.to_string(),
            modified: Some(1),
            size: 1,
        }
    }

    #[test]
    fn empty_query_returns_all_notes_sorted() {
        let notes = vec![
            note("b.md", "Bravo", ""),
            note("a.md", "Alpha", ""),
        ];
        let results = search_notes_in_snapshot("", notes);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].path, "a.md");
    }

    #[test]
    fn title_match_ranks_above_body_only_match() {
        let notes = vec![
            note("body.md", "Other", "contains meeting notes from last week"),
            note("title.md", "Meeting", "unrelated summary text"),
        ];
        let results = search_notes_in_snapshot("meeting", notes);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].path, "title.md");
    }

    #[test]
    fn search_is_case_insensitive() {
        let notes = vec![note("note.md", "Meeting", "")];
        let results = search_notes_in_snapshot("MEET", notes);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Meeting");
    }

    #[test]
    fn unrelated_subsequence_match_is_excluded() {
        let notes = vec![note(
            "unrelated.md",
            "Zebra",
            "qwerty uiop asdf ghjkl",
        )];
        let results = search_notes_in_snapshot("daily", notes);
        assert!(results.is_empty());
    }

    #[test]
    fn title_prefix_scores_higher_than_snippet_contains() {
        let title_score = field_search_score("meet", "Meeting", SEARCH_TIER_TITLE).unwrap();
        let snippet_score =
            field_search_score("meet", "last week meeting notes", SEARCH_TIER_SNIPPET).unwrap();
        assert!(title_score > snippet_score);
    }

    #[test]
    fn build_search_excerpt_collapses_multiple_lines() {
        let content = "# Title\n\nFirst paragraph line.\n\nSecond paragraph line.";
        let excerpt = build_search_excerpt(content);
        assert!(excerpt.contains("First paragraph"));
        assert!(excerpt.contains("Second paragraph"));
    }

    #[test]
    fn note_opens_on_exact_title_match() {
        let target = note("notes/meeting.md", "Meeting", "");
        assert!(note_opens_from_search_query("Meeting", &target));
    }
}

#[cfg(test)]
mod quick_note_tests {
    use super::*;
    use chrono::TimeZone;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("echo-quick-note-{nanos}"));
        fs::create_dir_all(&path).expect("temp root should be created");
        path
    }

    #[test]
    fn quick_note_stem_uses_local_format_without_colons() {
        let now = Local
            .with_ymd_and_hms(2026, 5, 22, 14, 30, 0)
            .single()
            .expect("valid local datetime");
        assert_eq!(quick_note_stem(now), "QN 2026-05-22 14-30");
    }

    #[test]
    fn unique_quick_note_path_is_top_level() {
        let root = temp_root();
        let path = unique_quick_note_path(&root, "QN 2026-05-22 14-30").expect("path");
        assert_eq!(path, "QN 2026-05-22 14-30.md");
        assert!(!path.contains('/'));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unique_quick_note_path_adds_suffix_on_collision() {
        let root = temp_root();
        let stem = "QN 2026-05-22 14-30";
        let first = unique_quick_note_path(&root, stem).expect("first path");
        fs::write(root.join(&first), "").expect("first note");
        let second = unique_quick_note_path(&root, stem).expect("second path");
        assert_eq!(second, "QN 2026-05-22 14-30 (2).md");
        let _ = fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;

    const MONITOR: WorkArea = WorkArea {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };

    #[test]
    fn window_fully_on_monitor_is_unchanged() {
        let (x, y, width, height) = constrain_window_bounds(
            100,
            100,
            1280,
            820,
            &[MONITOR],
            Some(MONITOR),
            MIN_WINDOW_WIDTH,
            MIN_WINDOW_HEIGHT,
        );
        assert_eq!((x, y, width, height), (100, 100, 1280, 820));
    }

    #[test]
    fn window_partially_off_screen_is_clamped() {
        let (x, y, width, height) = constrain_window_bounds(
            -50,
            -30,
            1280,
            820,
            &[MONITOR],
            Some(MONITOR),
            MIN_WINDOW_WIDTH,
            MIN_WINDOW_HEIGHT,
        );
        assert_eq!((x, y, width, height), (0, 0, 1280, 820));
    }

    #[test]
    fn oversized_window_is_shrunk_to_monitor() {
        let small = WorkArea {
            x: 0,
            y: 0,
            width: 1024,
            height: 768,
        };
        let (_, _, width, height) = constrain_window_bounds(
            0,
            0,
            2000,
            1500,
            &[small],
            Some(small),
            MIN_WINDOW_WIDTH,
            MIN_WINDOW_HEIGHT,
        );
        assert_eq!((width, height), (1024, 768));
    }

    #[test]
    fn disconnected_monitor_position_moves_to_primary() {
        let (x, y, width, height) = constrain_window_bounds(
            2500,
            100,
            1280,
            820,
            &[MONITOR],
            Some(MONITOR),
            MIN_WINDOW_WIDTH,
            MIN_WINDOW_HEIGHT,
        );
        assert_eq!((width, height), (1280, 820));
        assert!(x >= MONITOR.x);
        assert!(y >= MONITOR.y);
        assert!(x + width as i32 <= MONITOR.x + MONITOR.width as i32);
        assert!(y + height as i32 <= MONITOR.y + MONITOR.height as i32);
    }

    #[test]
    fn undersized_window_is_raised_to_minimum() {
        let (_, _, width, height) = constrain_window_bounds(
            0,
            0,
            400,
            300,
            &[MONITOR],
            Some(MONITOR),
            MIN_WINDOW_WIDTH,
            MIN_WINDOW_HEIGHT,
        );
        assert_eq!((width, height), (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT));
    }

    #[test]
    fn window_config_serializes_as_nested_camel_case_object() {
        let config = AppConfig {
            notes_root: Some("/tmp/notes".to_string()),
            ui_scale: Some(0),
            theme: Some("dark".to_string()),
            window: Some(WindowConfig {
                x: Some(120),
                y: Some(80),
                width: Some(1280),
                height: Some(820),
                maximized: Some(false),
            }),
        };
        let json = serde_json::to_string(&config).expect("serialize config");
        assert!(json.contains(r#""window":{"x":120,"y":80,"width":1280,"height":820,"maximized":false}"#));
        let restored: AppConfig = serde_json::from_str(&json).expect("deserialize config");
        assert_eq!(restored.window.and_then(|window| window.width), Some(1280));
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .manage(WindowPersistState::default())
        .setup(|app| {
            let state = app.state::<AppState>();
            let config = load_config();
            if let Some(root) = config.notes_root {
                if let Err(error) = state.configure_root(PathBuf::from(root), app.handle()) {
                    eprintln!("Could not restore notes root: {error}");
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }

            if !matches!(event, WindowEvent::CloseRequested { .. }) {
                return;
            }

            let app = window.app_handle();
            let persist_state = app.state::<WindowPersistState>();
            if let Some(webview_window) = app.get_webview_window("main") {
                if let Err(error) = persist_window_state(&webview_window, persist_state.inner()) {
                    eprintln!("Could not save window state: {error}");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_notes,
            set_notes_root,
            get_app_config,
            save_ui_scale,
            save_theme,
            restore_window_state,
            note_viewport_logical_size,
            search_notes,
            open_note,
            save_note,
            delete_note,
            rename_note,
            create_or_open_note,
            create_quick_note,
            render_markdown,
            process_image_paste,
            process_image_paste_base64,
            process_clipboard_image_paste,
            process_image_file_paste,
            copy_image_to_clipboard,
            resolve_note_asset
        ])
        .run(tauri::generate_context!())
        .expect("error while running Echo");
}

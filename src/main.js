import "./styles.css";

import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers } from "@codemirror/view";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";

const searchInput = document.querySelector("#search-input");
const chooseRootButton = document.querySelector("#choose-root");
const rootBanner = document.querySelector("#root-banner");
const resultsList = document.querySelector("#results-list");
const workspace = document.querySelector(".workspace");
const emptyState = document.querySelector("#empty-state");
const preview = document.querySelector("#preview");
const statusBar = document.querySelector("#status-bar");
const noteContextMenu = document.querySelector("#note-context-menu");
const renameNoteButton = document.querySelector("#rename-note");
const deleteNoteButton = document.querySelector("#delete-note");
const renameDialog = document.querySelector("#rename-dialog");
const renameForm = document.querySelector("#rename-form");
const renameInput = document.querySelector("#rename-input");
const cancelRenameButton = document.querySelector("#cancel-rename");
const imageOverlay = document.querySelector("#image-overlay");
const zoomedImage = document.querySelector("#zoomed-image");
const appInfoOverlay = document.querySelector("#app-info-overlay");
const imageContextMenu = document.querySelector("#image-context-menu");
const copyImageButton = document.querySelector("#copy-image");
const calendarTitle = document.querySelector("#calendar-title");
const calendarGrid = document.querySelector("#calendar-grid");
const calendarPrevButton = document.querySelector("#calendar-prev");
const calendarTodayButton = document.querySelector("#calendar-today");
const calendarNextButton = document.querySelector("#calendar-next");
const appWindow = getCurrentWindow();

let rowHeight = 42;
const minUiScale = -4;
const maxUiScale = 8;
const themes = ["dark", "light", "solarized", "hacker", "orange-hacker"];
const lineNumberCompartment = new Compartment();
const welcomeNoteContent = `# Welcome to puretype

puretype is a fast Markdown notebook for writing, searching, and daily notes.

## Quick Start

- Use Ctrl/Cmd+L to search notes or type a new note name.
- Press Enter in search to open the best match or create a note.
- Use Ctrl/Cmd+D or the Today button to open today's daily note.
- Use the calendar to jump to any daily note; days with daily notes are marked.
- Use Ctrl/Cmd+E to toggle Markdown preview.
- Use Ctrl/Cmd+T to switch themes: dark, light, solarized, hacker, and orange hacker.
- Use Ctrl/Cmd+F to toggle focus mode when you want only the editor.
- Use Ctrl/Cmd++ and Ctrl/Cmd+- to adjust the UI size.
- Paste images directly into an open note to attach them.
- Use Ctrl/Cmd+K anytime to see shortcuts.
`;
const resultSpacer = document.createElement("div");
const rowsLayer = document.createElement("div");
rowsLayer.style.position = "absolute";
rowsLayer.style.inset = "0";
resultsList.append(resultSpacer, rowsLayer);
const initialCalendarDate = new Date();

const appState = {
  root: null,
  notes: [],
  results: [],
  currentPath: null,
  loadingDocument: false,
  searchTimer: null,
  saveTimer: null,
  previewTimer: null,
  contextMenuPath: null,
  renamePath: null,
  viewMode: "edit",
  imageContextAssetPath: null,
  uiScale: 0,
  theme: "dark",
  focusMode: false,
  focusModeTransition: false,
  firstRun: false,
  welcomeCreated: false,
  calendarYear: initialCalendarDate.getFullYear(),
  calendarMonth: initialCalendarDate.getMonth(),
};

const editorTheme = EditorView.theme(
  {
    ".cm-content": {
      caretColor: "var(--accent)",
    },
    "&.cm-focused .cm-cursor": {
      borderLeftColor: "var(--accent)",
      borderLeftWidth: "2px",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
      backgroundColor: "color-mix(in srgb, var(--accent-strong) 35%, transparent)",
    },
    ".cm-activeLine": {
      backgroundColor: "color-mix(in srgb, var(--panel-soft) 55%, transparent)",
    },
  },
  { dark: true },
);

const editor = new EditorView({
  parent: document.querySelector("#editor"),
  state: EditorState.create({
    doc: "",
    extensions: [
      lineNumberCompartment.of(lineNumbers()),
      history(),
      markdown(),
      keymap.of([indentWithTab, ...defaultKeymap, ...historyKeymap]),
      EditorView.lineWrapping,
      editorTheme,
      EditorView.updateListener.of((update) => {
        if (!update.docChanged || appState.loadingDocument) {
          return;
        }
        scheduleSave();
        schedulePreview();
      }),
      EditorView.domEventHandlers({
        paste: (event, view) => {
          return handlePaste(event, view);
        },
      }),
    ],
  }),
});

function setStatus(message, isError = false) {
  statusBar.textContent = message;
  statusBar.classList.toggle("error", isError);
}

function debounce(key, delay, callback) {
  clearTimeout(appState[key]);
  appState[key] = setTimeout(callback, delay);
}

async function togglePreviewMode() {
  if (!appState.currentPath) {
    return;
  }

  appState.viewMode = appState.viewMode === "preview" ? "edit" : "preview";
  if (appState.viewMode === "preview") {
    await updatePreview();
  }
  updateWorkspaceState();
}

function updateWorkspaceState() {
  const hasSelectedNote = Boolean(appState.currentPath);
  workspace.classList.toggle("no-note-selected", !hasSelectedNote);
  workspace.classList.toggle("edit-mode", hasSelectedNote && appState.viewMode === "edit");
  workspace.classList.toggle("preview-mode", hasSelectedNote && appState.viewMode === "preview");
  emptyState.hidden = hasSelectedNote;
}

function setEditorContent(content) {
  appState.loadingDocument = true;
  editor.dispatch({
    changes: {
      from: 0,
      to: editor.state.doc.length,
      insert: content,
    },
  });
  appState.loadingDocument = false;
}

function clearCurrentNote() {
  appState.currentPath = null;
  appState.viewMode = "edit";
  setEditorContent("");
  preview.innerHTML = "";
  updateWorkspaceState();
}

async function setFocusMode(enabled) {
  if (appState.focusMode === enabled || appState.focusModeTransition) {
    return;
  }

  const previousFocusMode = appState.focusMode;
  const previousViewMode = appState.viewMode;
  appState.focusModeTransition = true;
  appState.focusMode = enabled;
  if (enabled) {
    hideAppInfoOverlay();
    hideImageOverlay();
    hideAllContextMenus();
    appState.viewMode = "edit";
  }

  try {
    await appWindow.setFullscreen(enabled);
    document.documentElement.classList.toggle("focus-mode", enabled);
    editor.dispatch({
      effects: lineNumberCompartment.reconfigure(enabled ? [] : lineNumbers()),
    });
    updateWorkspaceState();
  } catch (error) {
    appState.focusMode = previousFocusMode;
    appState.viewMode = previousViewMode;
    document.documentElement.classList.toggle("focus-mode", previousFocusMode);
    editor.dispatch({
      effects: lineNumberCompartment.reconfigure(previousFocusMode ? [] : lineNumbers()),
    });
    updateWorkspaceState();
    setStatus(String(error), true);
  } finally {
    appState.focusModeTransition = false;
  }

  if (appState.currentPath) {
    editor.focus();
  }
}

async function toggleFocusMode() {
  await setFocusMode(!appState.focusMode);
}

function formatDateKey(date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function dailyNotePathForDate(date, includeExtension = false) {
  const path = `daily/${formatDateKey(date)}`;
  return includeExtension ? `${path}.md` : path;
}

function todayDailyNotePath() {
  return dailyNotePathForDate(new Date());
}

function parseDailyNoteDate(path) {
  const match = path?.match(/^daily\/(\d{4})-(\d{2})-(\d{2})\.md$/i);
  if (!match) {
    return null;
  }
  const year = Number(match[1]);
  const month = Number(match[2]) - 1;
  const day = Number(match[3]);
  const date = new Date(year, month, day);
  if (
    date.getFullYear() !== year ||
    date.getMonth() !== month ||
    date.getDate() !== day
  ) {
    return null;
  }
  return date;
}

function syncCalendarToCurrentNote() {
  const date = parseDailyNoteDate(appState.currentPath);
  if (!date) {
    renderCalendar();
    return;
  }
  appState.calendarYear = date.getFullYear();
  appState.calendarMonth = date.getMonth();
  renderCalendar();
}

function sameDay(left, right) {
  return (
    left.getFullYear() === right.getFullYear() &&
    left.getMonth() === right.getMonth() &&
    left.getDate() === right.getDate()
  );
}

function isoWeekNumber(date) {
  const utcDate = new Date(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()));
  const dayNumber = utcDate.getUTCDay() || 7;
  utcDate.setUTCDate(utcDate.getUTCDate() + 4 - dayNumber);
  const yearStart = new Date(Date.UTC(utcDate.getUTCFullYear(), 0, 1));
  return Math.ceil(((utcDate - yearStart) / 86400000 + 1) / 7);
}

function hasDailyNote(date) {
  const path = dailyNotePathForDate(date, true).toLowerCase();
  return appState.notes.some((note) => note.path.toLowerCase() === path);
}

function renderCalendar() {
  const monthStart = new Date(appState.calendarYear, appState.calendarMonth, 1);
  const today = new Date();
  const selectedDate = parseDailyNoteDate(appState.currentPath);
  const mondayOffset = (monthStart.getDay() + 6) % 7;
  const gridStart = new Date(monthStart);
  gridStart.setDate(monthStart.getDate() - mondayOffset);
  const monthName = monthStart.toLocaleDateString(undefined, {
    month: "long",
    year: "numeric",
  });

  calendarTitle.textContent = monthName;
  calendarGrid.replaceChildren();

  const emptyHeader = document.createElement("div");
  emptyHeader.className = "calendar-weekday";
  emptyHeader.setAttribute("aria-hidden", "true");
  calendarGrid.append(emptyHeader);

  for (const weekday of ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]) {
    const weekdayCell = document.createElement("div");
    weekdayCell.className = "calendar-weekday";
    weekdayCell.textContent = weekday;
    calendarGrid.append(weekdayCell);
  }

  const cursor = new Date(gridStart);
  do {
    const weekNumber = document.createElement("div");
    weekNumber.className = "calendar-week-number";
    weekNumber.textContent = String(isoWeekNumber(cursor));
    calendarGrid.append(weekNumber);

    for (let index = 0; index < 7; index += 1) {
      const date = new Date(cursor);
      const button = document.createElement("button");
      button.type = "button";
      button.className = "calendar-day";
      button.textContent = String(date.getDate());
      button.setAttribute(
        "aria-label",
        date.toLocaleDateString(undefined, {
          weekday: "long",
          year: "numeric",
          month: "long",
          day: "numeric",
        }),
      );
      button.classList.toggle("outside-month", date.getMonth() !== appState.calendarMonth);
      button.classList.toggle("weekend", date.getDay() === 0 || date.getDay() === 6);
      button.classList.toggle("today", sameDay(date, today));
      button.classList.toggle("has-note", hasDailyNote(date));
      button.classList.toggle("selected", Boolean(selectedDate && sameDay(date, selectedDate)));
      button.classList.toggle("editing", Boolean(selectedDate && sameDay(date, selectedDate)));
      button.addEventListener("click", () => {
        void openDailyNoteForDate(date);
      });
      calendarGrid.append(button);
      cursor.setDate(cursor.getDate() + 1);
    }
  } while (
    cursor.getMonth() === appState.calendarMonth ||
    cursor.getDay() !== 1
  );
}

async function openDailyNoteForDate(date) {
  const dailyPath = dailyNotePathForDate(date);
  searchInput.value = dailyPath;

  try {
    const result = await createOrOpenNote(dailyPath);
    await runSearch(dailyPath);
    scrollCurrentResultIntoView();
    setStatus(`${result.created ? "Created" : "Opened"} ${appState.currentPath}`);
  } catch (error) {
    setStatus(String(error), true);
  }
}

function navigateCalendarMonth(delta) {
  const nextMonth = new Date(appState.calendarYear, appState.calendarMonth + delta, 1);
  appState.calendarYear = nextMonth.getFullYear();
  appState.calendarMonth = nextMonth.getMonth();
  renderCalendar();
}

function setCalendarToToday() {
  const today = new Date();
  appState.calendarYear = today.getFullYear();
  appState.calendarMonth = today.getMonth();
  renderCalendar();
}

async function openTodayDailyNote() {
  const dailyPath = todayDailyNotePath();
  searchInput.value = dailyPath;

  try {
    const result = await createOrOpenNote(dailyPath);
    await runSearch(dailyPath);
    scrollCurrentResultIntoView();
    setStatus(`${result.created ? "Created" : "Opened"} ${appState.currentPath}`);
  } catch (error) {
    setStatus(String(error), true);
  }
}

function hideContextMenu() {
  noteContextMenu.classList.add("hidden");
  appState.contextMenuPath = null;
}

function hideImageContextMenu() {
  imageContextMenu.classList.add("hidden");
  appState.imageContextAssetPath = null;
}

function hideAllContextMenus() {
  hideContextMenu();
  hideImageContextMenu();
}

function showContextMenu(event, note) {
  event.preventDefault();
  hideImageContextMenu();
  appState.contextMenuPath = note.path;
  noteContextMenu.style.left = `${event.clientX}px`;
  noteContextMenu.style.top = `${event.clientY}px`;
  noteContextMenu.classList.remove("hidden");
}

async function loadSnapshot() {
  try {
    const snapshot = await invoke("get_notes");
    appState.root = snapshot.root;
    appState.notes = snapshot.notes;
    rootBanner.classList.toggle("hidden", Boolean(appState.root));
    setStatus(
      appState.root
        ? `${appState.notes.length} notes in ${appState.root}`
        : "No notes root selected",
    );
    await runSearch(searchInput.value);
    renderCalendar();
  } catch (error) {
    setStatus(String(error), true);
  }
}

async function chooseRoot() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "Choose puretype Notes Folder",
  });

  if (!selected) {
    return;
  }

  try {
    const snapshot = await invoke("set_notes_root", { path: selected });
    appState.root = snapshot.root;
    appState.notes = snapshot.notes;
    rootBanner.classList.add("hidden");
    setStatus(`${appState.notes.length} notes in ${appState.root}`);

    if (appState.firstRun && !appState.welcomeCreated) {
      await openWelcomeNote();
      return;
    }

    await runSearch(searchInput.value);
    searchInput.focus();
  } catch (error) {
    setStatus(String(error), true);
  }
}

async function openWelcomeNote() {
  const result = await createOrOpenNote("Welcome");
  if (result.created) {
    await invoke("save_note", {
      path: appState.currentPath,
      content: welcomeNoteContent,
    });
    setEditorContent(welcomeNoteContent);
    await loadSnapshot();
  }
  appState.firstRun = false;
  appState.welcomeCreated = true;
  searchInput.value = "Welcome";
  await runSearch(searchInput.value);
  scrollCurrentResultIntoView();
  editor.focus();
  setStatus(`${result.created ? "Created" : "Opened"} ${appState.currentPath}`);
}

async function runSearch(query) {
  if (!appState.root) {
    appState.results = [];
    renderResults();
    return;
  }

  try {
    appState.results = await invoke("search_notes", { query });
    renderResults();
  } catch (error) {
    setStatus(String(error), true);
  }
}

function renderResults() {
  resultSpacer.style.height = `${appState.results.length * rowHeight}px`;
  renderVisibleRows();
}

function scrollCurrentResultIntoView() {
  const selectedIndex = appState.results.findIndex((note) => note.path === appState.currentPath);
  if (selectedIndex === -1) {
    return;
  }

  const rowTop = selectedIndex * rowHeight;
  const rowBottom = rowTop + rowHeight;
  const viewportTop = resultsList.scrollTop;
  const viewportBottom = viewportTop + resultsList.clientHeight;

  if (rowTop < viewportTop || rowBottom > viewportBottom) {
    resultsList.scrollTop = Math.max(0, rowTop - rowHeight);
    renderVisibleRows();
  }
}

function renderVisibleRows() {
  const scrollTop = resultsList.scrollTop;
  const viewportHeight = resultsList.clientHeight;
  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - 4);
  const end = Math.min(
    appState.results.length,
    Math.ceil((scrollTop + viewportHeight) / rowHeight) + 4,
  );

  rowsLayer.replaceChildren();
  for (let index = start; index < end; index += 1) {
    const note = appState.results[index];
    const row = document.createElement("button");
    row.type = "button";
    row.className = "result-row";
    row.style.top = `${index * rowHeight}px`;
    row.classList.toggle("selected", note.path === appState.currentPath);
    row.innerHTML = `
      <span class="result-title"></span>
    `;
    row.querySelector(".result-title").textContent = note.title || filenameFromPath(note.path);
    row.addEventListener("click", () => {
      hideAllContextMenus();
      void openNote(note.path);
    });
    row.addEventListener("contextmenu", (event) => showContextMenu(event, note));
    rowsLayer.append(row);
  }
}

async function openNote(path) {
  try {
    const opened = await invoke("open_note", { path });
    appState.currentPath = opened.note.path;
    setEditorContent(opened.content);
    searchInput.value = opened.note.path.replace(/\.md$/i, "");
    renderResults();
    syncCalendarToCurrentNote();
    if (appState.viewMode === "preview") {
      await updatePreview();
    }
    updateWorkspaceState();
    if (appState.viewMode === "edit") {
      editor.focus();
    }
    setStatus(`Opened ${opened.note.path}`);
  } catch (error) {
    appState.loadingDocument = false;
    setStatus(String(error), true);
  }
}

async function createOrOpenFromSearch() {
  const query = searchInput.value.trim();
  if (!query) {
    return;
  }

  try {
    const result = await createOrOpenNote(query);
    setStatus(`${result.created ? "Created" : "Opened"} ${appState.currentPath}`);
  } catch (error) {
    setStatus(String(error), true);
  }
}

async function createOrOpenNote(query) {
  try {
    const result = await invoke("create_or_open_note", { query });
    appState.currentPath = result.note.note.path;
    setEditorContent(result.note.content);
    if (result.created) {
      appState.viewMode = "edit";
    }
    await loadSnapshot();
    if (appState.viewMode === "preview") {
      await updatePreview();
    }
    updateWorkspaceState();
    syncCalendarToCurrentNote();
    editor.focus();
    return result;
  } catch (error) {
    appState.loadingDocument = false;
    throw error;
  }
}

async function deleteSelectedContextNote() {
  const path = appState.contextMenuPath;
  hideAllContextMenus();
  if (!path) {
    return;
  }

  if (!window.confirm(`Delete ${path}? This cannot be undone.`)) {
    return;
  }

  try {
    const snapshot = await invoke("delete_note", { path });
    appState.root = snapshot.root;
    appState.notes = snapshot.notes;
    appState.results = snapshot.notes;

    if (appState.currentPath === path) {
      searchInput.value = "";
      clearCurrentNote();
    }

    await runSearch(searchInput.value);
    setStatus(`Deleted ${path}`);
  } catch (error) {
    appState.loadingDocument = false;
    setStatus(String(error), true);
  }
}

function filenameFromPath(path) {
  return path.split("/").pop()?.replace(/\.[^.]+$/u, "") || path;
}

function showRenameDialog() {
  const path = appState.contextMenuPath;
  hideAllContextMenus();
  if (!path) {
    return;
  }

  appState.renamePath = path;
  renameInput.value = filenameFromPath(path);
  renameDialog.showModal();
  renameInput.focus();
  renameInput.select();
}

function closeRenameDialog() {
  appState.renamePath = null;
  renameDialog.close();
}

async function renameSelectedNote() {
  const path = appState.renamePath;
  const newFilename = renameInput.value.trim();
  if (!path || !newFilename) {
    return;
  }

  try {
    const result = await invoke("rename_note", {
      path,
      newFilename,
    });
    appState.root = result.snapshot.root;
    appState.notes = result.snapshot.notes;
    if (appState.currentPath === result.oldPath) {
      appState.currentPath = result.newPath;
      searchInput.value = result.newPath.replace(/\.[^.]+$/u, "");
    }
    closeRenameDialog();
    await runSearch(searchInput.value);
    scrollCurrentResultIntoView();
    setStatus(`Renamed ${result.oldPath} to ${result.newPath}`);
  } catch (error) {
    setStatus(String(error), true);
  }
}

function scheduleSave() {
  if (!appState.currentPath) {
    return;
  }
  debounce("saveTimer", 250, async () => {
    try {
      await invoke("save_note", {
        path: appState.currentPath,
        content: editor.state.doc.toString(),
      });
      setStatus(`Saved ${appState.currentPath}`);
    } catch (error) {
      setStatus(String(error), true);
    }
  });
}

function schedulePreview() {
  if (appState.viewMode !== "preview") {
    return;
  }
  debounce("previewTimer", 75, () => {
    void updatePreview();
  });
}

async function updatePreview() {
  const html = await invoke("render_markdown", {
    content: editor.state.doc.toString(),
  });
  preview.innerHTML = html;
  await resolvePreviewImages();
  wirePreviewImages();
}

async function resolvePreviewImages() {
  if (!appState.currentPath) {
    return;
  }

  const images = [...preview.querySelectorAll("img")];
  await Promise.all(
    images.map(async (image) => {
      const source = image.getAttribute("src");
      if (!source || /^(https?:|data:|asset:|file:)/i.test(source)) {
        return;
      }
      try {
        const absolutePath = await invoke("resolve_note_asset", {
          notePath: appState.currentPath,
          assetPath: source,
        });
        image.dataset.assetPath = source;
        image.dataset.absolutePath = absolutePath;
        image.src = convertFileSrc(absolutePath);
      } catch (error) {
        image.alt = `${image.alt || "Image"} (${error})`;
      }
    }),
  );
}

function wirePreviewImages() {
  for (const image of preview.querySelectorAll("img")) {
    image.addEventListener("click", () => {
      showImageOverlay(image.src, image.dataset.assetPath || "");
    });
    image.addEventListener("contextmenu", (event) => {
      const assetPath = image.dataset.assetPath;
      if (!assetPath) {
        return;
      }
      event.preventDefault();
      showImageContextMenu(event, assetPath);
    });
  }
}

function showImageOverlay(source, assetPath) {
  if (!source) {
    return;
  }
  zoomedImage.src = source;
  zoomedImage.dataset.assetPath = assetPath;
  imageOverlay.classList.remove("hidden");
  imageOverlay.setAttribute("aria-hidden", "false");
}

function hideImageOverlay() {
  imageOverlay.classList.add("hidden");
  imageOverlay.setAttribute("aria-hidden", "true");
  zoomedImage.removeAttribute("src");
  zoomedImage.dataset.assetPath = "";
  hideImageContextMenu();
}

function showAppInfoOverlay() {
  appInfoOverlay.classList.remove("hidden");
  appInfoOverlay.setAttribute("aria-hidden", "false");
  hideAllContextMenus();
}

function hideAppInfoOverlay() {
  appInfoOverlay.classList.add("hidden");
  appInfoOverlay.setAttribute("aria-hidden", "true");
}

function showImageContextMenu(event, assetPath) {
  hideContextMenu();
  appState.imageContextAssetPath = assetPath;
  imageContextMenu.style.left = `${event.clientX}px`;
  imageContextMenu.style.top = `${event.clientY}px`;
  imageContextMenu.classList.remove("hidden");
}

async function copyContextImage() {
  const assetPath = appState.imageContextAssetPath;
  hideImageContextMenu();
  if (!assetPath || !appState.currentPath) {
    return;
  }

  try {
    await invoke("copy_image_to_clipboard", {
      notePath: appState.currentPath,
      assetPath,
    });
    setStatus("Copied image to clipboard");
  } catch (error) {
    setStatus(String(error), true);
  }
}

function handlePaste(event, view) {
  const imageSource = findPastedImage(event.clipboardData);
  const mightBeImagePaste = clipboardMightContainImage(event.clipboardData);
  if (!imageSource && !mightBeImagePaste) {
    return false;
  }

  event.preventDefault();
  if (!appState.currentPath) {
    setStatus("Open or create a note before pasting an image.", true);
    return true;
  }

  void processPastedImage(imageSource, view);
  return true;
}

function clipboardMightContainImage(clipboardData) {
  if (!clipboardData) {
    return true;
  }

  const types = [...clipboardData.types];
  if (types.length === 0) {
    return true;
  }
  if (types.some((type) => type.startsWith("image/"))) {
    return true;
  }
  if (types.some((type) => type === "text/uri-list" || type === "x-special/gnome-copied-files")) {
    return true;
  }

  const html = clipboardData.getData("text/html");
  if (/<img\b/i.test(html)) {
    return true;
  }

  // Screenshot tools can expose image data only to the native clipboard API.
  return !types.includes("text/plain") && !types.includes("text/html");
}

function findPastedImage(clipboardData) {
  if (!clipboardData) {
    return null;
  }

  for (const clipboardItem of clipboardData.items) {
    if (!clipboardItem.type.startsWith("image/")) {
      continue;
    }

    const file = clipboardItem.getAsFile();
    if (file) {
      return file;
    }
  }

  const file = [...clipboardData.files].find((pastedFile) =>
    pastedFile.type.startsWith("image/"),
  );
  if (file) {
    return file;
  }

  return (
    imageFromHtml(clipboardData.getData("text/html")) ??
    imageFileFromClipboardText(clipboardData.getData("text/uri-list")) ??
    imageFileFromClipboardText(clipboardData.getData("x-special/gnome-copied-files")) ??
    imageFileFromClipboardText(clipboardData.getData("text/plain"))
  );
}

function imageFromHtml(html) {
  if (!html) {
    return null;
  }

  const doc = new DOMParser().parseFromString(html, "text/html");
  const source = doc.querySelector("img")?.getAttribute("src");
  if (!source?.startsWith("data:image/")) {
    return null;
  }

  const match = source.match(/^data:(image\/[^;]+);base64,(.+)$/);
  if (!match) {
    return null;
  }

  return {
    base64: match[2],
    type: match[1],
  };
}

function imageFileFromClipboardText(text) {
  if (!text) {
    return null;
  }

  const uri = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line.startsWith("file://"));
  if (!uri) {
    return null;
  }

  try {
    const path = decodeURIComponent(new URL(uri).pathname);
    if (!/\.(png|jpe?g|gif|webp)$/i.test(path)) {
      return null;
    }
    return { path };
  } catch {
    return null;
  }
}

async function fileToBase64(file) {
  const dataUrl = await new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () => resolve(reader.result));
    reader.addEventListener("error", () => reject(reader.error));
    reader.readAsDataURL(file);
  });

  const base64 = String(dataUrl).split(",", 2)[1];
  if (!base64) {
    throw new Error("Could not read pasted image data.");
  }
  return base64;
}

async function processPastedImage(imageSource, view) {
  try {
    const markdownLink = imageSource
      ? await savePastedImageSource(imageSource)
      : await invoke("process_clipboard_image_paste", {
          notePath: appState.currentPath,
        });
    view.dispatch(view.state.replaceSelection(markdownLink));
    setStatus(`Attached image to ${appState.currentPath}`);
  } catch (error) {
    setStatus(String(error), true);
  }
}

async function savePastedImageSource(imageSource) {
  if ("path" in imageSource) {
    return invoke("process_image_file_paste", {
      notePath: appState.currentPath,
      imagePath: imageSource.path,
    });
  }

  const imageBase64 =
    "base64" in imageSource ? imageSource.base64 : await fileToBase64(imageSource);
  const imageType = imageSource.type || "image/png";
  return invoke("process_image_paste_base64", {
    notePath: appState.currentPath,
    imageBase64,
    extension: imageType.split("/")[1] || "png",
  });
}

function applyUiScale(uiScale) {
  appState.uiScale = Math.min(maxUiScale, Math.max(minUiScale, uiScale));
  rowHeight = Math.max(32, 42 + appState.uiScale);
  document.documentElement.style.setProperty("--app-font-size", `${15 + appState.uiScale}px`);
  document.documentElement.style.setProperty("--editor-font-size", `${14.7 + appState.uiScale}px`);
  document.documentElement.style.setProperty("--result-row-height", `${rowHeight}px`);
  renderResults();
}

function normalizeTheme(theme) {
  return themes.includes(theme) ? theme : "dark";
}

function applyTheme(theme) {
  appState.theme = normalizeTheme(theme);
  document.documentElement.dataset.theme = appState.theme;
}

async function loadAppConfig() {
  try {
    const config = await invoke("get_app_config");
    applyUiScale(config.uiScale ?? 0);
    applyTheme(config.theme ?? "dark");
    appState.firstRun = Boolean(config.firstRun);
  } catch (error) {
    applyTheme("dark");
    setStatus(String(error), true);
  }
}

async function cycleTheme() {
  const currentIndex = themes.indexOf(appState.theme);
  const nextTheme = themes[(currentIndex + 1) % themes.length] ?? "dark";
  applyTheme(nextTheme);
  try {
    await invoke("save_theme", { theme: nextTheme });
    setStatus(`Theme: ${nextTheme}`);
  } catch (error) {
    setStatus(String(error), true);
  }
}

async function changeUiScale(delta) {
  const nextScale = Math.min(maxUiScale, Math.max(minUiScale, appState.uiScale + delta));
  if (nextScale === appState.uiScale) {
    return;
  }

  applyUiScale(nextScale);
  try {
    await invoke("save_ui_scale", { uiScale: nextScale });
    setStatus(`UI size ${nextScale >= 0 ? "+" : ""}${nextScale}`);
  } catch (error) {
    setStatus(String(error), true);
  }
}

searchInput.addEventListener("input", () => {
  debounce("searchTimer", 15, () => {
    void runSearch(searchInput.value);
  });
});

searchInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    void createOrOpenFromSearch();
  }
});

window.addEventListener("keydown", (event) => {
  const isCommandShortcut = event.metaKey || event.ctrlKey;
  const key = event.key.toLowerCase();
  const isFocusToggleShortcut = isCommandShortcut && !event.altKey && key === "f";
  const appShortcutKeys = new Set(["l", "k", "e", "t", "d", "+", "=", "-", "f"]);

  if (appState.focusMode) {
    if (isFocusToggleShortcut) {
      event.preventDefault();
      void toggleFocusMode();
    } else if (isCommandShortcut && appShortcutKeys.has(key)) {
      event.preventDefault();
    }
    return;
  }

  if (isCommandShortcut && key === "l") {
    event.preventDefault();
    searchInput.focus();
    searchInput.select();
  } else if (isCommandShortcut && key === "k") {
    event.preventDefault();
    showAppInfoOverlay();
  } else if (isCommandShortcut && key === "e") {
    event.preventDefault();
    void togglePreviewMode();
  } else if (isCommandShortcut && key === "t") {
    event.preventDefault();
    void cycleTheme();
  } else if (isCommandShortcut && key === "d") {
    event.preventDefault();
    void openTodayDailyNote();
  } else if (isFocusToggleShortcut) {
    event.preventDefault();
    void toggleFocusMode();
  } else if (isCommandShortcut && (event.key === "+" || event.key === "=")) {
    event.preventDefault();
    void changeUiScale(1);
  } else if (isCommandShortcut && event.key === "-") {
    event.preventDefault();
    void changeUiScale(-1);
  } else if (event.key === "Escape") {
    hideAppInfoOverlay();
    hideImageOverlay();
    hideAllContextMenus();
  }
});

chooseRootButton.addEventListener("click", () => {
  void chooseRoot();
});

calendarPrevButton.addEventListener("click", () => {
  navigateCalendarMonth(-1);
});
calendarTodayButton.addEventListener("click", () => {
  void openTodayDailyNote();
});
calendarNextButton.addEventListener("click", () => {
  navigateCalendarMonth(1);
});
resultsList.addEventListener("scroll", renderVisibleRows, { passive: true });
resultsList.addEventListener("scroll", hideAllContextMenus, { passive: true });
deleteNoteButton.addEventListener("click", () => {
  void deleteSelectedContextNote();
});
renameNoteButton.addEventListener("click", showRenameDialog);
renameForm.addEventListener("submit", (event) => {
  event.preventDefault();
  void renameSelectedNote();
});
cancelRenameButton.addEventListener("click", closeRenameDialog);
renameDialog.addEventListener("close", () => {
  appState.renamePath = null;
});
copyImageButton.addEventListener("click", () => {
  void copyContextImage();
});
imageOverlay.addEventListener("click", (event) => {
  if (event.button === 0) {
    hideImageOverlay();
  }
});
appInfoOverlay.addEventListener("click", (event) => {
  if (event.button === 0) {
    hideAppInfoOverlay();
  }
});
zoomedImage.addEventListener("contextmenu", (event) => {
  const assetPath = zoomedImage.dataset.assetPath;
  if (!assetPath) {
    return;
  }
  event.preventDefault();
  event.stopPropagation();
  showImageContextMenu(event, assetPath);
});
window.addEventListener("click", (event) => {
  if (!noteContextMenu.contains(event.target) && !imageContextMenu.contains(event.target)) {
    hideAllContextMenus();
  }
});
window.addEventListener("blur", hideAllContextMenus);
await listen("notes-changed", () => {
  void loadSnapshot();
});

await loadAppConfig();
await loadSnapshot();
updateWorkspaceState();
searchInput.focus();

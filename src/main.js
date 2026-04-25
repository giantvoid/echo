import "./styles.css";

import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers } from "@codemirror/view";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

const searchInput = document.querySelector("#search-input");
const previewToggle = document.querySelector("#preview-toggle");
const chooseRootButton = document.querySelector("#choose-root");
const rootBanner = document.querySelector("#root-banner");
const resultsList = document.querySelector("#results-list");
const workspace = document.querySelector(".workspace");
const preview = document.querySelector("#preview");
const statusBar = document.querySelector("#status-bar");
const noteContextMenu = document.querySelector("#note-context-menu");
const deleteNoteButton = document.querySelector("#delete-note");

const rowHeight = 64;
const resultSpacer = document.createElement("div");
const rowsLayer = document.createElement("div");
rowsLayer.style.position = "absolute";
rowsLayer.style.inset = "0";
resultsList.append(resultSpacer, rowsLayer);

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
      lineNumbers(),
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

function togglePreview() {
  const hidden = workspace.classList.toggle("preview-hidden");
  previewToggle.setAttribute("aria-pressed", String(!hidden));
}

function todayDailyNotePath() {
  const today = new Date();
  const year = today.getFullYear();
  const month = String(today.getMonth() + 1).padStart(2, "0");
  const day = String(today.getDate()).padStart(2, "0");
  return `daily/${year}-${month}-${day}`;
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

function showContextMenu(event, note) {
  event.preventDefault();
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
  } catch (error) {
    setStatus(String(error), true);
  }
}

async function chooseRoot() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "Choose PureType Notes Folder",
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
    await runSearch(searchInput.value);
    searchInput.focus();
  } catch (error) {
    setStatus(String(error), true);
  }
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
      <span class="result-snippet"></span>
    `;
    row.querySelector(".result-title").textContent = note.path;
    row.querySelector(".result-snippet").textContent = note.snippet || "Empty note";
    row.addEventListener("click", () => {
      hideContextMenu();
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
    appState.loadingDocument = true;
    editor.dispatch({
      changes: {
        from: 0,
        to: editor.state.doc.length,
        insert: opened.content,
      },
    });
    appState.loadingDocument = false;
    searchInput.value = opened.note.path.replace(/\.md$/i, "");
    renderResults();
    await updatePreview();
    editor.focus();
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
    appState.loadingDocument = true;
    editor.dispatch({
      changes: {
        from: 0,
        to: editor.state.doc.length,
        insert: result.note.content,
      },
    });
    appState.loadingDocument = false;
    await loadSnapshot();
    await updatePreview();
    editor.focus();
    return result;
  } catch (error) {
    appState.loadingDocument = false;
    throw error;
  }
}

async function deleteSelectedContextNote() {
  const path = appState.contextMenuPath;
  hideContextMenu();
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
      appState.currentPath = null;
      appState.loadingDocument = true;
      editor.dispatch({
        changes: {
          from: 0,
          to: editor.state.doc.length,
          insert: "",
        },
      });
      appState.loadingDocument = false;
      preview.innerHTML = "";
    }

    await runSearch(searchInput.value);
    setStatus(`Deleted ${path}`);
  } catch (error) {
    appState.loadingDocument = false;
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
        image.src = convertFileSrc(absolutePath);
      } catch (error) {
        image.alt = `${image.alt || "Image"} (${error})`;
      }
    }),
  );
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

  if (isCommandShortcut && key === "l") {
    event.preventDefault();
    searchInput.focus();
    searchInput.select();
  } else if (isCommandShortcut && key === "p") {
    event.preventDefault();
    togglePreview();
  } else if (isCommandShortcut && key === "d") {
    event.preventDefault();
    void openTodayDailyNote();
  } else if (event.key === "Escape") {
    hideContextMenu();
  }
});

previewToggle.addEventListener("click", () => {
  togglePreview();
});

chooseRootButton.addEventListener("click", () => {
  void chooseRoot();
});

resultsList.addEventListener("scroll", renderVisibleRows, { passive: true });
resultsList.addEventListener("scroll", hideContextMenu, { passive: true });
deleteNoteButton.addEventListener("click", () => {
  void deleteSelectedContextNote();
});
window.addEventListener("click", (event) => {
  if (!noteContextMenu.contains(event.target)) {
    hideContextMenu();
  }
});
window.addEventListener("blur", hideContextMenu);

await listen("notes-changed", () => {
  void loadSnapshot();
});

await loadSnapshot();
searchInput.focus();

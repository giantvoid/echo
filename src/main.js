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
};

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
      EditorView.updateListener.of((update) => {
        if (!update.docChanged || appState.loadingDocument) {
          return;
        }
        scheduleSave();
        schedulePreview();
      }),
      EditorView.domEventHandlers({
        paste: (event, view) => {
          void handlePaste(event, view);
          return false;
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
    row.addEventListener("click", () => openNote(note.path));
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
    setStatus(`${result.created ? "Created" : "Opened"} ${appState.currentPath}`);
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

async function handlePaste(event, view) {
  if (!appState.currentPath || !event.clipboardData) {
    return;
  }

  const imageItem = [...event.clipboardData.items].find((item) =>
    item.type.startsWith("image/"),
  );
  if (!imageItem) {
    return;
  }

  event.preventDefault();
  const file = imageItem.getAsFile();
  if (!file) {
    return;
  }

  try {
    const bytes = [...new Uint8Array(await file.arrayBuffer())];
    const markdownLink = await invoke("process_image_paste", {
      notePath: appState.currentPath,
      imageBytes: bytes,
      extension: file.type.split("/")[1] || "png",
    });
    view.dispatch(view.state.replaceSelection(markdownLink));
    setStatus(`Attached image to ${appState.currentPath}`);
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
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "l") {
    event.preventDefault();
    searchInput.focus();
    searchInput.select();
  }
});

previewToggle.addEventListener("click", () => {
  const hidden = workspace.classList.toggle("preview-hidden");
  previewToggle.setAttribute("aria-pressed", String(!hidden));
});

chooseRootButton.addEventListener("click", () => {
  void chooseRoot();
});

resultsList.addEventListener("scroll", renderVisibleRows, { passive: true });

await listen("notes-changed", () => {
  void loadSnapshot();
});

await loadSnapshot();
searchInput.focus();

Project Specification: "puretype" – High-Performance nvALT Clone

1. Core Vision

Build a "blazingly fast" desktop Markdown note-taking app inspired by nvALT. The app prioritizes instantaneous search, a flat-file-first philosophy (with subfolder support), and a seamless Rust-backed file system interface.

2. Tech Stack

Framework: Tauri v2 (Rust backend, Webview frontend).

Frontend: HTML5, CSS3 (Modern Grid/Flexbox), standard JavaScript.

Editor: CodeMirror 6 (highly modular and performant).

Markdown: pulldown-cmark (Rust).

Search Engine: Rust-based fuzzy search using the nucleo or skim crates for sub-millisecond indexing.

3. UI Layout (Single Window)

Top Bar: A persistent, global search input field. Focuses on launch (Cmd/Ctrl + L to refocus).

Main Area (Three-Pane Split):

Left Pane (Search Results): A vertical, virtualized list of notes. Displays filename and a small content snippet.

Center Pane (Editor): The CodeMirror 6 editor instance.

Right Pane (Preview): A vertical split Markdown preview (toggleable).

Behavior: The layout should be responsive and use CSS Grid for high-performance rendering.

4. Functional Requirements

A. Search & Creation Logic

Search-to-Create: If a user types a query in the search bar and presses Enter:

If a match exists, select and open it.

If no match exists, create a new file.

Subfolder Support: If the query is Daily/2026-04-25, the app must:

Ensure the directory [root]/Daily/ exists (create if missing).

Create 2026-04-25.md inside that directory.

B. File System & Indexing

Root Directory: User selects a local folder on first launch (stored in app config).

Watcher: Use the Rust notify crate to watch the folder for external changes and update the search index in real-time.

In-Memory Index: All file paths and metadata should be held in a Rust BTreeMap or similar for instant filtering.

C. Image Handling (The "Attachments" Pipeline)

Paste Event: Intercept paste events in the editor.

Logic:

Save image to a subfolder named attachments/ located relative to the current note.

Filename Format: [timestamp]_[unique_hash].png.

Markdown Insertion: Automatically insert the relative link: ![](attachments/file_name.png).

Rendering: Use Tauri's convertFileSrc to allow the Webview to display these local images.

5. Development Instructions

Phase 1: Foundation

Initialize a Tauri v2 project with a Rust backend.

Set up the app_handle to manage a "Notes Root" directory path.

Implement a Rust command get_notes that recursively scans the directory (walkdir) and returns a JSON list of file metadata.

Phase 2: The Search Engine

Implement a search_notes Tauri command in Rust using the nucleo crate for fuzzy matching.

The search should be triggered on every keystroke in the frontend (debounce to 10ms-20ms).

Phase 3: The Editor & Preview

Integrate CodeMirror 6 into the frontend.

Implement a "Save-on-Type" system (auto-save to disk via Rust backend).

Set up the vertical split for Markdown preview. Use a high-performance parser to ensure no lag during typing.

Phase 4: Image & Folder Logic

Implement the Notes Rust command that handles recursive folder creation (fs::create_dir_all).

Implement the process_image_paste command:

Receive bytes -> Determine note location -> Create attachments/ folder -> Write file -> Return relative path.

6. Performance Constraints

UI must remain responsive at 60fps+ during typing.

Initial index of 1,000 notes must take < 500ms.

Search filtering across 1,000 notes must be < 16ms (within one frame).
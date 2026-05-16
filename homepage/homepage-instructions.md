Create a modern, minimal, and beautiful single-page landing website for Echo — a blazingly fast, distraction-free note-taking app.

**Project name**: Echo
**Domain**: echoedit.app
**Tagline**: A blazingly fast, minimal, distraction-free note-taking app.

**Style requirements**:

- Very clean and minimalist design (white space, elegant typography)
- Dark mode by default with a nice accent color (soft teal or soft orange)
- Modern, calm, and professional feel — no clutter
- Fully responsive (mobile friendly)

**Structure the page with these sections**:

1. Hero Section
  - Big headline: “Think clearly. Write freely.”
  - Subheadline: “A blazingly fast, minimal note-taking app that gets out of your way.”
  - Prominent “Download for Linux / Windows / macOS” buttons. The buttons should point to the latest release assets on GitHub. Project repository: https://github.com/giantvoid/echo
  - Use the main screenshot (screenshots/echo.png) on the right side

2. Features Section
  - Show 4–5 key features with icons or short descriptions:
    - Blazing fast
    - Plain Markdown files
    - Instant fuzzy search
    - Beautiful focus mode
    - Multiple themes


3. Screenshots / Gallery
  - Show the screenshots we already have:
    - screenshots/echo.png (main view)
    - screenshots/themes.png (all themes)
    - screenshots/focus-green.png (focus mode)
    - screenshots/keyboard-shortcuts.png (infopanel inside of the app with the list of all application keyboard shortcuts)
    - screenshots/markdown-syntax.png (infopanel inside of the app with a compact overview of Markdown syntax)

4. Made with love section (small)
  - Brief mention: Built with Tauri 2 + Rust + Vite

5. Footer
  - GitHub link (https://github.com/giantvoid/echo)
  - echoedit.app
  - MIT License


Create everything inside of the already existing folder "homepage". For favicon, use homepage/favicon.png, if possible.
If possible, use Tailwind CSS (or clean modern CSS) and make it look premium and simple. The overall feeling should match the app: fast, calm, and focused.
If you need any resources (for example images), which are missing yet, create placeholder files and I will replace them with the real content files later.
Use the existing screenshots from the "screenshots" folder with correct relative paths.
Make everything static, so that there are no need to run any runtime environment. the homepage will be hosted on nginx as static website by copying the content of the homepage folder into folder, where nginx will serve it from.
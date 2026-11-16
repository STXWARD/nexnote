# Nexnote

Nexnote is a Windows-first, offline general-purpose personal notebook built with Tauri 2, React, TypeScript, and SQLite.

## Architecture

- `src/ui/` contains the React application, presentation, and typed desktop API.
- `src-tauri/src/` contains the Rust command layer and SQLite persistence.
- SQLite stores books, nested chapters, notes, tags, attachment references, soft-deletion state, and an FTS5 search index. WAL mode and foreign keys are enabled.
- Notes store rich editor HTML and a separate plain-text field; FTS5 indexes the plain-text field and title so formatted prose, tables, and code remain searchable.
- The Tauri app data directory contains `nexnote.sqlite3` and an `assets/` folder for imported images, audio recordings, and other files.
- Schema changes are versioned in `schema_migrations`; migrations should be additive and run during app startup.

## Project structure

```text
src/
  main.tsx                 React entry point
  ui/
    App.tsx                Dashboard, library workspace, note editing, search, and trash
    api.ts                 Typed Tauri command contracts
    styles.css             Light/dark design system and layouts
src-tauri/
  src/lib.rs               SQLite setup, migrations, and application commands
  src/main.rs              Windows desktop entry point
  capabilities/            Tauri window permissions
  icons/                   Windows application icon
```

## Data model

`books → chapters → notes` is the primary hierarchy. Chapters support nesting with `parent_id`, and notes point to a book and optionally a chapter. Notes use soft deletion so they can be restored. Attachments are stored as metadata and relative asset paths; binary assets live outside SQLite. A note-to-tag join table supports many-to-many tags, and a note-links table is ready for cross-note references. FTS5 indexes note titles and plain text and is maintained by triggers.

## UI direction

The interface uses a quiet, warm neutral palette with sage accents, a compact library rail, a three-column book workspace, and a distraction-free editor. Books support academic, personal, meeting, research, business, recipe, travel, reading, planning, and programming notes. Dark appearance is available from the workspace profile control.

## Run on Windows

Install the Rust toolchain and Node.js, then run `npm install` and `npm run tauri dev`. Build a Windows installer with `npm run tauri build`.

In restricted development environments, set `NEXNOTE_DATA_DIR` to a writable local folder before starting the app. Normal installations use the Windows per-user application data folder automatically.

## Delivery phases

The current build includes rich text with tables, inline images, and code blocks; reading mode; autosave; tags; general-purpose templates; context menus; book and chapter management; global full-text search with keyboard navigation and book/date filters; favourites; Recently Deleted with restore, permanent delete, and empty-trash actions; local attachments and voice recording; Markdown export; and ZIP backup/restore. Duplicate operations retain tags and attachment references, and shared attachment files are only removed after their last reference is deleted.

## Checks

Run `npm run build` for the frontend, `cargo check --manifest-path src-tauri/Cargo.toml` for the desktop backend, and `cargo test --manifest-path src-tauri/Cargo.toml --lib` for database regression checks.

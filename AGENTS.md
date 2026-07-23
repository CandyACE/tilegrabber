# AGENTS.md

TileGrabber (御图) — Tauri 2 desktop app for batch downloading/publishing map tiles. Two coupled codebases in one repo: `src/` (Vue 3 + TS frontend, npm) and `src-tauri/` (Rust backend, cargo).

## Commands

```bash
npm install            # .npmrc sets legacy-peer-deps=true — do not remove
npm run dev            # Vite only, port 4000 (strictPort). Browser with MOCKED Tauri IPC — UI work only, no backend.
npm run tauri:dev      # full desktop app (Rust + Vue). Use for any backend/IPC work.
npm run build          # frontend build (vite) — PR check #1
cargo check            # run inside src-tauri/ — PR check #2
npm run tauri:build    # release bundle
```

No test suite, linter, or formatter is configured. Don't invent `npm test` / `npm run lint`. For a TS typecheck use `npx vue-tsc --noEmit` (vue-tsc is a devDep, not a script).

## Architecture

- **Two entry points / windows**: `index.html`→`src/main.ts`→`App.vue` (main window, frameless, `decorations:false`, custom titlebar) and `float.html`→`src/float-main.ts`→`FloatApp.vue` (always-on-top floating speed window). Each window has its own capability file in `src-tauri/capabilities/` (`default.json`, `float.json`). Don't assume a single window/entry.
- **Rust entry**: `src-tauri/src/lib.rs::run()` registers every Tauri command in one `invoke_handler!` macro; `main.rs` just calls `run()`. `main.rs` carries `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` — DO NOT REMOVE (hides the debug console window in release).
- **Backend modules** (`src-tauri/src/`): `commands/` (Tauri command handlers), `download/` (engine + `clip_pipeline.rs`), `export/` (directory/MBTiles/PMTiles/GeoTIFF), `parser/` (LRC/LRA/WMTS/web capture), `server/` (axum HTTP publish: TMS/WMTS/WMS/OGC API/ArcGIS), `storage/` (`AppDb` via rusqlite **bundled** SQLite), `remote/`, `gcj02.rs`, `tile_math.rs`, `types.rs`.
- **Storage model**: per-task tile data lives in `.tiles` SQLite files under Tauri `app_data_dir()`; `AppDb` holds tasks/layers/settings. `.tgr` task import/export is SQLite-based (zero-copy).
- **Path aliases**: `@` and `~` both resolve to `src/` (vite.config.ts + tsconfig.json). `cn()` helper at `src/lib/utils.ts`.
- **Vendored crate**: `Cargo.toml` patches `tiff` to `vendor/tiff` via `[patch.crates-io]`. Don't "fix" by removing the patch or upgrading `tiff` blindly.

## Tile pipeline quirks (easy to break)

- **Raster vs vector**: a source is treated as vector (MVT/PBF) when its URL contains `pbf`, `mvt`, or `vector`. Vector tasks **skip** GCJ02 pixel correction and `tile_clip` (raster-only post-processing). Preview auto-generates a skeleton style; MBTiles/PMTiles export gzip-wraps tiles and writes `vector_layers` metadata.
- **GCJ02** (China coordinate offset) is handled in `src-tauri/src/gcj02.rs` and `src/lib/gcj02.ts`. GCJ02 raster tasks must composite before clipping, so they take the legacy post-clip path — not the streaming clip pipeline.
- **Streaming clip pipeline** (`download/clip_pipeline.rs`) gates on `clip_to_bounds && !vector && !GCJ02`. Vector and GCJ02 tasks intentionally bypass it.

## Conventions

- **CHANGELOG.md is mandatory** for every change: append under the top `## [vX.Y.Z] - 待发布` section, categorized (`### 新增` / `### 修复` / `### 优化` / `### 安全` / `### 破坏性变更`). CI extracts release notes by awk-matching `## [vX.Y.Z]`, so the heading must match the tag (minus the `v`). Breaking changes must include a migration path.
- **Branches**: `main` (always releasable) + `feat/*` feature branches. No `develop`.
- **Commits**: conventional commits, often scoped — `feat(download):`, `fix(ui):`, `chore:`, `docs:`. Match existing style.
- **i18n**: vue-i18n, locale files in `src/locales/` (Chinese primary + English). Add every user-facing string to both; the UI and product name (御图) are Chinese-first.
- **UI stack**: Reka UI + Tailwind CSS **v4** (CSS-first: `@import 'tailwindcss'` + `@theme` in `src/assets/css/main.css`; **no `tailwind.config.js`**). shadcn-vue `new-york` style (`components.json`), components under `src/components/ui/`. Map rendering = MapLibre GL JS; area drawing = Terra Draw + `terra-draw-maplibre-gl-adapter`.

## Release

- **Tag-triggered**: pushing a `v*` tag runs `.github/workflows/release.yml` (also `workflow_dispatch` with a tag). There is no CI on PRs.
- **Don't bump `src-tauri/Cargo.toml` version manually for releases** — CI syncs it from the tag. The Cargo version is only authoritative between releases.
- Before tagging: change the CHANGELOG heading from `- 待发布` to the real date, commit, then `git tag vX.Y.Z && git push --tags`.
- **Updater signing** (minisign): pubkey is baked into `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`; private key comes from CI secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. A fork **must** generate its own keypair and replace the pubkey, or CI emits updates no client will accept. `src-tauri/update_url` is gitignored (build-time injection).
- `tauri.conf.json` bundles **NSIS only** (`"targets": ["nsis"]`); the CI matrix adds macOS dmg + Linux AppImage/deb via `--bundles`. `scripts/` holds Windows-only release helpers — `pack-release.ps1` (post-build NSIS packaging) and `relink-msi.ps1` (workaround for a Tauri 2.10.1 `light.exe` CLI bug; only relevant if building MSI locally).

## Notes

- `.codegraph/` is present — prefer codegraph tools (`codegraph_explore` / `codegraph_node`) to navigate before reading source files.

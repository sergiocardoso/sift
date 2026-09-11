# Sift docs

The official documentation site for [Sift](https://github.com/sergiocardoso/sift), built with [Astro](https://astro.build) + [Starlight](https://starlight.astro.build). This is a standalone Node/pnpm project inside the Sift repo — it isn't part of the Cargo workspace and doesn't affect the `sift`/`sift-tray` binaries.

## Commands

Run from this directory (`website/`):

| Command | Action |
| --- | --- |
| `pnpm install` | Install dependencies |
| `pnpm dev` | Start the local dev server at `localhost:4321` |
| `pnpm build` | Build the static site to `./dist/` |
| `pnpm preview` | Preview the production build locally |

## Structure

```
src/
├── assets/           images imported by content (e.g. the logo used in the hero)
├── content/docs/     one Markdown file per page, mirrors the sidebar in astro.config.mjs
└── styles/custom.css theme overrides (color tokens, dark/light)
public/
├── favicon.png
└── images/
    ├── logo/         reused Sift brand assets
    ├── marketing/     reused hero/feature banners, not currently linked from any page
    └── screenshots/   empty — drop future product screenshots here
```

Sidebar order is explicit in `astro.config.mjs` (not auto-generated from the filesystem), so adding a page also means adding it to the sidebar there.

## Content source of truth

Page content is derived from the project's root `README.md` and cross-checked against the Rust source for anything not obvious from the README alone (exact flags, config field names, default values). It intentionally doesn't duplicate the README's prose verbatim — see each page for the restructured, docs-oriented version.

## i18n

English only for now, declared as an explicit `root` locale in `astro.config.mjs` rather than left implicit. Adding a language later means adding a sibling locale entry there and moving pages under `src/content/docs/<lang>/` — no restructuring of what already exists.

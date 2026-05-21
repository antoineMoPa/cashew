# Cashew Project Context

Cashew is a cached, Excel-like desktop application for media-generation workflows. The frontend is built with Dioxus Desktop for CSS-driven layout and styling control, and the Rust backend should own document state, formula evaluation, cache keys, provider integration, and JSON serialization.

The product direction is a spreadsheet where users can prototype prompt pipelines and media workflows. A typical flow is:

1. Draft or manipulate prompts and scenario/storyboard data in cells.
2. Generate storyboard images from prompts and references.
3. Generate videos from images, prompts, and other inputs.
4. Concatenate outputs into longer media, up to complete short films.

Some users may only need image-generation workflows, so media features should stay modular rather than assuming every project becomes a full movie pipeline.

Core formula examples planned for later include:

```text
=GENERATEIMAGE(A1, A2)
=GENERATEVIDEO(...)
```

These formulas are intentionally not implemented yet. When they are added, formula execution must be cache-first: identical formulas with identical resolved inputs should reuse stored results so users do not pay twice for the same provider call. External generation providers, especially `fal.ai`, should be integrated behind backend abstractions rather than called directly from UI code.

Files should remain serializable to JSON. Preserve compatibility with saved documents by versioning schema changes and keeping cache entries explicit in the document model.

UI direction:

- Keep the first screen as the actual spreadsheet-like workspace.
- Use CSS to keep the grid visually close to Google Sheets/Excel: edge-to-edge workspace, compact cells, row/column headers, formula bar, and resize handles.
- Maintain a File menu for document actions; add an Edit menu later.
- Treat media-generation state as document data, not transient frontend-only state.

Engineering direction:

- Keep frontend and backend concerns separated.
- Prefer deterministic cache keys based on formula identity and resolved inputs.
- Avoid implementing real provider calls until the formula and cache model are ready.
- Keep tests around document serialization and cache behavior as the model evolves.

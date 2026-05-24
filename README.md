# Cashew

Cashew is a desktop spreadsheet for building cached AI workflows.

The idea is simple: each cell can hold plain text, a formula, or the output of a provider call. Formulas can reference other cells, so you can build multi-step pipelines directly in a grid instead of wiring nodes or writing glue code. Results are cached in the document, so identical inputs can reuse previous outputs instead of calling a provider again.

The app is split into two parts:

- `Rust` backend for document state, formula evaluation, caching, provider integration, and JSON save/load
- `Dioxus` desktop frontend for the spreadsheet UI, menus, formula bar, and panels

Cashew is focused on prototyping workflows for text, image, video, and other AI-generated media while keeping the document portable and serializable.

## Development

Serve with

```
dx serve --desktop
```

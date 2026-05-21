# Cashew Project Context

Cashew is a cached spreadsheet desktop application for orchestrating `fal.ai` workflows from cells. The frontend is built with Dioxus Desktop for CSS-driven layout and styling control, and the Rust backend should own document state, formula evaluation, cache keys, provider integration, and JSON serialization.

The product direction is a spreadsheet-based UI for AI prototyping, providing a structured alternative to node UI pipelines and code-driven approaches. It should empower creatives to design custom AI workflows combining multiple AI models in a cost-effective way with result caching. Cells can hold raw inputs, formulas, intermediate outputs, model choices, prompt variants, references, structured JSON, and final artifacts.

Important workflow categories include:

- LLM calls for prompt manipulation, planning, summarization, extraction, and structured text generation.
- Image, video, audio, and other media generation through later formula functions.
- Multi-step pipelines where one cell's output becomes another cell's input.
- Batch exploration of parameter grids such as model, prompt, seed, style, size, temperature, or other provider-specific options.

Formula execution must be cache-first. Identical formulas with identical resolved inputs should reuse stored results so users do not pay twice for the same provider call. Provider calls should go through backend abstractions and should never be called directly from UI code.

Files should remain serializable to JSON. Preserve compatibility with saved documents by versioning schema changes and keeping cache entries explicit in the document model.

UI direction:

- Keep the first screen as the actual spreadsheet-like workspace.
- Use CSS to keep the grid visually close to a familiar spreadsheet: edge-to-edge workspace, compact cells, row/column headers, formula bar, and resize handles.
- Maintain a File menu for document actions; add an Edit menu later.
- Treat provider outputs, cache state, and generated artifacts as document data, not transient frontend-only state.

Engineering direction:

- Keep frontend and backend concerns separated.
- Prefer deterministic cache keys based on formula identity and resolved inputs.
- Keep provider integrations modular so new `fal.ai` endpoints can be added as formula functions without rewriting UI code.
- Keep tests around document serialization, formula evaluation, provider request construction, and cache behavior as the model evolves.

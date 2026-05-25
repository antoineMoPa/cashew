= Cashew Caching System

Cashew uses a cache-first execution model for formulas and provider calls.
The backend owns cache state, and the frontend never calls providers directly.

== Core rule

If two formulas have the same formula identity and the same resolved inputs,
they should reuse the same stored result.

== What the cache stores

- cache entries live inside the document JSON
- formula outputs are keyed deterministically
- provider results are preserved so reopening a file does not force a new call

== Practical effect

- editing a referenced cell invalidates dependent results
- identical reruns reuse existing outputs when inputs do not change
- the spreadsheet stays predictable and cheap to iterate on

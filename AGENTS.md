# Repository language

Write all repository content in English, including interface labels, status and
error messages, accessibility text, documentation, comments, test descriptions,
and commit messages. Preserve Unicode coverage using non-French test fixtures.

# Verification

Use the real Rust/WebAssembly engine for browser tests. Keep source processing
local to the browser and do not execute submitted scripts in the website.

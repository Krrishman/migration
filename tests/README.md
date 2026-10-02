# Tests

- `tests/ui.test.tsx` contains UI flow tests (Vitest + Testing Library) against the mock backend.
- `src/lib/*.test.ts` contains unit tests for formatting and selection rules.
- `src-tauri/src/**` contains Rust unit tests (`#[cfg(test)]`).
- `src-tauri/tests/*.rs` contains Rust integration tests (capture, restore, session) against `fixtures/`.

Run everything with `npm run test:all`. See `docs/TESTING.md` for the requirement-to-test map.

# Fixtures

Fake Windows PCs used by tests, by `--fixture` mode and by the browser preview.
Regenerate them with `python3 scripts/generate_fixtures.py` (the output is deterministic).

- `source-pc/` is the source computer "ACCT-PC-07". `platform.json` describes the machine, profiles, apps, printers and drives. `Users/` holds real profile trees.
- `target-pc/` is the destination computer "ACCT-PC-21", with existing files for collision tests.

Files containing `FIXTURE-SECRET-DO-NOT-COPY` represent protected secrets (browser passwords, cookies, credential stores, Wi-Fi keys, private keys). Tests fail if this marker ever reaches a bundle or a restore target.

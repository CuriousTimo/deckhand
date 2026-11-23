# This fork

CuriousTimo's fork of [OpenActionAPI/rust-elgato-streamdeck](https://github.com/OpenActionAPI/rust-elgato-streamdeck),
the one Stream Deck implementation for all of CuriousTimo's projects (Maker Connect, Hive Rail, ...).

- **`main`**: an exact copy of upstream `main`, fast-forwarded every day by
  [`follow-upstream.yml`](.github/workflows/follow-upstream.yml). Never commit to it.
- **`curioustimo`** (default branch): our changes on top. Every change that differs from upstream is
  listed below, with why.
- **New upstream commits** arrive as one pull request `main` → `curioustimo`, with a build check of
  the merge. A fix that is useful to everyone is offered upstream as well, so this list stays short.
- **Using it**: pin a commit of `curioustimo`:
  `elgato-streamdeck = { git = "https://github.com/CuriousTimo/rust-elgato-streamdeck", rev = "<commit>" }`
- **Licence**: MPL-2.0, as upstream. Changed files stay MPL-2.0 and are published here.

## Changes against upstream

| Change | Files | Why |
|---|---|---|
| This file and the workflow that follows upstream | `FORK.md`, `.github/workflows/follow-upstream.yml` | keeping up with upstream; no change to the library |

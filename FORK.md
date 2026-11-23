# deckhand

**deckhand** is CuriousTimo's fork of [OpenActionAPI/rust-elgato-streamdeck](https://github.com/OpenActionAPI/rust-elgato-streamdeck),
the one Stream Deck implementation for all of CuriousTimo's projects (Maker Connect, Hive Rail, ...).

- **`main`**: an exact copy of upstream `main`, fast-forwarded every day by
  [`follow-upstream.yml`](.github/workflows/follow-upstream.yml). Never commit to it.
- **`curioustimo`** (default branch): our changes on top. Every change that differs from upstream is
  listed below, with why.
- **New upstream commits** arrive as one pull request `main` → `curioustimo`, with a build check of
  the merge. A fix that is useful to everyone can also be offered upstream (the owner decides), so this
  list stays short.
- **Using it**: pin a commit of `curioustimo`:
  `deckhand = { package = "elgato-streamdeck", git = "https://github.com/CuriousTimo/deckhand", rev = "<commit>" }`,
  so the code says `use deckhand::...`. A project that picks its own `hidapi` backend adds
  `default-features = false` and turns that backend on in its own `hidapi` dependency (see the change below). The package name in `Cargo.toml` stays `elgato-streamdeck`, as upstream,
  so upstream's releases merge without conflicts (`deckhand` on crates.io is someone else's crate; this one is not published).
- **Licence**: MPL-2.0, as upstream. Changed files stay MPL-2.0 and are published here.

## Used in

| Project | How | State |
|---|---|---|
| [Maker Connect](https://github.com/CuriousTimo/Maker-Connect) | a Stream Deck beside the machines: machine status and actions, lockers, a weigh station at the desk ([#190](https://github.com/CuriousTimo/Maker-Connect/issues/190), [wiki](https://github.com/CuriousTimo/Maker-Connect/wiki/Design:-Stream-Deck)) | **idea, work in progress**: a demo with made-up data, [`examples/maker_connect_demo.rs`](examples/maker_connect_demo.rs); not in Maker Connect's code yet |

## Changes against upstream

| Change | Files | Why |
|---|---|---|
| This file and the workflow that follows upstream | `FORK.md`, `.github/workflows/follow-upstream.yml` | keeping up with upstream; no change to the library |
| A key test example (owner, 2026-11-23: "zet het testprogramma als example in deckhand") | `examples/key_test.rs` | every key its colour and number, white while held, key 0/1 brightness, last key clears and stops; tested on an Original V2 (fw 1.03.000) on macOS; no change to the library |
| `hidapi` without its default features; a feature `hidapi-default-backend` (on by default) gives them back (owner, 2026-11-23: "deckhand is van ons en precies de reden dat we een fork hebben gedaan", "fix de hidapi default features in deckhand") | `Cargo.toml` | hidapi allows exactly one backend per system (`build.rs:105-106` at 2.6.7: "Exactly one linux hidapi backend must be selected."). Upstream's `hidapi = "2.6"` always turned on hidapi's default `linux-static-hidraw`, so a project with its own backend (Maker Connect's agent: `linux-native-basic-udev`) could not build for Linux. Checked: deckhand `--all-features` builds and tests as before; a project with `default-features = false` and Maker Connect's hidapi features checks for x86_64 and aarch64 Linux, Windows and macOS. Build only: nothing changes on the wire. |
| **Idea, work in progress:** a Maker Connect mockup example (owner, 2026-11-23: "zet de demo in de deckhand repo als example", "mark het wel als idea/WIP"; [Maker-Connect#190](https://github.com/CuriousTimo/Maker-Connect/issues/190)) | `examples/maker_connect_demo.rs`, `Cargo.toml` (`[dev-dependencies]` `ab_glyph`, `image` with `png`) | pages of machines, a locker menu and a weigh station with made-up data, in Maker Connect's house style; talks to no hub; macOS only for now (system font); run on an Original V2 (fw 1.03.000); no change to the library |

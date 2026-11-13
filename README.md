# Starbound Outfit Studio

Create Starbound clothing, hats, and character redraws from PNG artwork. Rust / WebAssembly handles image decoding and encoding locally—**PNGs are not uploaded**. Exports reference vanilla assets, so other players do not need to install your images.

**[Open the web app](https://erodeesfleurs.github.io/Starbound-OutfitStudio/)** · [Quick start](#quick-start) · [Usage](#usage) · [CLI](#cli) · [Development](#development)

| Tool | Output |
| --- | --- |
| Pants, body-hiding pants, chest/sleeves, back items | Item JSON or `/spawnitem` commands |
| Hats with custom metadata and hair/body visibility controls | Item JSON or `/spawnitem` commands |
| Character redraws, including hair-only editing | Identity patches or full identities; backup/undo exports in the web app |
| Direct recoloring from paired source/target PNGs | A single `?replace` directive |

## Quick start

```sh
git clone https://github.com/ErodeesFleurs/Starbound-OutfitStudio.git
cd Starbound-OutfitStudio
nix develop path:./nix
cargo xtask serve
```

Open <http://127.0.0.1:4200>. No Node.js or frontend package manager is required.

<details>
<summary>Without Nix</summary>

From the project root, install the versions pinned by [rust-toolchain.toml](rust-toolchain.toml) and [Cargo.toml](Cargo.toml):

```sh
rustup toolchain install 1.100.0 --profile minimal \
  --component rustfmt --component clippy --target wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked
cargo xtask serve
```

</details>

## Usage

Choose a mode, download its PNG/PDN template, draw your artwork, and upload or drop the PNG into the editor. Preserve frame positions, the 43 × 43 frame grid, and transparent background. Chest bodies and sleeves can be supplied individually or combined. Inspect the preview, then copy or download the result.

Use the header's **Language** selector for **简体中文** or **English**. The initial language follows browser preferences, defaulting to English when unsupported; explicit choices are remembered when local storage is available. Switching preserves uploads, settings, user input, and generated output. Native file controls follow the browser's own language.

Clipboard access requires localhost or HTTPS and browser permission. If denied, download the result or copy it manually. PNG decoding preserves 8-bit RGBA, including hidden RGB in transparent pixels; Canvas is only used for previews.

### Image sizes

All dimensions are width × height in pixels. Use the correct mode's template even when dimensions match.

| Input | Size |
| --- | --- |
| Pants; character body/legs | 387 × 258 |
| Chest body | 86 × 258 |
| Sleeves, back items, character arms | 387 × 301 per layer |
| Character base head | 43 × 43, or vanilla 86 × 215 |
| Expression overlays | 387 × 344 |
| Hair | 43 × 43 |
| Hat | 1–256 per axis |

An exact 86 × 215 head or hat uses the upper-right 43 × 43 `normal` frame at `(43, 0)`; other hat sizes use the full image, centered.

Fixed clothing and character sheets may have fully transparent right/bottom padding, which is removed without scaling, shifting frames, or modifying the original file. Visible pixels outside the template are rejected. Top/left margins and internal blank cells are not trimmed. Hats and recoloring follow their own size rules.

### Character redraws

**Back up the complete `.player` save first. Character output edits identity, not equipment.**

- Provide body, both arms, base head, and expression overlays together. Hair is optional and may be edited alone if the original identity has a valid, nonempty hair style. To hide a linked part, provide a transparent PNG rather than omitting it.
- Draw the base head once; expression overlays add differences above it. Transparent overlay pixels retain the head. Frames sharing an encoding ID must match, and unused cells must stay empty.
- Draw on blank PNGs using `*-layout.svg` guides. Annotated `*-reference.svg` files are visual references—not uploadable templates or exact source sheets for recoloring. Do not bake labels or grids into artwork.
- Supply your actual **identity JSON object**, not a `.player` binary or full save. Replace the web app's example identity; example-based backups and undo exports cannot restore your real character.
- **Merge patches into identity. Never run `/identity set PATCH` or overwrite a `.player` file with JSON.** Commands use the OpenStarbound client's local `/run` API; `/run` and `/identity` are not vanilla server commands. Exit the game before offline save editing. Save and rejoin to check changes; restore from the original backup or matching undo export when needed.

Full redraws use a fixed Floran image source while preserving species and other non-drawing identity fields. Previews are not complete game simulations; equipment may still mask redrawn layers.

### Direct recoloring

Provide actual source/target PNGs for **every affected frame and shared layer**. Each pair must have identical dimensions. One source RGBA must map to exactly one target RGBA across all pairs, including unchanged pixels and hidden transparent RGB. Conflicts are rejected; missing frames are not validated.

This is color mapping, not arbitrary local drawing. Apply the output to the exact source images and processing stage supplied. Alpha changes affect all occurrences of the source color. Identical images yield a valid empty result: web downloads are zero-byte files; CLI output retains a trailing newline.

## CLI

Run from the project root inside the development environment. Replace example paths with your own files.

```sh
# Equipment JSON; --stats writes statistics to stderr.
cargo run -p outfit-cli -- encode --garment pants --input art/pants.png \
  --format json --out outfit.json --stats

# Linked character redraw; the backup file must not already exist.
cargo run -p outfit-cli -- character-encode --identity identity.json \
  --body art/body.png --front-arm art/front-arm.png --back-arm art/back-arm.png \
  --head art/head.png --emotes art/emotes.png --hair art/hair.png \
  --backup-out identity.backup.json --format json --out identity.patch.json

# Recoloring; repeat source/target flags for additional pairs.
cargo run -p outfit-cli -- recolor \
  --source art/original.png --target art/edited.png --out recolor.txt
```

Equipment kinds: `pants`, `hidingPants`, `chest`, `back`, `hat`. Export formats: `json`, `command`, `directives`; characters also support `merged`. Character directives-only output may omit required image-source fields and is not a standalone identity patch. For hair-only editing, supply only `--identity` and `--hair` as inputs.

`--backup-out` privately saves the exact original identity and refuses to overwrite files; it does not replace a full save backup. Hats do not accept clothing layer/profile flags. Full options and character frame coordinates:

```sh
cargo run -p outfit-cli -- --help
cargo run -p outfit-cli -- character-encode --help
cargo run -p outfit-cli -- recolor --help
```

### Encoding options

Defaults: `--profile auto --strategy text --palette auto`.

- `text`: minimize complete directive text length.
- `compressed`: minimize the full directive's Deflate estimate.
- `passes`: disable palette aliases and use one pixel-assignment stage per output field; fixed template operations remain unchanged.

Clothing `auto` compares `original`, `conservative`, and `compact` helper-image profiles. Explicit profiles restrict the search. Hats and characters use separate paths; direct recoloring ignores these options. Search is bounded, not globally optimal. Browser and CLI compression may choose different pixel-equivalent candidates; estimates are **not network packet sizes or FPS measurements**.

## Compatibility

The baseline is Starbound 1.4.4, with historical native validation on vanilla and OpenStarbound 0.1.15.1. Engine builds and resource-replacement mods may affect fixed sampling; previews do not guarantee every race, animation, or multiplayer setup. Directives cannot add skeletons, animation states, or collision geometry.

Server-side `/spawnitem` requires administrator permission. Character editing tools and server rules are separate from asset compatibility. **Saved items and identities do not update automatically:** regenerate and reapply exports after relevant fixes.

## Development

The workspace contains `outfit-core` (encoding), `outfit-web` (browser interaction), and `outfit-cli` (native tools). `web/` and `public/assets/` contain the page and resources; `xtask/` builds and serves the site.

Development requires Rust 1.100.0 and wasm-bindgen-cli 0.2.130. Keep the Rust pins in `rust-toolchain.toml` and `nix/flake.nix` aligned; the wasm-bindgen CLI must match the exact crate version in `Cargo.toml`. CI uses the locked Nix development environment.

UI translations live in `public/assets/i18n/messages.js`. Keep named placeholders intact; filenames, identifiers, JSON, and commands are not translated.

Run the [CI checks](.github/workflows/verify.yml) in the development environment:

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p outfit-web --target wasm32-unknown-unknown --locked -- -D warnings
cargo xtask build
```

CI needs no commercial assets. Optional Linux `verify-assets` and `native-verify` commands require legally supplied game assets/binaries; consult their CLI help. Generate private fixtures in a new or empty directory outside the repository and run probes in new isolated storage—not existing worlds or saves. Never commit or publish game packages, private fixtures, or player saves.

## Deployment

`cargo xtask build` produces a static site in `dist/`. Upload **only that directory**, including `pkg/`, `assets/`, and license files, to an HTTP(S) host. No encoding backend is needed. Use HTTPS for remote clipboard access; do not open the app through `file://`.

For GitHub Pages, set repository **Settings → Pages → Source** to **GitHub Actions**. The [workflow](.github/workflows/verify.yml) deploys verified `dist/` builds from `main` (push or manual run) to <https://erodeesfleurs.github.io/Starbound-OutfitStudio/>; pull requests only run checks. It uses GitHub's built-in token, not VPS credentials.

Inspect an existing build with `cargo xtask serve --no-build --port 4201`. [deploy/](deploy/) contains hosting examples; adapt their addresses and paths. Publish complete releases atomically and keep the previous version for rollback. Static updates should not overwrite the existing VPS's NixOS-managed services or certificate state.

## Credits and licenses

- Based on [Silverfeelin's NgOutfitGenerator](https://github.com/Silverfeelin/Starbound-NgOutfitGenerator), retaining its MIT license, templates, and Nettle Boy's directive scheme.
- Hatter-derived algorithms, including shared coordinate-canvas geometry, are adapted from [Starbound Hatter](https://github.com/Silverfeelin/Starbound-Hatter) under **CC BY 3.0**, not MIT.
- Original integration code, blank layouts, brand artwork, and mode icons use MIT. [Silkscreen](https://github.com/googlefonts/silkscreen) uses [SIL OFL 1.1](public/assets/fonts/OFL.txt).
- Starbound preview/reference artwork belongs to **Chucklefish and/or its original rights holders**. The software licenses do not relicense it or grant additional redistribution rights.

This is an unofficial tool, independent of the OpenStarbound source repository. Full notices, pinned upstream references, and community technique credits are in [LICENSE](LICENSE).

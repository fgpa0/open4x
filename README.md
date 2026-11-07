# Open4X

A free, open-source, turn-based 4X strategy game in the tradition of Civilization, written in Rust with [Bevy](https://bevyengine.org). It's in active development.

- **Fast AI turns.** Computer players act only through the same commands a human player uses.
- **Automation you can always override:** standing orders for units, city governors, auto-research and automated workers.
- **Group orders.** Select many units, right-click an enemy city to take it or one of yours to defend it, and they carry the order out together.
- **Moddable rules.** Terrain, units, buildings, technologies, civilizations and more are plain [RON](https://github.com/ron-rs/ron) files in `data/`.
- **Procedural art.** Every model, texture and icon is generated in code: Rust, or Python scripts for Blender. The only third-party assets are the OFL fonts.
- **A living world.** Day and night run in real time, seasons follow the turns, and weather follows the climate, with wildlife and ambient life around towns.

Ten civilizations, six eras (Ancient to Modern) and several map types: Continents, Pangaea, Archipelago, Fractal, Inland Sea, Terra, and Earth with true start locations.

## Playing

Download the latest build for Linux, Windows or macOS from the [Releases](../../releases) page, unpack it and run `open4x` (`open4x.exe` on Windows). Keep the `data` folder next to the executable.

The builds aren't signed. On Windows, if SmartScreen warns you, choose *More info → Run anyway*. On macOS, clear the quarantine flag once with `xattr -d com.apple.quarantine open4x`.

Saves go to `~/.local/share/open4x/saves` on Linux, `%APPDATA%\open4x\saves` on Windows and `~/Library/Application Support/open4x/saves` on macOS. Press the **?** in the top bar in game for the controls.

## Building from source

You need a [Rust toolchain](https://rustup.rs) and [Git LFS](https://git-lfs.com). The 3D models are stored in LFS, so run `git lfs install` before cloning.

```sh
git clone <repository url>
cd open4x
cargo run --release -p client
```

On Linux you also need the development packages for ALSA, udev, Wayland and xkbcommon. On Debian or Ubuntu:

```sh
sudo apt install libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
```

Use `--release`: debug builds are slow.

Other useful commands:

```sh
cargo test --workspace --release            # all tests
cargo run --release -p headless -- 200 6 1  # an AI-only game: turns, players, seed
```

The models are prebuilt and committed, so you don't need Blender to build the game. To change them, edit the scripts in `art/models` and rebuild with Blender 5.2: `blender -b --factory-startup -P art/build.py`.

## Layout

| Path | What it holds |
| --- | --- |
| `crates/sim` | The rules: a pure, deterministic simulation with no engine dependency |
| `crates/ai` | Computer players and automation, acting only through the commands players use |
| `crates/client` | The Bevy game client |
| `crates/headless` | Runs AI-only games for testing and benchmarks |
| `data` | Game rules as RON files |
| `art` | Blender scripts that generate the models |

[`CLAUDE.md`](CLAUDE.md) documents the architecture in detail.

## Modding

The rules load from the `data` folder next to the executable, or from the folder named by the `FOURX_DATA` environment variable. The game checks the files when it loads them, so a broken cross-reference is reported at startup. Saves made with different rules are refused.

## Releasing

Push a version tag and GitHub Actions builds, tests and publishes the release:

```sh
git tag v0.1.0
git push origin v0.1.0
```

## License

Open4X is free software under the [GNU General Public License v3.0 or later](LICENSE). You may use, study, change and share it; versions you distribute must stay under the same license. The fonts (Inter and Cinzel) are under the SIL Open Font License.

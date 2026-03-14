# yggcli

`yggcli` is the terminal front door for the Yggdrasil ecosystem.
It writes the native config files used by `yggdrasil`, `yggclient`, and `yggsync` instead of hiding state inside a private database.
If local config files already exist, `yggcli` loads them first so reruns feel like editing a living workspace instead of starting from zero.

## Stack

- Rust
- ratatui
- crossterm

## Quick Start

```bash
curl -fsSL https://raw.githubusercontent.com/yggdrasilhq/yggcli/main/install.sh | bash
yggcli --help
```

Fallback for local development:

```bash
cargo run
```

Controls:

- `Tab` / `Shift+Tab`: switch section
- `Up` / `Down`: move between fields
- `Enter`: toggle boolean fields
- `Ctrl-S`: write config files into the selected workspace
- `q`: quit

## Output

The current scaffold writes:

- `yggdrasil/ygg.local.toml`
- `yggclient/yggclient.local.toml`
- `yggclient/config/profiles.local.env`
- `yggsync/ygg_sync.local.toml`

The intent is simple:

- the repos stay independently usable
- the TUI becomes the polished front end
- power users can still open and edit the generated files directly

## Platform Behavior

- Linux hosts can bootstrap the full workspace and run `yggdrasil` build/smoke actions.
- Android/Termux hosts bootstrap only `yggcli`, `yggclient`, `yggsync`, and `yggdocs`.
- Android/Termux hosts do not run `yggdrasil` ISO builds or smoke benches.
- Re-running `yggcli` detects existing local config files and lets you inspect or overwrite them deliberately.

## License

Apache-2.0

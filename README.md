# yggcli

`yggcli` is the terminal front door for the Yggdrasil ecosystem.
It writes the native config files used by `yggdrasil`, `yggclient`, and `yggsync` instead of hiding state inside a private database.

## Stack

- Rust
- ratatui
- crossterm

## Quick Start

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

## License

Apache-2.0

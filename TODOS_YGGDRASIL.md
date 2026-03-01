# TODOS Yggdrasil Integration

1. Add a full local-config wizard that writes `ygg.local.toml` from `ygg.example.toml` defaults.
2. Support profile selection (`server`, `kde`, `both`) and pass it to `mkconfig.sh --profile`.
3. Validate required fields for `setup_mode = "recommended"` before launching builds.
4. Add SSH key file picker/validator for `ssh_authorized_keys_file`.
5. Add network mode UX for `dhcp` vs `static` with static field validation.
6. Add APT proxy fields (`apt_http_proxy`, `apt_https_proxy`, `apt_proxy_bypass_host`).
7. Add a preview/diff step before writing local config files.
8. Add command execution flow to run `./mkconfig.sh --config ./ygg.local.toml`.
9. Add build progress + smoke summary display from command output.
10. Add release helper UX for ISO retention policy and latest artifact locations.
11. Add docs links to `yggdrasil` README and config examples.
12. Add tests for config generation parity with `yggdrasil/scripts/toml-to-env.sh` behavior.

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
    Terminal,
};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{self, Stdout, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const DEFAULT_REPO_BASE: &str = "https://github.com/yggdrasilhq";
const DEFAULT_WORKSPACE_FALLBACK: &str = "/root/gh";
const ECOSYSTEM_REPOS: &[&str] = &[
    "yggdrasil",
    "yggcli",
    "yggclient",
    "yggsync",
    "yggdocs",
    "yggterm",
    "yggtopo",
];
const ANDROID_REPOS: &[&str] = &["yggcli", "yggclient", "yggsync", "yggdocs"];

#[derive(Clone, Debug)]
struct Platform {
    os: String,
    arch: String,
    is_android: bool,
    is_termux: bool,
}

impl Platform {
    fn detect() -> Self {
        let prefix = env::var("PREFIX").unwrap_or_default();
        let termux_files = Path::new("/data/data/com.termux/files/usr/bin/bash").exists();
        let is_termux = prefix.contains("com.termux") || termux_files;
        let is_android = env::consts::OS == "android" || is_termux;

        Self {
            os: env::consts::OS.into(),
            arch: env::consts::ARCH.into(),
            is_android,
            is_termux,
        }
    }

    fn supports_host_builds(&self) -> bool {
        !self.is_android
    }

    fn repos(&self) -> &'static [&'static str] {
        if self.is_android {
            ANDROID_REPOS
        } else {
            ECOSYSTEM_REPOS
        }
    }

    fn label(&self) -> String {
        let flavor = if self.is_termux {
            "termux"
        } else if self.is_android {
            "android"
        } else {
            "standard"
        };
        format!("{}/{}/{}", self.os, self.arch, flavor)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Workspace,
    Yggdrasil,
    Yggclient,
    Yggsync,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Section::Workspace => "Workspace",
            Section::Yggdrasil => "Server",
            Section::Yggclient => "Client",
            Section::Yggsync => "Sync",
        }
    }

    fn all() -> [Section; 4] {
        [
            Section::Workspace,
            Section::Yggdrasil,
            Section::Yggclient,
            Section::Yggsync,
        ]
    }
}

#[derive(Clone)]
struct Field {
    label: &'static str,
    help: &'static str,
    value: String,
    bool_field: bool,
}

impl Field {
    fn text(label: &'static str, value: impl Into<String>, help: &'static str) -> Self {
        Self {
            label,
            help,
            value: value.into(),
            bool_field: false,
        }
    }

    fn boolean(label: &'static str, value: bool, help: &'static str) -> Self {
        Self {
            label,
            help,
            value: if value { "true" } else { "false" }.into(),
            bool_field: true,
        }
    }

    fn as_bool(&self) -> bool {
        self.value == "true"
    }
}

#[derive(Default, Serialize, Deserialize)]
struct YggdrasilConfig {
    build_profile: String,
    enable_qemu_smoke: bool,
    with_nvidia: bool,
    with_lts: bool,
    setup_mode: String,
    apt_proxy_mode: String,
    infisical_boot_mode: String,
    infisical_container_name: String,
    embed_ssh_keys: bool,
    ssh_authorized_keys_file: String,
    hostname: String,
    net_mode: String,
    lxc_parent_if: String,
    macvlan_cidr: String,
    macvlan_route: String,
    static_iface: String,
    static_ip: String,
    static_gateway: String,
    static_dns: String,
    apt_http_proxy: String,
    apt_https_proxy: String,
    apt_proxy_bypass_host: String,
}

#[derive(Default, Serialize, Deserialize)]
struct IdentityConfig {
    profile_name: String,
    user_name: String,
    user_home: String,
}

#[derive(Default, Serialize, Deserialize)]
struct NetworkConfig {
    ssh_host: String,
    ssh_user: String,
    apt_http_proxy: String,
    apt_https_proxy: String,
}

#[derive(Default, Serialize, Deserialize)]
struct SyncConfig {
    enable_yggsync: bool,
    yggsync_repo: String,
    yggsync_config: String,
    samba_host: String,
    samba_share: String,
    samba_user: String,
    samba_username: String,
    samba_password_env: String,
    screencasts_remote: String,
    use_mounted_nas: bool,
    mounted_nas_root: String,
}

#[derive(Default, Serialize, Deserialize)]
struct ServicesConfig {
    install_desktop_timer: bool,
    install_shift_sync: bool,
    install_kmonad: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct YggclientConfig {
    identity: IdentityConfig,
    network: NetworkConfig,
    sync: SyncConfig,
    services: ServicesConfig,
}

#[derive(Default, Serialize, Deserialize)]
struct KeepLatestRule {
    glob: String,
    keep: u32,
}

#[derive(Default, Serialize, Deserialize)]
struct TargetConfig {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    host: String,
    #[serde(skip_serializing_if = "is_zero_u16")]
    port: u16,
    #[serde(skip_serializing_if = "String::is_empty")]
    share: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    base_path: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    path: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    username: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    password: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    username_env: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    password_env: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    domain: String,
}

#[derive(Default, Serialize, Deserialize)]
struct JobConfig {
    name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(rename = "type")]
    kind: String,
    local: String,
    remote: String,
    #[serde(skip_serializing_if = "is_zero_u32")]
    timeout_seconds: u32,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    include: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    exclude: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    filter_rules: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    local_retention_days: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    keep_latest: Vec<KeepLatestRule>,
    #[serde(skip_serializing_if = "String::is_empty")]
    state_file: String,
}

#[derive(Default, Serialize, Deserialize)]
struct YggsyncConfig {
    lock_file: String,
    worktree_state_dir: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    default_flags: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    targets: Vec<TargetConfig>,
    jobs: Vec<JobConfig>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    rclone_binary: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    rclone_config: String,
}

#[derive(Default)]
struct CliOptions {
    workspace_root: String,
    repo_base: String,
    bootstrap: bool,
    write_defaults: bool,
    force: bool,
    build_iso: bool,
    smoke: bool,
    profile: String,
    sets: Vec<String>,
    skip_smoke: bool,
    with_qemu: bool,
    help: bool,
    version: bool,
    list_actions: bool,
    list_fields: bool,
    fetch_yggsync: bool,
    render_runtime_config: bool,
    apply_client_stack: bool,
    install_desktop_yggsync: bool,
    setup_android_sync: bool,
}

struct SaveReport {
    written: Vec<PathBuf>,
    skipped: Vec<PathBuf>,
}

struct UiLayout {
    outer: Vec<Rect>,
    body: Vec<Rect>,
    right: Vec<Rect>,
}

#[derive(Clone, Copy)]
enum UiAction {
    Bootstrap,
    SaveLocal,
    RenderRuntimeConfig,
    FetchYggsync,
    ApplyClientStack,
    InstallDesktopYggsync,
    SetupAndroidSync,
    BuildIso,
    Smoke,
}

enum UiEvent {
    None,
    Quit,
    Action(UiAction),
}

struct ActionSpec {
    key: &'static str,
    cli: &'static str,
    description: &'static str,
}

struct App {
    platform: Platform,
    section: usize,
    field_index: usize,
    workspace: Vec<Field>,
    yggdrasil: Vec<Field>,
    yggclient: Vec<Field>,
    yggsync: Vec<Field>,
    status: String,
}

fn main() -> io::Result<()> {
    let opts = parse_cli().map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let platform = Platform::detect();

    if opts.version {
        println!("yggcli {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if opts.help {
        usage();
        return Ok(());
    }
    if opts.list_actions {
        list_actions(&platform);
        return Ok(());
    }
    if opts.list_fields {
        list_fields(&platform, &opts.workspace_root);
        return Ok(());
    }

    if (opts.build_iso || opts.smoke) && !platform.supports_host_builds() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "platform {} does not support yggdrasil build or smoke actions",
                platform.label()
            ),
        ));
    }

    if has_non_interactive_action(&opts) {
        return run_non_interactive(opts, platform);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let result = run_app(stdout);
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    result
}

impl Default for App {
    fn default() -> Self {
        Self::with_platform_and_workspace(Platform::detect(), default_workspace())
    }
}

impl App {
    fn with_platform_and_workspace(platform: Platform, root: String) -> Self {
        let root_str = root;
        let yggclient_fields = if platform.is_android {
            vec![
                Field::text(
                    "profile_name",
                    "android",
                    "Short profile label for this endpoint. This becomes PROFILE_NAME in the compatibility env file.",
                ),
                Field::text(
                    "user_name",
                    "termux",
                    "Human-facing endpoint owner. This usually matches the directory owner only loosely on Android.",
                ),
                Field::text(
                    "user_home",
                    "$HOME",
                    "Home path used in generated client metadata.",
                ),
                Field::text(
                    "ssh_host",
                    "example-host",
                    "Primary host or NAS reachable from this endpoint. This is operator metadata for yggclient workflows.",
                ),
                Field::text(
                    "ssh_user",
                    "alice",
                    "SSH login name used by other yggclient helper scripts.",
                ),
                Field::text(
                    "apt_http_proxy",
                    "",
                    "Optional apt proxy for Linux-oriented workflows. Usually blank on Android.",
                ),
                Field::text(
                    "apt_https_proxy",
                    "",
                    "Optional apt proxy for Linux-oriented workflows. Usually blank on Android.",
                ),
                Field::boolean(
                    "enable_yggsync",
                    true,
                    "Whether this endpoint should manage yggsync at all.",
                ),
                Field::text(
                    "yggsync_repo",
                    "https://github.com/yggdrasilhq/yggsync",
                    "Public source or release repo for yggsync.",
                ),
                Field::text(
                    "yggsync_config",
                    "~/.config/ygg_sync.toml",
                    "Runtime yggsync config path used by this endpoint.",
                ),
                Field::text(
                    "samba_host",
                    "nas.lan",
                    "NAS hostname or IP for native SMB sync jobs.",
                ),
                Field::text(
                    "samba_share",
                    "data",
                    "SMB share name, usually data.",
                ),
                Field::text(
                    "samba_user",
                    "alice",
                    "Path owner segment used in NAS remote paths.",
                ),
                Field::text(
                    "samba_username",
                    "alice",
                    "SMB login account. This can differ from samba_user.",
                ),
                Field::text(
                    "samba_password_env",
                    "SAMBA_PASSWORD",
                    "Environment variable that supplies the SMB password at runtime.",
                ),
                Field::text(
                    "screencasts_remote",
                    "immich02/alice/desktop/Screencasts",
                    "Desktop screencast remote path. Usually irrelevant on Android but kept for shared schema symmetry.",
                ),
                Field::boolean(
                    "use_mounted_nas",
                    false,
                    "Desktop-only mode. Keep false on Android. When true, yggcli writes local-path targets instead of SMB targets.",
                ),
                Field::text(
                    "mounted_nas_root",
                    "/mnt/nas/data",
                    "Desktop-only mount root used when use_mounted_nas is true.",
                ),
                Field::boolean(
                    "install_desktop_timer",
                    false,
                    "Desktop-only service installation toggle. Keep false on Android.",
                ),
                Field::boolean(
                    "install_shift_sync",
                    false,
                    "Whether shift-sync helper units should be installed by legacy service workflows.",
                ),
                Field::boolean(
                    "install_kmonad",
                    false,
                    "Whether kmonad helper units should be installed by legacy service workflows.",
                ),
            ]
        } else {
            vec![
                Field::text(
                    "profile_name",
                    "laptop",
                    "Short profile label for this endpoint. This becomes PROFILE_NAME in the compatibility env file.",
                ),
                Field::text(
                    "user_name",
                    "alice",
                    "Human-facing endpoint owner.",
                ),
                Field::text(
                    "user_home",
                    "/home/alice",
                    "Home path used in generated client metadata.",
                ),
                Field::text(
                    "ssh_host",
                    "example-host",
                    "Primary host or NAS reachable from this endpoint. This is operator metadata for yggclient workflows.",
                ),
                Field::text(
                    "ssh_user",
                    "alice",
                    "SSH login name used by other yggclient helper scripts.",
                ),
                Field::text(
                    "apt_http_proxy",
                    "",
                    "Optional apt proxy for workstation or build-host workflows.",
                ),
                Field::text(
                    "apt_https_proxy",
                    "",
                    "Optional apt proxy for workstation or build-host workflows.",
                ),
                Field::boolean(
                    "enable_yggsync",
                    true,
                    "Whether this endpoint should manage yggsync at all.",
                ),
                Field::text(
                    "yggsync_repo",
                    "https://github.com/yggdrasilhq/yggsync",
                    "Public source or release repo for yggsync.",
                ),
                Field::text(
                    "yggsync_config",
                    "~/.config/ygg_sync.toml",
                    "Runtime yggsync config path used by this endpoint.",
                ),
                Field::text(
                    "samba_host",
                    "nas.lan",
                    "NAS hostname or IP for native SMB sync jobs.",
                ),
                Field::text(
                    "samba_share",
                    "data",
                    "SMB share name, usually data.",
                ),
                Field::text(
                    "samba_user",
                    "alice",
                    "Path owner segment used in NAS remote paths.",
                ),
                Field::text(
                    "samba_username",
                    "alice",
                    "SMB login account. This can differ from samba_user.",
                ),
                Field::text(
                    "samba_password_env",
                    "SAMBA_PASSWORD",
                    "Environment variable that supplies the SMB password at runtime.",
                ),
                Field::text(
                    "screencasts_remote",
                    "immich02/alice/desktop/Screencasts",
                    "Relative remote path for desktop screencasts.",
                ),
                Field::boolean(
                    "use_mounted_nas",
                    false,
                    "When true, yggcli writes local-path targets instead of SMB targets for desktop runtime config.",
                ),
                Field::text(
                    "mounted_nas_root",
                    "/mnt/nas/data",
                    "Mounted NAS root used when use_mounted_nas is true.",
                ),
                Field::boolean(
                    "install_desktop_timer",
                    true,
                    "Whether the desktop yggsync user service and timer should be installed during apply-client-stack.",
                ),
                Field::boolean(
                    "install_shift_sync",
                    false,
                    "Whether shift-sync helper units should be installed by legacy service workflows.",
                ),
                Field::boolean(
                    "install_kmonad",
                    false,
                    "Whether kmonad helper units should be installed by legacy service workflows.",
                ),
            ]
        };

        let yggsync_fields = if platform.is_android {
            vec![
                Field::text(
                    "lock_file",
                    "~/.local/state/yggsync.lock",
                    "Global lock path that prevents overlapping runs.",
                ),
                Field::text(
                    "worktree_state_dir",
                    "~/.local/state/yggsync/worktrees",
                    "Directory where worktree state files are stored.",
                ),
                Field::text(
                    "notes_local",
                    "~/storage/shared/Documents/obsidian",
                    "Local Obsidian worktree path on the phone.",
                ),
                Field::text(
                    "notes_remote_path",
                    "smbfs/alice/obsidian",
                    "Remote path relative to the selected target. Do not include the target prefix.",
                ),
                Field::text(
                    "camera_local",
                    "~/storage/shared/DCIM",
                    "Local camera-roll or DCIM path.",
                ),
                Field::text(
                    "camera_remote_path",
                    "immich/alice/DCIM",
                    "Remote path for retained camera media uploads.",
                ),
                Field::text(
                    "screenshots_local",
                    "~/storage/shared/Pictures/Screenshots",
                    "Local screenshots path.",
                ),
                Field::text(
                    "screenshots_remote_path",
                    "immich02/alice/android/Screenshots",
                    "Remote path for screenshot uploads.",
                ),
                Field::text(
                    "screencasts_local",
                    "",
                    "Usually blank on Android. Kept so the same schema can drive both desktop and phone endpoints.",
                ),
            ]
        } else {
            vec![
                Field::text(
                    "lock_file",
                    "~/.local/state/yggsync.lock",
                    "Global lock path that prevents overlapping runs.",
                ),
                Field::text(
                    "worktree_state_dir",
                    "~/.local/state/yggsync/worktrees",
                    "Directory where worktree state files are stored.",
                ),
                Field::text(
                    "notes_local",
                    "~/Documents/notes",
                    "Local notes or Obsidian worktree path on the desktop.",
                ),
                Field::text(
                    "notes_remote_path",
                    "smbfs/alice/notes",
                    "Remote path relative to the selected target. Do not include the target prefix.",
                ),
                Field::text(
                    "camera_local",
                    "~/Pictures/Camera",
                    "Optional local media directory if this desktop also archives camera or imported media.",
                ),
                Field::text(
                    "camera_remote_path",
                    "immich/alice/media/camera-roll",
                    "Remote path for retained media uploads.",
                ),
                Field::text(
                    "screenshots_local",
                    "~/Pictures/Screenshots",
                    "Local screenshots path.",
                ),
                Field::text(
                    "screenshots_remote_path",
                    "immich02/alice/desktop/Screenshots",
                    "Remote path for screenshot uploads.",
                ),
                Field::text(
                    "screencasts_local",
                    "~/Screencasts",
                    "Local screencasts directory for desktop uploads.",
                ),
            ]
        };

        let mut app = Self {
            platform,
            section: 0,
            field_index: 0,
            workspace: vec![
                Field::text(
                    "workspace_root",
                    root_str,
                    "Workspace root containing the yggdrasil ecosystem repositories.",
                ),
                Field::text(
                    "repo_base",
                    DEFAULT_REPO_BASE,
                    "Base URL used when bootstrapping repos. Example: https://github.com/yggdrasilhq",
                ),
                Field::text(
                    "docs_repo",
                    "yggdocs",
                    "Repository name for ecosystem docs within the workspace.",
                ),
                Field::text(
                    "server_repo",
                    "yggdrasil",
                    "Repository name for the server build tree within the workspace.",
                ),
                Field::text(
                    "client_repo",
                    "yggclient",
                    "Repository name for endpoint automation within the workspace.",
                ),
                Field::text(
                    "sync_repo",
                    "yggsync",
                    "Repository name for the sync engine within the workspace.",
                ),
            ],
            yggdrasil: vec![
                Field::text(
                    "build_profile",
                    "both",
                    "Profile for mkconfig and smoke flows: server, kde, or both.",
                ),
                Field::boolean(
                    "enable_qemu_smoke",
                    false,
                    "Whether explicit smoke runs should include QEMU boot checks.",
                ),
                Field::boolean(
                    "with_nvidia",
                    false,
                    "Whether the server build should include the NVIDIA path.",
                ),
                Field::boolean(
                    "with_lts",
                    false,
                    "Whether to use the compatibility-pinned kernel path instead of the normal sid kernel path.",
                ),
                Field::text(
                    "setup_mode",
                    "recommended",
                    "High-level server setup intent used by yggdrasil config generation.",
                ),
                Field::text(
                    "apt_proxy_mode",
                    "off",
                    "Recommended first host setting: off. Move to explicit only after the apt-proxy container exists.",
                ),
                Field::text(
                    "infisical_boot_mode",
                    "disabled",
                    "Recommended first host setting: disabled. Move to container only after you adopt that pattern intentionally.",
                ),
                Field::text(
                    "infisical_container_name",
                    "infisical",
                    "Container name used when infisical_boot_mode is container.",
                ),
                Field::boolean(
                    "embed_ssh_keys",
                    true,
                    "Whether to embed authorized SSH keys into the generated host config.",
                ),
                Field::text(
                    "ssh_authorized_keys_file",
                    "/root/.ssh/authorized_keys",
                    "Path to the authorized_keys file used when embed_ssh_keys is true.",
                ),
                Field::text(
                    "hostname",
                    "yggdrasil",
                    "Host name for the generated server system.",
                ),
                Field::text(
                    "net_mode",
                    "dhcp",
                    "Network mode for the host. DHCP is the conservative default.",
                ),
                Field::text(
                    "lxc_parent_if",
                    "eno1",
                    "Parent interface for LXC networking and macvlan-derived flows.",
                ),
                Field::text(
                    "macvlan_cidr",
                    "10.10.0.250/24",
                    "MACVLAN address used when the network mode needs it.",
                ),
                Field::text(
                    "macvlan_route",
                    "10.10.0.0/24",
                    "MACVLAN route used when the network mode needs it.",
                ),
                Field::text(
                    "static_iface",
                    "eno1",
                    "Interface name for static networking mode.",
                ),
                Field::text(
                    "static_ip",
                    "",
                    "Static IP for static networking mode.",
                ),
                Field::text(
                    "static_gateway",
                    "",
                    "Static gateway for static networking mode.",
                ),
                Field::text(
                    "static_dns",
                    "1.1.1.1 8.8.8.8",
                    "Static DNS servers for static networking mode.",
                ),
                Field::text(
                    "apt_http_proxy",
                    "",
                    "Optional HTTP proxy for apt on the host build path.",
                ),
                Field::text(
                    "apt_https_proxy",
                    "",
                    "Optional HTTPS proxy for apt on the host build path.",
                ),
                Field::text(
                    "apt_proxy_bypass_host",
                    "",
                    "Optional apt-proxy bypass host for split network setups.",
                ),
            ],
            yggclient: yggclient_fields,
            yggsync: yggsync_fields,
            status: "Ctrl-B bootstrap | Ctrl-S save | Ctrl-R render runtime config | Ctrl-F fetch yggsync | Ctrl-Y apply client stack | q quit".into(),
        };

        let root = app.get(&app.workspace, "workspace_root");
        app.load_existing_configs(&root);
        app
    }

    fn load_existing_configs(&mut self, root: &str) {
        let root = PathBuf::from(root);
        if self.platform.supports_host_builds() {
            self.load_yggdrasil_config(&root.join("yggdrasil/ygg.local.toml"));
        }
        self.load_yggclient_config(&root.join("yggclient/yggclient.local.toml"));
        self.load_yggsync_config(&root.join("yggsync/ygg_sync.local.toml"));
    }

    fn load_yggdrasil_config(&mut self, path: &Path) {
        let Ok(raw) = fs::read_to_string(path) else {
            return;
        };
        let Ok(cfg) = toml::from_str::<YggdrasilConfig>(&raw) else {
            return;
        };
        Self::set_field(&mut self.yggdrasil, "build_profile", cfg.build_profile);
        Self::set_bool_field(
            &mut self.yggdrasil,
            "enable_qemu_smoke",
            cfg.enable_qemu_smoke,
        );
        Self::set_bool_field(&mut self.yggdrasil, "with_nvidia", cfg.with_nvidia);
        Self::set_bool_field(&mut self.yggdrasil, "with_lts", cfg.with_lts);
        Self::set_field(&mut self.yggdrasil, "setup_mode", cfg.setup_mode);
        Self::set_field(&mut self.yggdrasil, "apt_proxy_mode", cfg.apt_proxy_mode);
        Self::set_field(
            &mut self.yggdrasil,
            "infisical_boot_mode",
            cfg.infisical_boot_mode,
        );
        Self::set_field(
            &mut self.yggdrasil,
            "infisical_container_name",
            cfg.infisical_container_name,
        );
        Self::set_bool_field(&mut self.yggdrasil, "embed_ssh_keys", cfg.embed_ssh_keys);
        Self::set_field(
            &mut self.yggdrasil,
            "ssh_authorized_keys_file",
            cfg.ssh_authorized_keys_file,
        );
        Self::set_field(&mut self.yggdrasil, "hostname", cfg.hostname);
        Self::set_field(&mut self.yggdrasil, "net_mode", cfg.net_mode);
        Self::set_field(&mut self.yggdrasil, "lxc_parent_if", cfg.lxc_parent_if);
        Self::set_field(&mut self.yggdrasil, "macvlan_cidr", cfg.macvlan_cidr);
        Self::set_field(&mut self.yggdrasil, "macvlan_route", cfg.macvlan_route);
        Self::set_field(&mut self.yggdrasil, "static_iface", cfg.static_iface);
        Self::set_field(&mut self.yggdrasil, "static_ip", cfg.static_ip);
        Self::set_field(&mut self.yggdrasil, "static_gateway", cfg.static_gateway);
        Self::set_field(&mut self.yggdrasil, "static_dns", cfg.static_dns);
        Self::set_field(&mut self.yggdrasil, "apt_http_proxy", cfg.apt_http_proxy);
        Self::set_field(&mut self.yggdrasil, "apt_https_proxy", cfg.apt_https_proxy);
        Self::set_field(
            &mut self.yggdrasil,
            "apt_proxy_bypass_host",
            cfg.apt_proxy_bypass_host,
        );
    }

    fn load_yggclient_config(&mut self, path: &Path) {
        let Ok(raw) = fs::read_to_string(path) else {
            return;
        };
        let Ok(cfg) = toml::from_str::<YggclientConfig>(&raw) else {
            return;
        };
        Self::set_field(
            &mut self.yggclient,
            "profile_name",
            cfg.identity.profile_name,
        );
        Self::set_field(&mut self.yggclient, "user_name", cfg.identity.user_name);
        Self::set_field(&mut self.yggclient, "user_home", cfg.identity.user_home);
        Self::set_field(&mut self.yggclient, "ssh_host", cfg.network.ssh_host);
        Self::set_field(&mut self.yggclient, "ssh_user", cfg.network.ssh_user);
        Self::set_field(
            &mut self.yggclient,
            "apt_http_proxy",
            cfg.network.apt_http_proxy,
        );
        Self::set_field(
            &mut self.yggclient,
            "apt_https_proxy",
            cfg.network.apt_https_proxy,
        );
        Self::set_bool_field(
            &mut self.yggclient,
            "enable_yggsync",
            cfg.sync.enable_yggsync,
        );
        Self::set_field(&mut self.yggclient, "yggsync_repo", cfg.sync.yggsync_repo);
        Self::set_field(
            &mut self.yggclient,
            "yggsync_config",
            cfg.sync.yggsync_config,
        );
        Self::set_field(&mut self.yggclient, "samba_host", cfg.sync.samba_host);
        Self::set_field(&mut self.yggclient, "samba_share", cfg.sync.samba_share);
        Self::set_field(&mut self.yggclient, "samba_user", cfg.sync.samba_user);
        Self::set_field(
            &mut self.yggclient,
            "samba_username",
            cfg.sync.samba_username,
        );
        Self::set_field(
            &mut self.yggclient,
            "samba_password_env",
            cfg.sync.samba_password_env,
        );
        Self::set_field(
            &mut self.yggclient,
            "screencasts_remote",
            cfg.sync.screencasts_remote,
        );
        Self::set_bool_field(
            &mut self.yggclient,
            "use_mounted_nas",
            cfg.sync.use_mounted_nas,
        );
        Self::set_field(
            &mut self.yggclient,
            "mounted_nas_root",
            cfg.sync.mounted_nas_root,
        );
        Self::set_bool_field(
            &mut self.yggclient,
            "install_desktop_timer",
            cfg.services.install_desktop_timer,
        );
        Self::set_bool_field(
            &mut self.yggclient,
            "install_shift_sync",
            cfg.services.install_shift_sync,
        );
        Self::set_bool_field(
            &mut self.yggclient,
            "install_kmonad",
            cfg.services.install_kmonad,
        );
    }

    fn load_yggsync_config(&mut self, path: &Path) {
        let Ok(raw) = fs::read_to_string(path) else {
            return;
        };
        let Ok(cfg) = toml::from_str::<YggsyncConfig>(&raw) else {
            return;
        };

        Self::set_field(&mut self.yggsync, "lock_file", cfg.lock_file);
        Self::set_field(
            &mut self.yggsync,
            "worktree_state_dir",
            cfg.worktree_state_dir,
        );

        for target in cfg.targets {
            if target.kind == "local" && !target.path.is_empty() {
                Self::set_bool_field(&mut self.yggclient, "use_mounted_nas", true);
                Self::set_field(&mut self.yggclient, "mounted_nas_root", target.path);
                break;
            }
        }

        for job in cfg.jobs {
            match job.name.as_str() {
                "notes" | "obsidian" => {
                    Self::set_field(&mut self.yggsync, "notes_local", job.local);
                    Self::set_field(
                        &mut self.yggsync,
                        "notes_remote_path",
                        strip_target_prefix(&job.remote),
                    );
                }
                "camera-roll" | "dcim" => {
                    Self::set_field(&mut self.yggsync, "camera_local", job.local);
                    Self::set_field(
                        &mut self.yggsync,
                        "camera_remote_path",
                        strip_target_prefix(&job.remote),
                    );
                }
                "screenshots" | "screenshots-desktop" => {
                    Self::set_field(&mut self.yggsync, "screenshots_local", job.local);
                    Self::set_field(
                        &mut self.yggsync,
                        "screenshots_remote_path",
                        strip_target_prefix(&job.remote),
                    );
                }
                "screencasts" => {
                    Self::set_field(&mut self.yggsync, "screencasts_local", job.local);
                    Self::set_field(
                        &mut self.yggclient,
                        "screencasts_remote",
                        strip_target_prefix(&job.remote),
                    );
                }
                _ => {}
            }
        }
    }

    fn section(&self) -> Section {
        Section::all()[self.section]
    }

    fn fields(&self) -> &Vec<Field> {
        match self.section() {
            Section::Workspace => &self.workspace,
            Section::Yggdrasil => &self.yggdrasil,
            Section::Yggclient => &self.yggclient,
            Section::Yggsync => &self.yggsync,
        }
    }

    fn fields_mut(&mut self) -> &mut Vec<Field> {
        match self.section() {
            Section::Workspace => &mut self.workspace,
            Section::Yggdrasil => &mut self.yggdrasil,
            Section::Yggclient => &mut self.yggclient,
            Section::Yggsync => &mut self.yggsync,
        }
    }

    fn current_field(&self) -> &Field {
        let idx = self.field_index.min(self.fields().len().saturating_sub(1));
        &self.fields()[idx]
    }

    fn current_mut(&mut self) -> &mut Field {
        let idx = self.field_index.min(self.fields().len().saturating_sub(1));
        self.field_index = idx;
        &mut self.fields_mut()[idx]
    }

    fn next_section(&mut self) {
        self.section = (self.section + 1) % Section::all().len();
        self.field_index = 0;
    }

    fn previous_section(&mut self) {
        self.section = (self.section + Section::all().len() - 1) % Section::all().len();
        self.field_index = 0;
    }

    fn workspace_root(&self) -> PathBuf {
        PathBuf::from(self.get(&self.workspace, "workspace_root"))
    }

    fn server_repo_path(&self) -> PathBuf {
        self.workspace_root()
            .join(self.get(&self.workspace, "server_repo"))
    }

    fn client_repo_path(&self) -> PathBuf {
        self.workspace_root()
            .join(self.get(&self.workspace, "client_repo"))
    }

    fn sync_repo_path(&self) -> PathBuf {
        self.workspace_root()
            .join(self.get(&self.workspace, "sync_repo"))
    }

    fn repo_base(&self) -> String {
        self.get(&self.workspace, "repo_base")
    }

    fn build_yggdrasil_config(&self) -> YggdrasilConfig {
        YggdrasilConfig {
            build_profile: self.get(&self.yggdrasil, "build_profile"),
            enable_qemu_smoke: self.get_bool(&self.yggdrasil, "enable_qemu_smoke"),
            with_nvidia: self.get_bool(&self.yggdrasil, "with_nvidia"),
            with_lts: self.get_bool(&self.yggdrasil, "with_lts"),
            setup_mode: self.get(&self.yggdrasil, "setup_mode"),
            apt_proxy_mode: self.get(&self.yggdrasil, "apt_proxy_mode"),
            infisical_boot_mode: self.get(&self.yggdrasil, "infisical_boot_mode"),
            infisical_container_name: self.get(&self.yggdrasil, "infisical_container_name"),
            embed_ssh_keys: self.get_bool(&self.yggdrasil, "embed_ssh_keys"),
            ssh_authorized_keys_file: self.get(&self.yggdrasil, "ssh_authorized_keys_file"),
            hostname: self.get(&self.yggdrasil, "hostname"),
            net_mode: self.get(&self.yggdrasil, "net_mode"),
            lxc_parent_if: self.get(&self.yggdrasil, "lxc_parent_if"),
            macvlan_cidr: self.get(&self.yggdrasil, "macvlan_cidr"),
            macvlan_route: self.get(&self.yggdrasil, "macvlan_route"),
            static_iface: self.get(&self.yggdrasil, "static_iface"),
            static_ip: self.get(&self.yggdrasil, "static_ip"),
            static_gateway: self.get(&self.yggdrasil, "static_gateway"),
            static_dns: self.get(&self.yggdrasil, "static_dns"),
            apt_http_proxy: self.get(&self.yggdrasil, "apt_http_proxy"),
            apt_https_proxy: self.get(&self.yggdrasil, "apt_https_proxy"),
            apt_proxy_bypass_host: self.get(&self.yggdrasil, "apt_proxy_bypass_host"),
        }
    }

    fn build_yggclient_config(&self) -> YggclientConfig {
        YggclientConfig {
            identity: IdentityConfig {
                profile_name: self.get(&self.yggclient, "profile_name"),
                user_name: self.get(&self.yggclient, "user_name"),
                user_home: self.get(&self.yggclient, "user_home"),
            },
            network: NetworkConfig {
                ssh_host: self.get(&self.yggclient, "ssh_host"),
                ssh_user: self.get(&self.yggclient, "ssh_user"),
                apt_http_proxy: self.get(&self.yggclient, "apt_http_proxy"),
                apt_https_proxy: self.get(&self.yggclient, "apt_https_proxy"),
            },
            sync: SyncConfig {
                enable_yggsync: self.get_bool(&self.yggclient, "enable_yggsync"),
                yggsync_repo: self.get(&self.yggclient, "yggsync_repo"),
                yggsync_config: self.get(&self.yggclient, "yggsync_config"),
                samba_host: self.get(&self.yggclient, "samba_host"),
                samba_share: self.get(&self.yggclient, "samba_share"),
                samba_user: self.get(&self.yggclient, "samba_user"),
                samba_username: self.get(&self.yggclient, "samba_username"),
                samba_password_env: self.get(&self.yggclient, "samba_password_env"),
                screencasts_remote: self.get(&self.yggclient, "screencasts_remote"),
                use_mounted_nas: self.get_bool(&self.yggclient, "use_mounted_nas"),
                mounted_nas_root: self.get(&self.yggclient, "mounted_nas_root"),
            },
            services: ServicesConfig {
                install_desktop_timer: self.get_bool(&self.yggclient, "install_desktop_timer"),
                install_shift_sync: self.get_bool(&self.yggclient, "install_shift_sync"),
                install_kmonad: self.get_bool(&self.yggclient, "install_kmonad"),
            },
        }
    }

    fn build_yggsync_config(&self) -> YggsyncConfig {
        let client_cfg = self.build_yggclient_config();
        let use_mounted = client_cfg.sync.use_mounted_nas && !self.platform.is_android;
        let target_name = if use_mounted { "mounted" } else { "nas" };

        let targets = if use_mounted {
            vec![TargetConfig {
                name: target_name.into(),
                kind: "local".into(),
                path: client_cfg.sync.mounted_nas_root,
                ..Default::default()
            }]
        } else {
            vec![TargetConfig {
                name: target_name.into(),
                kind: "smb".into(),
                host: client_cfg.sync.samba_host,
                share: client_cfg.sync.samba_share,
                username: client_cfg.sync.samba_username,
                password_env: client_cfg.sync.samba_password_env,
                ..Default::default()
            }]
        };

        let mut jobs = vec![JobConfig {
            name: if self.platform.is_android {
                "obsidian".into()
            } else {
                "notes".into()
            },
            description: if self.platform.is_android {
                "Local Obsidian worktree against the NAS repository".into()
            } else {
                "Local notes or Obsidian worktree against the NAS repository".into()
            },
            kind: "worktree".into(),
            local: self.get(&self.yggsync, "notes_local"),
            remote: remote_ref(target_name, &self.get(&self.yggsync, "notes_remote_path")),
            timeout_seconds: 900,
            filter_rules: default_worktree_filters(),
            ..Default::default()
        }];

        let camera_local = self.get(&self.yggsync, "camera_local");
        if !camera_local.trim().is_empty() {
            jobs.push(JobConfig {
                name: if self.platform.is_android {
                    "dcim".into()
                } else {
                    "camera-roll".into()
                },
                description: "Retained media upload".into(),
                kind: "retained_copy".into(),
                local: camera_local,
                remote: remote_ref(target_name, &self.get(&self.yggsync, "camera_remote_path")),
                timeout_seconds: 1800,
                local_retention_days: Some(31),
                ..Default::default()
            });
        }

        let screenshot_kind = if self.platform.is_android {
            "retained_copy"
        } else {
            "copy"
        };
        jobs.push(JobConfig {
            name: if self.platform.is_android {
                "screenshots".into()
            } else {
                "screenshots-desktop".into()
            },
            description: "Screenshot upload job".into(),
            kind: screenshot_kind.into(),
            local: self.get(&self.yggsync, "screenshots_local"),
            remote: remote_ref(
                target_name,
                &self.get(&self.yggsync, "screenshots_remote_path"),
            ),
            timeout_seconds: 900,
            local_retention_days: if self.platform.is_android {
                Some(31)
            } else {
                None
            },
            ..Default::default()
        });

        if !self.platform.is_android {
            let screencasts_local = self.get(&self.yggsync, "screencasts_local");
            if !screencasts_local.trim().is_empty() {
                jobs.push(JobConfig {
                    name: "screencasts".into(),
                    description: "Desktop screencast upload".into(),
                    kind: "copy".into(),
                    local: screencasts_local,
                    remote: remote_ref(
                        target_name,
                        &self.get(&self.yggclient, "screencasts_remote"),
                    ),
                    timeout_seconds: 900,
                    ..Default::default()
                });
            }
        }

        YggsyncConfig {
            lock_file: self.get(&self.yggsync, "lock_file"),
            worktree_state_dir: self.get(&self.yggsync, "worktree_state_dir"),
            default_flags: vec!["--use-json-log".into(), "--stats=120s".into()],
            targets,
            jobs,
            rclone_binary: String::new(),
            rclone_config: String::new(),
        }
    }

    fn save_local_configs(&self, force: bool) -> io::Result<SaveReport> {
        let server_repo = self.server_repo_path();
        let client_repo = self.client_repo_path();
        let sync_repo = self.sync_repo_path();

        let yggdrasil = self.build_yggdrasil_config();
        let yggclient = self.build_yggclient_config();
        let yggsync = self.build_yggsync_config();

        let mut report = SaveReport {
            written: Vec::new(),
            skipped: Vec::new(),
        };

        if self.platform.supports_host_builds() && server_repo.exists() {
            write_file(
                &server_repo.join("ygg.local.toml"),
                &toml::to_string_pretty(&yggdrasil).unwrap(),
                force,
                &mut report,
            )?;
        } else {
            report.skipped.push(server_repo.join("ygg.local.toml"));
        }

        if client_repo.exists() {
            write_file(
                &client_repo.join("yggclient.local.toml"),
                &toml::to_string_pretty(&yggclient).unwrap(),
                force,
                &mut report,
            )?;
            write_file(
                &client_repo.join("config/profiles.local.env"),
                &render_client_env(&yggclient),
                force,
                &mut report,
            )?;
        } else {
            report
                .skipped
                .push(client_repo.join("yggclient.local.toml"));
            report
                .skipped
                .push(client_repo.join("config/profiles.local.env"));
        }

        if sync_repo.exists() {
            write_file(
                &sync_repo.join("ygg_sync.local.toml"),
                &toml::to_string_pretty(&yggsync).unwrap(),
                force,
                &mut report,
            )?;
        } else {
            report.skipped.push(sync_repo.join("ygg_sync.local.toml"));
        }

        Ok(report)
    }

    fn write_runtime_config(&self, force: bool) -> io::Result<PathBuf> {
        let runtime_path = expand_tilde(&self.get(&self.yggclient, "yggsync_config"));
        let runtime_cfg = self.build_yggsync_config();
        let mut report = SaveReport {
            written: Vec::new(),
            skipped: Vec::new(),
        };
        write_file(
            &runtime_path,
            &toml::to_string_pretty(&runtime_cfg).unwrap(),
            force,
            &mut report,
        )?;
        Ok(runtime_path)
    }

    fn apply_override(&mut self, spec: &str) -> Result<(), String> {
        let (path, value) = spec
            .split_once('=')
            .ok_or_else(|| format!("override must be section.key=value: {spec}"))?;
        let (section, key) = path
            .split_once('.')
            .ok_or_else(|| format!("override must be section.key=value: {spec}"))?;
        let fields = match section {
            "workspace" => &mut self.workspace,
            "yggdrasil" | "server" => &mut self.yggdrasil,
            "yggclient" | "client" => &mut self.yggclient,
            "yggsync" | "sync" => &mut self.yggsync,
            other => return Err(format!("unknown override section: {other}")),
        };
        let field = fields
            .iter_mut()
            .find(|f| f.label == key)
            .ok_or_else(|| format!("unknown override field: {section}.{key}"))?;
        if field.bool_field {
            let normalized = match value {
                "true" | "1" | "yes" | "on" => "true",
                "false" | "0" | "no" | "off" => "false",
                _ => {
                    return Err(format!(
                        "invalid boolean override for {section}.{key}: {value}"
                    ))
                }
            };
            field.value = normalized.into();
        } else {
            field.value = value.into();
        }
        Ok(())
    }

    fn get(&self, fields: &[Field], key: &str) -> String {
        fields
            .iter()
            .find(|f| f.label == key)
            .map(|f| f.value.clone())
            .unwrap_or_default()
    }

    fn get_bool(&self, fields: &[Field], key: &str) -> bool {
        fields
            .iter()
            .find(|f| f.label == key)
            .map(|f| f.as_bool())
            .unwrap_or(false)
    }

    fn set_field(fields: &mut [Field], key: &str, value: String) {
        if let Some(field) = fields.iter_mut().find(|f| f.label == key) {
            field.value = value;
        }
    }

    fn set_bool_field(fields: &mut [Field], key: &str, value: bool) {
        Self::set_field(fields, key, if value { "true" } else { "false" }.into());
    }
}

fn default_workspace() -> String {
    if let Some(home) = home_dir() {
        let candidate = home.join("gh");
        if candidate.exists() {
            return candidate.display().to_string();
        }
        return candidate.display().to_string();
    }
    DEFAULT_WORKSPACE_FALLBACK.into()
}

fn home_dir() -> Option<PathBuf> {
    env::var("HOME").ok().map(PathBuf::from)
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(stripped);
        }
    }
    if path == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

fn strip_target_prefix(remote: &str) -> String {
    if remote.starts_with('/') {
        return remote.to_string();
    }
    remote
        .split_once(':')
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| remote.to_string())
}

fn remote_ref(target: &str, relative: &str) -> String {
    if relative.starts_with('/') {
        relative.into()
    } else {
        format!("{target}:{relative}")
    }
}

fn default_worktree_filters() -> Vec<String> {
    vec![
        "- **/.obsidian/**".into(),
        "- **/.trash/**".into(),
        "- **/*.conflict*".into(),
        "- [A-Za-z0-9_][A-Za-z0-9_][A-Za-z0-9_][A-Za-z0-9_][A-Za-z0-9_][A-Za-z0-9_]~[A-Za-z0-9].*".into(),
        "- **/[A-Za-z0-9_][A-Za-z0-9_][A-Za-z0-9_][A-Za-z0-9_][A-Za-z0-9_][A-Za-z0-9_]~[A-Za-z0-9].*".into(),
    ]
}

fn render_client_env(cfg: &YggclientConfig) -> String {
    fn sq(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }

    format!(
        "# Generated by yggcli\nPROFILE_NAME={profile}\nUSER_NAME={user}\nUSER_HOME={home}\nSSH_HOST={host}\nSSH_USER={ssh_user}\nAPT_HTTP_PROXY={http}\nAPT_HTTPS_PROXY={https}\nYGGSYNC_REPO={repo}\nYGGSYNC_CONFIG={sync_cfg}\nSAMBA_HOST={samba_host}\nSAMBA_SHARE={samba_share}\nSAMBA_USER={samba_user}\nSAMBA_USERNAME={samba_username}\nSAMBA_PASSWORD_ENV={samba_password_env}\nSCREENCASTS_REMOTE={screencasts_remote}\nUSE_MOUNTED_NAS={use_mounted}\nMOUNTED_NAS_ROOT={mounted_root}\nENABLE_YGGSYNC={sync}\nINSTALL_DESKTOP_TIMER={timer}\nINSTALL_SHIFT_SYNC={shift}\nINSTALL_KMONAD={kmonad}\n",
        profile = sq(&cfg.identity.profile_name),
        user = sq(&cfg.identity.user_name),
        home = sq(&cfg.identity.user_home),
        host = sq(&cfg.network.ssh_host),
        ssh_user = sq(&cfg.network.ssh_user),
        http = sq(&cfg.network.apt_http_proxy),
        https = sq(&cfg.network.apt_https_proxy),
        repo = sq(&cfg.sync.yggsync_repo),
        sync_cfg = sq(&cfg.sync.yggsync_config),
        samba_host = sq(&cfg.sync.samba_host),
        samba_share = sq(&cfg.sync.samba_share),
        samba_user = sq(&cfg.sync.samba_user),
        samba_username = sq(&cfg.sync.samba_username),
        samba_password_env = sq(&cfg.sync.samba_password_env),
        screencasts_remote = sq(&cfg.sync.screencasts_remote),
        use_mounted = sq(if cfg.sync.use_mounted_nas { "1" } else { "0" }),
        mounted_root = sq(&cfg.sync.mounted_nas_root),
        sync = sq(if cfg.sync.enable_yggsync { "1" } else { "0" }),
        timer = sq(if cfg.services.install_desktop_timer { "1" } else { "0" }),
        shift = sq(if cfg.services.install_shift_sync { "1" } else { "0" }),
        kmonad = sq(if cfg.services.install_kmonad { "1" } else { "0" }),
    )
}

fn is_zero_u16(v: &u16) -> bool {
    *v == 0
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn write_file(path: &Path, contents: &str, force: bool, report: &mut SaveReport) -> io::Result<()> {
    if path.exists() && !force {
        report.skipped.push(path.to_path_buf());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
    report.written.push(path.to_path_buf());
    Ok(())
}

fn action_specs(platform: &Platform) -> Vec<ActionSpec> {
    let mut specs = vec![
        ActionSpec {
            key: "Ctrl-B",
            cli: "--bootstrap",
            description: "Clone missing repos into the workspace",
        },
        ActionSpec {
            key: "Ctrl-S",
            cli: "--write-defaults",
            description: "Write yggdrasil, yggclient, and yggsync local config files",
        },
        ActionSpec {
            key: "Ctrl-R",
            cli: "--render-runtime-config",
            description: "Write the live ~/.config/ygg_sync.toml from the current fields",
        },
        ActionSpec {
            key: "Ctrl-F",
            cli: "--fetch-yggsync",
            description: "Fetch or build the yggsync binary for this endpoint",
        },
        ActionSpec {
            key: "Ctrl-Y",
            cli: "--apply-client-stack",
            description: "Save, render, fetch, and run the platform-appropriate yggclient/yggsync setup pipeline",
        },
    ];

    if platform.supports_host_builds() {
        specs.push(ActionSpec {
            key: "Ctrl-D",
            cli: "--install-desktop-yggsync",
            description: "Install the desktop yggsync user service and timer via yggclient",
        });
        specs.push(ActionSpec {
            key: "Ctrl-I",
            cli: "--build-iso",
            description: "Run the yggdrasil mkconfig build path",
        });
        specs.push(ActionSpec {
            key: "Ctrl-M",
            cli: "--smoke",
            description: "Run the yggdrasil smoke bench path",
        });
    } else {
        specs.push(ActionSpec {
            key: "Ctrl-A",
            cli: "--setup-android-sync",
            description: "Run the Termux setup flow for jobs, shortcuts, and boot integration",
        });
    }

    specs
}

fn list_actions(platform: &Platform) {
    println!("yggcli actions for platform {}:", platform.label());
    for spec in action_specs(platform) {
        println!("  {:<10} {:<28} {}", spec.key, spec.cli, spec.description);
    }
}

fn list_fields(platform: &Platform, workspace: &str) {
    let app = App::with_platform_and_workspace(platform.clone(), workspace.into());
    let sections = [
        ("workspace", &app.workspace),
        ("yggdrasil", &app.yggdrasil),
        ("yggclient", &app.yggclient),
        ("yggsync", &app.yggsync),
    ];
    println!("overrideable fields for platform {}:", platform.label());
    for (section, fields) in sections {
        println!("\n[{section}]");
        for field in fields {
            let kind = if field.bool_field { "bool" } else { "text" };
            println!(
                "  {:<36} {:<5} {}",
                format!("{section}.{}", field.label),
                kind,
                field.help
            );
        }
    }
}

fn usage() {
    let default_workspace = default_workspace();
    println!(
        "yggcli {version}\n\n\
Usage:\n\
  yggcli                         Launch interactive TUI\n\
  yggcli [options]               Run non-interactive workflow\n\n\
Discovery:\n\
  --help                         Show this help\n\
  --version                      Show the current version\n\
  --list-actions                 List supported actions for the current platform\n\
  --list-fields                  List overrideable fields for the current platform\n\n\
Core options:\n\
  --workspace PATH               Workspace root (default: {default_workspace})\n\
  --repo-base URL                Repo base for bootstrap clones (default: {repo_base})\n\
  --bootstrap                    Clone missing ecosystem repos\n\
  --write-defaults               Write local config files using current field values\n\
  --force                        Overwrite existing files that yggcli writes\n\
  --set section.key=value        Override one field before running actions (repeatable)\n\n\
Endpoint actions:\n\
  --render-runtime-config        Write the live yggsync config to ~/.config/ygg_sync.toml\n\
  --fetch-yggsync                Fetch or build the yggsync binary for this endpoint\n\
  --apply-client-stack           Save configs, render runtime config, fetch yggsync, and run the platform setup pipeline\n\
  --install-desktop-yggsync      Install the desktop yggsync service and timer (Linux only)\n\
  --setup-android-sync           Run the Android Termux setup flow (Android only)\n\n\
Server actions:\n\
  --build-iso                    Run yggdrasil build after config generation (Linux only)\n\
  --smoke                        Run smoke bench explicitly after build/config (Linux only)\n\
  --profile server|kde|both      Profile for build/smoke (default: both)\n\
  --skip-smoke                   Skip smoke inside mkconfig build step\n\
  --with-qemu                    Add QEMU/KVM smoke when running explicit smoke\n\n\
Examples:\n\
  yggcli --bootstrap --write-defaults\n\
  yggcli --list-fields\n\
  yggcli --workspace ~/gh --set yggclient.samba_host=nas.internal --set yggsync.notes_remote_path=smbfs/alice/obsidian --write-defaults --render-runtime-config\n\
  yggcli --workspace ~/gh --apply-client-stack\n\
  yggcli --workspace ~/gh --build-iso --profile server\n\n\
Guidance:\n\
  - First server build: keep apt_proxy_mode=off and infisical_boot_mode=disabled.\n\
  - For client bootstrap, set yggclient SMB fields first, then render or apply the client stack.\n\
  - use_mounted_nas=true is the desktop mounted-share path; keep it false on Android.\n\
  - Android/Termux hosts can configure yggclient and yggsync, but they do not build yggdrasil ISOs.\n\
  - The TUI exposes the same operations through hotkeys and documents them in the right-hand actions pane.\n",
        version = env!("CARGO_PKG_VERSION"),
        default_workspace = default_workspace,
        repo_base = DEFAULT_REPO_BASE,
    );
}

fn parse_cli() -> Result<CliOptions, String> {
    let mut opts = CliOptions {
        workspace_root: default_workspace(),
        repo_base: DEFAULT_REPO_BASE.into(),
        profile: "both".into(),
        ..Default::default()
    };

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace" => {
                opts.workspace_root = args.next().ok_or("--workspace requires a value")?
            }
            "--repo-base" => opts.repo_base = args.next().ok_or("--repo-base requires a value")?,
            "--bootstrap" => opts.bootstrap = true,
            "--write-defaults" => opts.write_defaults = true,
            "--force" => opts.force = true,
            "--build-iso" => opts.build_iso = true,
            "--smoke" => opts.smoke = true,
            "--profile" => opts.profile = args.next().ok_or("--profile requires a value")?,
            "--set" => opts
                .sets
                .push(args.next().ok_or("--set requires section.key=value")?),
            "--skip-smoke" => opts.skip_smoke = true,
            "--with-qemu" => opts.with_qemu = true,
            "--fetch-yggsync" => opts.fetch_yggsync = true,
            "--render-runtime-config" => opts.render_runtime_config = true,
            "--apply-client-stack" => opts.apply_client_stack = true,
            "--install-desktop-yggsync" => opts.install_desktop_yggsync = true,
            "--setup-android-sync" => opts.setup_android_sync = true,
            "--list-actions" => opts.list_actions = true,
            "--list-fields" => opts.list_fields = true,
            "--version" | "-V" => opts.version = true,
            "-h" | "--help" => opts.help = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    match opts.profile.as_str() {
        "server" | "kde" | "both" => {}
        _ => return Err(format!("invalid profile: {}", opts.profile)),
    }

    Ok(opts)
}

fn has_non_interactive_action(opts: &CliOptions) -> bool {
    opts.bootstrap
        || opts.write_defaults
        || opts.build_iso
        || opts.smoke
        || !opts.sets.is_empty()
        || opts.fetch_yggsync
        || opts.render_runtime_config
        || opts.apply_client_stack
        || opts.install_desktop_yggsync
        || opts.setup_android_sync
}

fn run_non_interactive(opts: CliOptions, platform: Platform) -> io::Result<()> {
    let mut app = App::with_platform_and_workspace(platform.clone(), opts.workspace_root.clone());
    App::set_field(&mut app.workspace, "repo_base", opts.repo_base.clone());

    if opts.bootstrap {
        bootstrap_repos(&app.workspace_root(), &app.repo_base(), &platform)?;
    }

    for spec in &opts.sets {
        app.apply_override(spec)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    }

    let should_write = opts.write_defaults
        || !opts.sets.is_empty()
        || opts.build_iso
        || opts.smoke
        || opts.render_runtime_config
        || opts.apply_client_stack
        || opts.install_desktop_yggsync
        || opts.setup_android_sync;

    if should_write {
        let should_force = opts.force || !opts.sets.is_empty() || opts.apply_client_stack;
        let report = app.save_local_configs(should_force)?;
        print_save_report("local configs", &report);
    }

    if opts.render_runtime_config || opts.apply_client_stack {
        let runtime_path = app.write_runtime_config(true)?;
        eprintln!("rendered runtime config: {}", runtime_path.display());
    }

    if opts.fetch_yggsync {
        run_fetch_yggsync(&app, &platform)?;
    }
    if opts.install_desktop_yggsync {
        install_desktop_yggsync(&app, &platform)?;
    }
    if opts.setup_android_sync {
        run_android_setup(&app, &platform)?;
    }
    if opts.apply_client_stack {
        apply_client_stack(&app, &platform)?;
    }
    if opts.build_iso {
        run_build(&app, opts.profile.as_str(), opts.skip_smoke, &platform)?;
    }
    if opts.smoke {
        run_smoke(&app, opts.profile.as_str(), opts.with_qemu, &platform)?;
    }

    Ok(())
}

fn print_save_report(label: &str, report: &SaveReport) {
    if !report.written.is_empty() {
        eprintln!(
            "{label} written: {}",
            report
                .written
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !report.skipped.is_empty() {
        eprintln!(
            "{label} skipped existing: {}",
            report
                .skipped
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

fn run_cmd(cmd: &mut Command) -> io::Result<()> {
    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("command failed with status {status}"),
        ))
    }
}

fn run_cmd_with_input(cmd: &mut Command, stdin_text: &str) -> io::Result<()> {
    cmd.stdin(Stdio::piped());
    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(stdin_text.as_bytes())?;
    }
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("command failed with status {status}"),
        ))
    }
}

fn run_capture(cmd: &mut Command) -> io::Result<String> {
    let output = cmd.output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

fn is_effective_root() -> io::Result<bool> {
    let output = Command::new("id").arg("-u").output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "0")
}

fn bootstrap_repos(workspace_root: &Path, repo_base: &str, platform: &Platform) -> io::Result<()> {
    fs::create_dir_all(workspace_root)?;
    for repo in platform.repos() {
        let target = workspace_root.join(repo);
        if target.exists() {
            continue;
        }
        let url = format!("{repo_base}/{repo}.git");
        run_cmd(Command::new("git").arg("clone").arg(url).arg(&target))?;
    }
    Ok(())
}

fn run_fetch_yggsync(app: &App, platform: &Platform) -> io::Result<()> {
    let client_repo = app.client_repo_path();
    let script = if platform.is_android {
        client_repo.join("android/scripts/fetch-yggsync.sh")
    } else {
        client_repo.join("scripts/yggsync/fetch-yggsync.sh")
    };
    if !script.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing fetch script: {}", script.display()),
        ));
    }

    let mut cmd = Command::new("bash");
    cmd.arg(&script).current_dir(&client_repo);
    if !platform.is_android {
        cmd.env("ALLOW_BUILD_FALLBACK", "1");
        cmd.env("SRC_DIR", app.sync_repo_path());
        cmd.env("YGGSYNC_REPO", app.get(&app.yggclient, "yggsync_repo"));
    }
    run_cmd(&mut cmd)
}

fn lookup_install_selection(script: &Path, wanted: &[&str]) -> io::Result<String> {
    let output = run_capture(Command::new("bash").arg(script).arg("--ls"))?;
    let mut found = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim_start();
        let Some((idx_part, rest)) = trimmed.split_once(')') else {
            continue;
        };
        let idx = idx_part.trim();
        if idx.is_empty() || !idx.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let name = rest.trim().split_whitespace().next().unwrap_or_default();
        if wanted.contains(&name) {
            found.push(idx.to_string());
        }
    }
    if found.len() != wanted.len() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "could not find all install selections in {}",
                script.display()
            ),
        ));
    }
    Ok(found.join(" "))
}

fn install_desktop_yggsync(app: &App, platform: &Platform) -> io::Result<()> {
    if !platform.supports_host_builds() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "desktop service installation is only available on non-Android platforms",
        ));
    }
    let script = app
        .client_repo_path()
        .join("scripts/install/install-service.sh");
    if !script.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing install script: {}", script.display()),
        ));
    }
    let selection = lookup_install_selection(
        &script,
        &["ygg-yggsync-desktop.service", "ygg-yggsync-desktop.timer"],
    )?;
    run_cmd(
        Command::new("bash")
            .arg(&script)
            .arg("-n")
            .arg("-y")
            .arg("-s")
            .arg(selection)
            .current_dir(app.client_repo_path()),
    )
}

fn run_android_setup(app: &App, platform: &Platform) -> io::Result<()> {
    if !platform.is_android {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "android sync setup is only available on Android or Termux platforms",
        ));
    }
    let script = app
        .client_repo_path()
        .join("android/scripts/setup-android-sync.sh");
    if !script.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing Android setup script: {}", script.display()),
        ));
    }
    let mut cmd = Command::new("bash");
    cmd.arg(&script)
        .current_dir(app.client_repo_path())
        .env("YGG_CLIENT_DIR", app.client_repo_path());
    run_cmd_with_input(&mut cmd, "n\n")
}

fn apply_client_stack(app: &App, platform: &Platform) -> io::Result<()> {
    if !app.get_bool(&app.yggclient, "enable_yggsync") {
        eprintln!("enable_yggsync=false; client stack apply is skipping yggsync actions.");
        return Ok(());
    }

    if platform.is_android {
        let bootstrap = app.client_repo_path().join("android/scripts/bootstrap.sh");
        let install = app.client_repo_path().join("android/scripts/install.sh");
        if bootstrap.exists() {
            run_cmd(
                Command::new("bash")
                    .arg(&bootstrap)
                    .current_dir(app.client_repo_path()),
            )?;
        }
        run_fetch_yggsync(app, platform)?;
        if install.exists() {
            run_cmd(
                Command::new("bash")
                    .arg(&install)
                    .current_dir(app.client_repo_path()),
            )?;
        }
        run_android_setup(app, platform)?;
        return Ok(());
    }

    run_fetch_yggsync(app, platform)?;
    if app.get_bool(&app.yggclient, "install_desktop_timer") {
        install_desktop_yggsync(app, platform)?;
    }
    Ok(())
}

fn run_build(app: &App, profile: &str, skip_smoke: bool, platform: &Platform) -> io::Result<()> {
    if !platform.supports_host_builds() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "yggdrasil ISO builds are disabled on Android/Termux hosts",
        ));
    }
    let repo = app.server_repo_path();
    let needs_sudo = !is_effective_root()?;
    let mut cmd = if needs_sudo {
        let mut cmd = Command::new("sudo");
        cmd.arg("-n").arg("./mkconfig.sh");
        cmd
    } else {
        Command::new("./mkconfig.sh")
    };
    cmd.current_dir(repo)
        .arg("--config")
        .arg("./ygg.local.toml")
        .arg("--profile")
        .arg(profile);
    if skip_smoke {
        cmd.arg("--skip-smoke");
    }
    run_cmd(&mut cmd)
}

fn run_smoke(app: &App, profile: &str, with_qemu: bool, platform: &Platform) -> io::Result<()> {
    if !platform.supports_host_builds() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "yggdrasil smoke benches are disabled on Android/Termux hosts",
        ));
    }
    let repo = app.server_repo_path();
    let mut cmd = Command::new("./tests/smoke/run.sh");
    cmd.current_dir(repo)
        .arg("--profile")
        .arg(profile)
        .arg("--require-artifacts")
        .arg("--with-iso-rootfs")
        .arg("--artifacts-dir")
        .arg("./artifacts")
        .arg("--server-iso")
        .arg("./artifacts/server-latest.iso")
        .arg("--kde-iso")
        .arg("./artifacts/kde-latest.iso");
    if with_qemu {
        cmd.arg("--with-qemu-boot");
    }
    run_cmd(&mut cmd)
}

fn run_app(stdout: Stdout) -> io::Result<()> {
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::default();

    loop {
        terminal.draw(|frame| draw(frame, &app))?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) => match handle_key(&mut app, key) {
                UiEvent::Quit => break,
                UiEvent::Action(action) => {
                    run_tui_action(&mut terminal, &mut app, action)?;
                }
                UiEvent::None => {}
            },
            Event::Mouse(mouse) => handle_mouse(&mut app, mouse)?,
            _ => {}
        }
    }

    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> UiEvent {
    match key.code {
        KeyCode::Char('q') => return UiEvent::Quit,
        KeyCode::Tab => app.next_section(),
        KeyCode::BackTab => app.previous_section(),
        KeyCode::Up => app.field_index = app.field_index.saturating_sub(1),
        KeyCode::Down => {
            app.field_index = (app.field_index + 1).min(app.fields().len().saturating_sub(1))
        }
        KeyCode::Enter => {
            if app.current_mut().bool_field {
                let current = app.current_mut().as_bool();
                app.current_mut().value = if current { "false" } else { "true" }.into();
            }
        }
        KeyCode::Backspace => {
            if !app.current_mut().bool_field {
                app.current_mut().value.pop();
            }
        }
        KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return UiEvent::Action(UiAction::Bootstrap)
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return UiEvent::Action(UiAction::SaveLocal)
        }
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return UiEvent::Action(UiAction::RenderRuntimeConfig)
        }
        KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return UiEvent::Action(UiAction::FetchYggsync)
        }
        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return UiEvent::Action(UiAction::ApplyClientStack)
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return UiEvent::Action(UiAction::InstallDesktopYggsync)
        }
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return UiEvent::Action(UiAction::SetupAndroidSync)
        }
        KeyCode::Char('i') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return UiEvent::Action(UiAction::BuildIso)
        }
        KeyCode::Char('m') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return UiEvent::Action(UiAction::Smoke)
        }
        KeyCode::Char(ch) => {
            if !app.current_mut().bool_field && !key.modifiers.contains(KeyModifiers::CONTROL) {
                app.current_mut().value.push(ch);
            }
        }
        _ => {}
    }
    UiEvent::None
}

fn handle_mouse(app: &mut App, mouse: MouseEvent) -> io::Result<()> {
    let (width, height) = crossterm::terminal::size()?;
    let layout = compute_layout(Rect::new(0, 0, width, height));
    let column_x = mouse.column;
    let row_y = mouse.row;

    match mouse.kind {
        MouseEventKind::ScrollDown => {
            app.field_index = (app.field_index + 1).min(app.fields().len().saturating_sub(1));
        }
        MouseEventKind::ScrollUp => {
            app.field_index = app.field_index.saturating_sub(1);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if point_in_rect(layout.outer[0], column_x, row_y) {
                let titles = Section::all();
                let tab_width = (layout.outer[0].width.max(1) / titles.len() as u16).max(1);
                let idx = ((column_x.saturating_sub(layout.outer[0].x)) / tab_width) as usize;
                app.section = idx.min(titles.len().saturating_sub(1));
                app.field_index = 0;
                return Ok(());
            }

            if point_in_rect(layout.body[0], column_x, row_y) {
                let local_row = row_y.saturating_sub(layout.body[0].y + 1) as usize;
                if local_row < app.fields().len() {
                    app.field_index = local_row;
                    if app.current_mut().bool_field {
                        let current = app.current_mut().as_bool();
                        app.current_mut().value = if current { "false" } else { "true" }.into();
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn run_tui_action(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    action: UiAction,
) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    println!("yggcli: running {}", action_name(action));

    let result = perform_action(app, action);
    match &result {
        Ok(message) => {
            println!("\nResult: {message}");
            app.status = message.clone();
        }
        Err(err) => {
            eprintln!("\nError: {err}");
            app.status = format!("{} failed: {err}", action_name(action));
        }
    }

    println!("\nPress Enter to return to yggcli...");
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);

    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    enable_raw_mode()?;
    Ok(())
}

fn perform_action(app: &App, action: UiAction) -> io::Result<String> {
    match action {
        UiAction::Bootstrap => {
            bootstrap_repos(&app.workspace_root(), &app.repo_base(), &app.platform)?;
            Ok(format!(
                "Bootstrapped repos into {}",
                app.workspace_root().display()
            ))
        }
        UiAction::SaveLocal => {
            let report = app.save_local_configs(true)?;
            Ok(format!(
                "Saved {} file(s), skipped {}",
                report.written.len(),
                report.skipped.len()
            ))
        }
        UiAction::RenderRuntimeConfig => {
            let _ = app.save_local_configs(true)?;
            let path = app.write_runtime_config(true)?;
            Ok(format!("Rendered runtime config to {}", path.display()))
        }
        UiAction::FetchYggsync => {
            run_fetch_yggsync(app, &app.platform)?;
            Ok("Fetched yggsync".into())
        }
        UiAction::ApplyClientStack => {
            let _ = app.save_local_configs(true)?;
            let path = app.write_runtime_config(true)?;
            apply_client_stack(app, &app.platform)?;
            Ok(format!(
                "Applied client stack and rendered {}",
                path.display()
            ))
        }
        UiAction::InstallDesktopYggsync => {
            let _ = app.save_local_configs(true)?;
            let path = app.write_runtime_config(true)?;
            install_desktop_yggsync(app, &app.platform)?;
            Ok(format!(
                "Installed desktop yggsync units after rendering {}",
                path.display()
            ))
        }
        UiAction::SetupAndroidSync => {
            let _ = app.save_local_configs(true)?;
            let path = app.write_runtime_config(true)?;
            run_android_setup(app, &app.platform)?;
            Ok(format!(
                "Ran Android setup after rendering {}",
                path.display()
            ))
        }
        UiAction::BuildIso => {
            let _ = app.save_local_configs(true)?;
            run_build(
                app,
                &app.get(&app.yggdrasil, "build_profile"),
                false,
                &app.platform,
            )?;
            Ok("Completed yggdrasil build".into())
        }
        UiAction::Smoke => {
            run_smoke(
                app,
                &app.get(&app.yggdrasil, "build_profile"),
                app.get_bool(&app.yggdrasil, "enable_qemu_smoke"),
                &app.platform,
            )?;
            Ok("Completed smoke run".into())
        }
    }
}

fn action_name(action: UiAction) -> &'static str {
    match action {
        UiAction::Bootstrap => "bootstrap",
        UiAction::SaveLocal => "save-local-configs",
        UiAction::RenderRuntimeConfig => "render-runtime-config",
        UiAction::FetchYggsync => "fetch-yggsync",
        UiAction::ApplyClientStack => "apply-client-stack",
        UiAction::InstallDesktopYggsync => "install-desktop-yggsync",
        UiAction::SetupAndroidSync => "setup-android-sync",
        UiAction::BuildIso => "build-iso",
        UiAction::Smoke => "smoke",
    }
}

fn point_in_rect(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

fn compute_layout(area: Rect) -> UiLayout {
    let outer = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(10),
        Constraint::Length(6),
    ])
    .split(area)
    .to_vec();
    let body = Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(outer[1])
        .to_vec();
    let right = Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(body[1])
        .to_vec();
    UiLayout { outer, body, right }
}

fn draw(frame: &mut Frame, app: &App) {
    let layout = compute_layout(frame.area());

    let title = format!(
        "yggcli {} [{}]",
        env!("CARGO_PKG_VERSION"),
        app.platform.label()
    );
    let titles: Vec<Line> = Section::all()
        .iter()
        .map(|section| {
            Line::from(Span::styled(
                section.title(),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Rgb(224, 196, 91)),
            ))
        })
        .collect();
    let tabs = Tabs::new(titles)
        .select(app.section)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    title,
                    Style::default()
                        .fg(Color::Rgb(255, 230, 140))
                        .add_modifier(Modifier::BOLD),
                ))
                .border_style(Style::default().fg(Color::Rgb(96, 128, 255))),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(133, 239, 163))
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        );
    frame.render_widget(tabs, layout.outer[0]);

    let fields = app.fields();
    let field_lines: Vec<Line> = fields
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let selected = idx == app.field_index;
            let value = if field.bool_field {
                format!("[{}]", field.value)
            } else {
                field.value.clone()
            };
            Line::from(vec![
                Span::styled(
                    if selected { "> " } else { "  " },
                    Style::default().fg(Color::Rgb(255, 230, 140)),
                ),
                Span::styled(
                    format!("{:<24}", field.label),
                    Style::default()
                        .fg(if selected {
                            Color::Rgb(133, 239, 163)
                        } else {
                            Color::Rgb(132, 182, 244)
                        })
                        .add_modifier(if selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::raw(" "),
                Span::styled(
                    value,
                    Style::default().fg(if selected { Color::White } else { Color::Gray }),
                ),
            ])
        })
        .collect();

    let fields_widget = Paragraph::new(field_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(app.section().title())
                .border_style(Style::default().fg(Color::Rgb(96, 128, 255))),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(fields_widget, layout.body[0]);

    let current = app.current_field();
    let note_lines = vec![
        Line::from(vec![
            Span::styled(
                "Field: ",
                Style::default()
                    .fg(Color::Rgb(255, 230, 140))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(current.label),
        ]),
        Line::from(""),
        Line::from(current.help),
        Line::from(""),
        Line::from(section_note(app.section())),
    ];
    let note_widget = Paragraph::new(note_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Selected Field")
                .border_style(Style::default().fg(Color::Rgb(224, 196, 91))),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(note_widget, layout.right[0]);

    let mut action_lines = Vec::new();
    action_lines.push(Line::from("Hotkeys and equivalent CLI:"));
    action_lines.push(Line::from(""));
    for spec in action_specs(&app.platform) {
        action_lines.push(Line::from(format!(
            "{:<7} {:<27} {}",
            spec.key, spec.cli, spec.description
        )));
    }
    let actions_widget = Paragraph::new(action_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Actions")
                .border_style(Style::default().fg(Color::Rgb(133, 239, 163))),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(actions_widget, layout.right[1]);

    let footer = vec![
        Line::from(vec![
            Span::styled(
                "Status: ",
                Style::default()
                    .fg(Color::Rgb(255, 230, 140))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(app.status.as_str()),
        ]),
        Line::from(format!(
            "Current runtime config path: {}",
            app.get(&app.yggclient, "yggsync_config")
        )),
        Line::from("Use --list-fields and --list-actions in CLI mode when you want the same information outside the TUI."),
    ];
    let status = Paragraph::new(footer)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Help")
                .border_style(Style::default().fg(Color::Rgb(133, 239, 163))),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(Clear, layout.outer[2]);
    frame.render_widget(status, layout.outer[2]);
}

fn section_note(section: Section) -> &'static str {
    match section {
        Section::Workspace => {
            "Workspace settings decide where yggcli finds or clones ecosystem repos. Change repo_base only when you intentionally want a different remote origin."
        }
        Section::Yggdrasil => {
            "Server settings are conservative by default. First host: apt_proxy_mode=off, infisical_boot_mode=disabled, with_lts=false unless you intentionally want the compatibility kernel path."
        }
        Section::Yggclient => {
            "Client settings define endpoint identity, SMB credentials, and whether this machine uses direct SMB or a mounted NAS path."
        }
        Section::Yggsync => {
            "Sync settings define local paths and remote relative paths. Keep remote values target-relative so yggcli can generate either SMB or mounted-path configs from the same fields."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linux_app() -> App {
        App::with_platform_and_workspace(
            Platform {
                os: "linux".into(),
                arch: "x86_64".into(),
                is_android: false,
                is_termux: false,
            },
            "/tmp/yggcli-test".into(),
        )
    }

    fn android_app() -> App {
        App::with_platform_and_workspace(
            Platform {
                os: "android".into(),
                arch: "aarch64".into(),
                is_android: true,
                is_termux: true,
            },
            "/tmp/yggcli-test".into(),
        )
    }

    #[test]
    fn bool_override_works() {
        let mut app = linux_app();
        app.apply_override("yggclient.use_mounted_nas=true")
            .expect("override should apply");
        assert!(app.get_bool(&app.yggclient, "use_mounted_nas"));
    }

    #[test]
    fn mounted_nas_generates_local_target() {
        let mut app = linux_app();
        app.apply_override("yggclient.use_mounted_nas=true")
            .expect("override should apply");
        app.apply_override("yggclient.mounted_nas_root=/run/mount/data")
            .expect("override should apply");
        let cfg = app.build_yggsync_config();
        assert_eq!(cfg.targets.len(), 1);
        assert_eq!(cfg.targets[0].kind, "local");
        assert_eq!(cfg.targets[0].path, "/run/mount/data");
        assert!(cfg
            .jobs
            .iter()
            .any(|job| job.remote.starts_with("mounted:")));
    }

    #[test]
    fn android_generates_obsidian_and_dcim_jobs() {
        let app = android_app();
        let cfg = app.build_yggsync_config();
        assert!(cfg
            .jobs
            .iter()
            .any(|job| job.name == "obsidian" && job.kind == "worktree"));
        assert!(cfg
            .jobs
            .iter()
            .any(|job| job.name == "dcim" && job.kind == "retained_copy"));
        assert!(!cfg.jobs.iter().any(|job| job.name == "screencasts"));
    }
}

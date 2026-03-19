use std::{
    env, fs,
    io::{self, Stdout},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use serde::{Deserialize, Serialize};
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

const DEFAULT_WORKSPACE: &str = "/root/gh";
const DEFAULT_REPO_BASE: &str = "https://github.com/yggdrasilhq";
const ECOSYSTEM_REPOS: &[&str] = &[
    "yggdrasil",
    "yggcli",
    "yggclient",
    "yggsync",
    "yggdocs",
    "yggterm",
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

#[derive(Default, Serialize, Deserialize)]
struct YggdrasilConfig {
    build_profile: String,
    enable_qemu_smoke: bool,
    with_nvidia: bool,
    setup_mode: String,
    apt_proxy_mode: String,
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
struct YggsyncConfig {
    rclone_binary: String,
    rclone_config: String,
    lock_file: String,
    default_flags: Vec<String>,
    jobs: Vec<YggsyncJob>,
}

#[derive(Default, Serialize, Deserialize)]
struct YggsyncJob {
    name: String,
    description: String,
    r#type: String,
    local: String,
    remote: String,
    timeout_seconds: u32,
    local_retention_days: Option<u32>,
    flags: Vec<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    resync_on_exit: Option<Vec<u32>>,
    resync_flags: Option<Vec<String>>,
}

struct Field {
    label: &'static str,
    value: String,
    bool_field: bool,
}

impl Field {
    fn text(label: &'static str, value: impl Into<String>) -> Self {
        Self {
            label,
            value: value.into(),
            bool_field: false,
        }
    }

    fn boolean(label: &'static str, value: bool) -> Self {
        Self {
            label,
            value: if value { "true" } else { "false" }.into(),
            bool_field: true,
        }
    }

    fn as_bool(&self) -> bool {
        self.value == "true"
    }
}

struct App {
    section: usize,
    field_index: usize,
    workspace: Vec<Field>,
    yggdrasil: Vec<Field>,
    yggclient: Vec<Field>,
    yggsync: Vec<Field>,
    status: String,
}

struct UiLayout {
    outer: Vec<Rect>,
    body: Vec<Rect>,
}

struct SaveReport {
    written: Vec<PathBuf>,
    skipped: Vec<PathBuf>,
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
}

impl Default for App {
    fn default() -> Self {
        Self::with_workspace(DEFAULT_WORKSPACE)
    }
}

impl App {
    fn with_workspace(root: &str) -> Self {
        let platform = Platform::detect();
        let mut app = Self {
            section: 0,
            field_index: 0,
            workspace: vec![
                Field::text("workspace_root", root),
                Field::text("docs_repo", "yggdocs"),
                Field::text("server_repo", "yggdrasil"),
                Field::text("client_repo", "yggclient"),
                Field::text("sync_repo", "yggsync"),
            ],
            yggdrasil: vec![
                Field::text("build_profile", "both"),
                Field::boolean("enable_qemu_smoke", false),
                Field::boolean("with_nvidia", false),
                Field::text("setup_mode", "recommended"),
                Field::text("apt_proxy_mode", "off"),
                Field::boolean("embed_ssh_keys", true),
                Field::text("ssh_authorized_keys_file", "/root/.ssh/authorized_keys"),
                Field::text("hostname", "yggdrasil"),
                Field::text("net_mode", "dhcp"),
                Field::text("lxc_parent_if", "eno1"),
                Field::text("macvlan_cidr", "10.10.0.250/24"),
                Field::text("macvlan_route", "10.10.0.0/24"),
                Field::text("static_iface", "eno1"),
                Field::text("static_ip", ""),
                Field::text("static_gateway", ""),
                Field::text("static_dns", "1.1.1.1 8.8.8.8"),
                Field::text("apt_http_proxy", ""),
                Field::text("apt_https_proxy", ""),
                Field::text("apt_proxy_bypass_host", ""),
            ],
            yggclient: vec![
                Field::text("profile_name", "laptop"),
                Field::text("user_name", "alice"),
                Field::text("user_home", "/home/alice"),
                Field::text("ssh_host", "example-host"),
                Field::text("ssh_user", "alice"),
                Field::text("apt_http_proxy", ""),
                Field::text("apt_https_proxy", ""),
                Field::boolean("enable_yggsync", true),
                Field::text("yggsync_repo", "https://github.com/yggdrasilhq/yggsync"),
                Field::text("yggsync_config", "~/.config/ygg_sync.toml"),
                Field::boolean("install_desktop_timer", true),
                Field::boolean("install_shift_sync", false),
                Field::boolean("install_kmonad", false),
            ],
            yggsync: vec![
                Field::text("rclone_binary", "rclone"),
                Field::text("rclone_config", "~/.config/rclone/rclone.conf"),
                Field::text("lock_file", "~/.local/state/yggsync.lock"),
                Field::text("notes_local", "~/Documents/notes"),
                Field::text("notes_remote", "nas:users/alice/notes"),
                Field::text("camera_local", "~/Pictures/Camera"),
                Field::text("camera_remote", "nas:users/alice/media/camera-roll"),
                Field::text("screenshots_local", "~/Pictures/Screenshots"),
                Field::text("screenshots_remote", "nas:users/alice/media/screenshots"),
            ],
            status: "Tab: switch section | Up/Down: move | Type: edit | Enter: toggle bool | Ctrl-S: save | q: quit".into(),
        };

        if platform.is_android {
            app.yggclient = vec![
                Field::text("profile_name", "android"),
                Field::text("user_name", "termux"),
                Field::text("user_home", "$HOME"),
                Field::text("ssh_host", "example-host"),
                Field::text("ssh_user", "alice"),
                Field::text("apt_http_proxy", ""),
                Field::text("apt_https_proxy", ""),
                Field::boolean("enable_yggsync", true),
                Field::text("yggsync_repo", "https://github.com/yggdrasilhq/yggsync"),
                Field::text("yggsync_config", "~/.config/ygg_sync.toml"),
                Field::boolean("install_desktop_timer", false),
                Field::boolean("install_shift_sync", false),
                Field::boolean("install_kmonad", false),
            ];
            app.yggsync = vec![
                Field::text("rclone_binary", "rclone"),
                Field::text("rclone_config", "~/.config/rclone/rclone.conf"),
                Field::text("lock_file", "~/.local/state/yggsync.lock"),
                Field::text("notes_local", "~/storage/shared/Documents/obsidian"),
                Field::text("notes_remote", "nas:users/alice/obsidian"),
                Field::text("camera_local", "~/storage/shared/DCIM"),
                Field::text("camera_remote", "nas:users/alice/media/dcim"),
                Field::text("screenshots_local", "~/storage/shared/Pictures/Screenshots"),
                Field::text("screenshots_remote", "nas:users/alice/media/screenshots"),
            ];
        }

        app.load_existing_configs(root, &platform);

        app
    }

    fn load_existing_configs(&mut self, root: &str, platform: &Platform) {
        let root = PathBuf::from(root);
        if platform.supports_host_builds() {
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
        Self::set_field(&mut self.yggdrasil, "setup_mode", cfg.setup_mode);
        Self::set_field(&mut self.yggdrasil, "apt_proxy_mode", cfg.apt_proxy_mode);
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
        Self::set_field(&mut self.yggsync, "rclone_binary", cfg.rclone_binary);
        Self::set_field(&mut self.yggsync, "rclone_config", cfg.rclone_config);
        Self::set_field(&mut self.yggsync, "lock_file", cfg.lock_file);
        for job in cfg.jobs {
            match job.name.as_str() {
                "notes" | "obsidian" => {
                    Self::set_field(&mut self.yggsync, "notes_local", job.local);
                    Self::set_field(&mut self.yggsync, "notes_remote", job.remote);
                }
                "camera-roll" | "dcim" => {
                    Self::set_field(&mut self.yggsync, "camera_local", job.local);
                    Self::set_field(&mut self.yggsync, "camera_remote", job.remote);
                }
                "screenshots" => {
                    Self::set_field(&mut self.yggsync, "screenshots_local", job.local);
                    Self::set_field(&mut self.yggsync, "screenshots_remote", job.remote);
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

    fn save(&self, force: bool, platform: &Platform) -> io::Result<SaveReport> {
        let root = PathBuf::from(self.get(&self.workspace, "workspace_root"));
        let server_repo = root.join(self.get(&self.workspace, "server_repo"));
        let client_repo = root.join(self.get(&self.workspace, "client_repo"));
        let sync_repo = root.join(self.get(&self.workspace, "sync_repo"));

        let yggdrasil = YggdrasilConfig {
            build_profile: self.get(&self.yggdrasil, "build_profile"),
            enable_qemu_smoke: self.get_bool(&self.yggdrasil, "enable_qemu_smoke"),
            with_nvidia: self.get_bool(&self.yggdrasil, "with_nvidia"),
            setup_mode: self.get(&self.yggdrasil, "setup_mode"),
            apt_proxy_mode: self.get(&self.yggdrasil, "apt_proxy_mode"),
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
        };

        let yggclient = YggclientConfig {
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
            },
            services: ServicesConfig {
                install_desktop_timer: self.get_bool(&self.yggclient, "install_desktop_timer"),
                install_shift_sync: self.get_bool(&self.yggclient, "install_shift_sync"),
                install_kmonad: self.get_bool(&self.yggclient, "install_kmonad"),
            },
        };

        let yggsync = YggsyncConfig {
            rclone_binary: self.get(&self.yggsync, "rclone_binary"),
            rclone_config: self.get(&self.yggsync, "rclone_config"),
            lock_file: self.get(&self.yggsync, "lock_file"),
            default_flags: if platform.is_android {
                vec![
                    "--use-json-log".into(),
                    "--stats=120s".into(),
                    "--transfers=1".into(),
                    "--checkers=2".into(),
                ]
            } else {
                vec![
                    "--use-json-log".into(),
                    "--stats=120s".into(),
                    "--transfers=2".into(),
                    "--checkers=4".into(),
                ]
            },
            jobs: vec![
                YggsyncJob {
                    name: if platform.is_android {
                        "obsidian".into()
                    } else {
                        "notes".into()
                    },
                    description: if platform.is_android {
                        "Keep the working Obsidian vault in sync between phone and NAS".into()
                    } else {
                        "Keep the working notes tree in sync between laptop and NAS".into()
                    },
                    r#type: "bisync".into(),
                    local: self.get(&self.yggsync, "notes_local"),
                    remote: self.get(&self.yggsync, "notes_remote"),
                    timeout_seconds: 900,
                    resync_on_exit: Some(vec![7]),
                    resync_flags: Some(vec!["--resync".into()]),
                    exclude: Some(vec!["**/.obsidian/**".into(), "**/.trash/**".into()]),
                    flags: vec![
                        "--create-empty-src-dirs".into(),
                        "--resilient".into(),
                        "--recover".into(),
                        "--conflict-loser".into(),
                        "pathname".into(),
                        "--max-delete".into(),
                        "90".into(),
                    ],
                    ..Default::default()
                },
                YggsyncJob {
                    name: if platform.is_android {
                        "dcim".into()
                    } else {
                        "camera-roll".into()
                    },
                    description: if platform.is_android {
                        "Upload phone camera media first, then prune old locals after remote confirmation".into()
                    } else {
                        "Upload camera media first, then prune old locals after remote confirmation"
                            .into()
                    },
                    r#type: "retained_copy".into(),
                    local: self.get(&self.yggsync, "camera_local"),
                    remote: self.get(&self.yggsync, "camera_remote"),
                    timeout_seconds: 1800,
                    local_retention_days: Some(30),
                    flags: vec!["--create-empty-src-dirs".into()],
                    ..Default::default()
                },
                YggsyncJob {
                    name: "screenshots".into(),
                    description: "Offload screenshots and keep the device lean".into(),
                    r#type: "retained_copy".into(),
                    local: self.get(&self.yggsync, "screenshots_local"),
                    remote: self.get(&self.yggsync, "screenshots_remote"),
                    timeout_seconds: 900,
                    local_retention_days: Some(30),
                    ..Default::default()
                },
            ],
        };

        let mut report = SaveReport {
            written: Vec::new(),
            skipped: Vec::new(),
        };
        if platform.supports_host_builds() && server_repo.exists() {
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
            fs::create_dir_all(client_repo.join("config"))?;
            write_file(
                &client_repo.join("config/profiles.local.env"),
                &self.render_client_env(&yggclient),
                force,
                &mut report,
            )?;
        } else {
            report.skipped.push(client_repo.join("yggclient.local.toml"));
            report.skipped.push(client_repo.join("config/profiles.local.env"));
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

    fn render_client_env(&self, cfg: &YggclientConfig) -> String {
        format!(
            "# Generated by yggcli\nPROFILE_NAME='{profile}'\nUSER_NAME='{user}'\nUSER_HOME='{home}'\nSSH_HOST='{host}'\nSSH_USER='{ssh_user}'\nAPT_HTTP_PROXY='{http}'\nAPT_HTTPS_PROXY='{https}'\nYGGSYNC_REPO='{repo}'\nYGGSYNC_CONFIG='{sync_cfg}'\nENABLE_YGGSYNC='{sync}'\nINSTALL_DESKTOP_TIMER='{timer}'\nINSTALL_SHIFT_SYNC='{shift}'\nINSTALL_KMONAD='{kmonad}'\n",
            profile = cfg.identity.profile_name,
            user = cfg.identity.user_name,
            home = cfg.identity.user_home,
            host = cfg.network.ssh_host,
            ssh_user = cfg.network.ssh_user,
            http = cfg.network.apt_http_proxy,
            https = cfg.network.apt_https_proxy,
            repo = cfg.sync.yggsync_repo,
            sync_cfg = cfg.sync.yggsync_config,
            sync = if cfg.sync.enable_yggsync { "1" } else { "0" },
            timer = if cfg.services.install_desktop_timer { "1" } else { "0" },
            shift = if cfg.services.install_shift_sync { "1" } else { "0" },
            kmonad = if cfg.services.install_kmonad { "1" } else { "0" },
        )
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

fn usage() {
    println!(
        "yggcli\n\nUsage:\n  yggcli                         Launch interactive TUI\n  yggcli [options]               Run non-interactive workflow\n\nOptions:\n  --workspace PATH               Workspace root (default: {DEFAULT_WORKSPACE})\n  --repo-base URL                Repo base for bootstrap clones (default: {DEFAULT_REPO_BASE})\n  --bootstrap                    Clone missing ecosystem repos\n  --write-defaults               Write local config files using sensible defaults\n  --force                        Overwrite existing local config files\n  --set section.key=value        Override one field before save/build (repeatable)\n  --build-iso                    Run yggdrasil build after config generation\n  --smoke                        Run smoke bench explicitly after build/config\n  --profile server|kde|both      Profile for build/smoke (default: both)\n  --skip-smoke                   Skip smoke inside mkconfig build step\n  --with-qemu                    Add QEMU/KVM smoke when running explicit smoke\n  -h, --help                     Show this help\n\nExamples:\n  yggcli --bootstrap --write-defaults\n  yggcli --workspace ~/gh --build-iso --profile server\n  yggcli --workspace ~/gh --smoke --profile kde --with-qemu\n  yggcli --workspace ~/gh --set yggdrasil.hostname=mewmew --set yggdrasil.net_mode=dhcp --build-iso --profile server\n\nGuidance:\n  - First server build: keep apt_proxy_mode=off.\n  - After the host is alive, follow the apt-proxy LXC recipe in yggdocs and switch to apt_proxy_mode=explicit.\n  - Android/Termux hosts can configure yggclient and yggsync, but they do not build yggdrasil ISOs.\n  - Non-interactive builds auto-use sudo -n when root privileges are required.\n\nTUI controls:\n  - Keyboard: Tab/Shift-Tab switch sections, Up/Down move, Enter toggles booleans, Ctrl-S saves, q quits.\n  - Mouse: click tabs, click fields, scroll within a section, click boolean values to toggle.\n"
    );
}

fn parse_cli() -> Result<CliOptions, String> {
    let mut opts = CliOptions {
        workspace_root: DEFAULT_WORKSPACE.into(),
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
            "--set" => opts.sets.push(args.next().ok_or("--set requires section.key=value")?),
            "--skip-smoke" => opts.skip_smoke = true,
            "--with-qemu" => opts.with_qemu = true,
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
        || opts.help
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

fn run_build(
    workspace_root: &Path,
    profile: &str,
    skip_smoke: bool,
    platform: &Platform,
) -> io::Result<()> {
    if !platform.supports_host_builds() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "yggdrasil ISO builds are disabled on Android/Termux hosts",
        ));
    }
    let repo = workspace_root.join("yggdrasil");
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

fn run_smoke(
    workspace_root: &Path,
    profile: &str,
    with_qemu: bool,
    platform: &Platform,
) -> io::Result<()> {
    if !platform.supports_host_builds() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "yggdrasil smoke benches are disabled on Android/Termux hosts",
        ));
    }
    let repo = workspace_root.join("yggdrasil");
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

fn main() -> io::Result<()> {
    let opts = parse_cli().map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let platform = Platform::detect();
    if opts.help {
        usage();
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
        let workspace_root = PathBuf::from(&opts.workspace_root);
        if opts.bootstrap {
            bootstrap_repos(&workspace_root, &opts.repo_base, &platform)?;
        }

        let mut app = App::with_workspace(&opts.workspace_root);
        for spec in &opts.sets {
            app.apply_override(spec)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        }
        if opts.write_defaults || opts.build_iso || opts.smoke || !opts.sets.is_empty() {
            let should_force = opts.force || !opts.sets.is_empty();
            let report = app.save(should_force, &platform)?;
            if !report.written.is_empty() {
                eprintln!(
                    "written: {}",
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
                    "skipped existing: {}",
                    report
                        .skipped
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }

        if opts.build_iso {
            run_build(&workspace_root, &opts.profile, opts.skip_smoke, &platform)?;
        }
        if opts.smoke {
            run_smoke(&workspace_root, &opts.profile, opts.with_qemu, &platform)?;
        }
        return Ok(());
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let result = run_app(stdout);
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    result
}

impl App {
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
            Event::Key(key) => {
                if handle_key(&mut app, key)? {
                    break;
                }
            }
            Event::Mouse(mouse) => handle_mouse(&mut app, mouse)?,
            _ => {}
        }
    }

    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> io::Result<bool> {
    match key.code {
        KeyCode::Char('q') => return Ok(true),
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
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let platform = Platform::detect();
            match app.save(true, &platform) {
            Ok(report) => {
                app.status = format!(
                    "Saved {} file(s), skipped {}",
                    report.written.len(),
                    report.skipped.len()
                );
            }
            Err(err) => app.status = format!("Save failed: {err}"),
            }
        }
        KeyCode::Char(ch) => {
            if !app.current_mut().bool_field && !key.modifiers.contains(KeyModifiers::CONTROL) {
                app.current_mut().value.push(ch);
            }
        }
        _ => {}
    }
    Ok(false)
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

fn point_in_rect(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

fn compute_layout(area: Rect) -> UiLayout {
    let outer = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(10),
        Constraint::Length(5),
    ])
    .split(area)
    .to_vec();
    let body = Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(outer[1])
        .to_vec();
    UiLayout { outer, body }
}

fn draw(frame: &mut Frame, app: &App) {
    let layout = compute_layout(frame.area());

    let titles: Vec<Line> = Section::all()
        .iter()
        .map(|s| Line::from(Span::styled(
            s.title(),
            Style::default().fg(Color::Black).bg(Color::Rgb(224, 196, 91)),
        )))
        .collect();
    let tabs = Tabs::new(titles)
        .select(app.section)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    "yggcli",
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
    let lines: Vec<Line> = fields
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
                    format!("{:<22}", field.label),
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
                    Style::default().fg(if selected {
                        Color::White
                    } else {
                        Color::Gray
                    }),
                ),
            ])
        })
        .collect();

    let help = match app.section() {
        Section::Workspace => "Choose the workspace roots and repo names. yggcli writes plain-text configs into those repos so advanced users can still edit them by hand.",
        Section::Yggdrasil => "Server ISO settings. First server build: keep apt_proxy_mode=off. After the host is alive, follow the apt-proxy LXC recipe in yggdocs and then switch to apt_proxy_mode=explicit for faster later builds.",
        Section::Yggclient => "Endpoint profile settings. yggcli writes both yggclient.local.toml and config/profiles.local.env so existing scripts and hand-edited setups keep working.",
        Section::Yggsync => "Sync engine settings. Start with a narrow scope. Notes first, then camera roll, then screenshots. Wider sync is easy later; data recovery is not.",
    };

    let fields_widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(app.section().title())
                .border_style(Style::default().fg(Color::Rgb(96, 128, 255))),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(fields_widget, layout.body[0]);

    let note = Paragraph::new(help)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Operator Note")
                .border_style(Style::default().fg(Color::Rgb(224, 196, 91))),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(note, layout.body[1]);

    let footer = vec![
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::Rgb(255, 230, 140)).add_modifier(Modifier::BOLD)),
            Span::raw(app.status.as_str()),
        ]),
        Line::from("Examples: yggcli --bootstrap --write-defaults | yggcli --workspace ~/gh --build-iso --profile server"),
        Line::from("Mouse: click tabs/fields, scroll to move. Keyboard: Tab, Shift-Tab, Up/Down, Enter, Ctrl-S, q."),
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

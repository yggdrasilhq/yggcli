use std::{
    env, fs,
    io::{self, Stdout},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Tabs, Wrap},
    Terminal,
};
use serde::Serialize;

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

#[derive(Default, Serialize)]
struct YggdrasilConfig {
    build_profile: String,
    enable_qemu_smoke: bool,
    setup_mode: String,
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

#[derive(Default, Serialize)]
struct IdentityConfig {
    profile_name: String,
    user_name: String,
    user_home: String,
}

#[derive(Default, Serialize)]
struct NetworkConfig {
    ssh_host: String,
    ssh_user: String,
    apt_http_proxy: String,
    apt_https_proxy: String,
}

#[derive(Default, Serialize)]
struct SyncConfig {
    enable_yggsync: bool,
    yggsync_repo: String,
    yggsync_config: String,
}

#[derive(Default, Serialize)]
struct ServicesConfig {
    install_desktop_timer: bool,
    install_shift_sync: bool,
    install_kmonad: bool,
}

#[derive(Default, Serialize)]
struct YggclientConfig {
    identity: IdentityConfig,
    network: NetworkConfig,
    sync: SyncConfig,
    services: ServicesConfig,
}

#[derive(Default, Serialize)]
struct YggsyncConfig {
    rclone_binary: String,
    rclone_config: String,
    lock_file: String,
    default_flags: Vec<String>,
    jobs: Vec<YggsyncJob>,
}

#[derive(Default, Serialize)]
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
        Self {
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
                Field::text("setup_mode", "recommended"),
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

    fn save(&self, force: bool) -> io::Result<SaveReport> {
        let root = PathBuf::from(self.get(&self.workspace, "workspace_root"));
        let server_repo = root.join(self.get(&self.workspace, "server_repo"));
        let client_repo = root.join(self.get(&self.workspace, "client_repo"));
        let sync_repo = root.join(self.get(&self.workspace, "sync_repo"));

        let yggdrasil = YggdrasilConfig {
            build_profile: self.get(&self.yggdrasil, "build_profile"),
            enable_qemu_smoke: self.get_bool(&self.yggdrasil, "enable_qemu_smoke"),
            setup_mode: self.get(&self.yggdrasil, "setup_mode"),
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
            default_flags: vec![
                "--fast-list".into(),
                "--use-json-log".into(),
                "--stats=30s".into(),
                "--transfers=4".into(),
            ],
            jobs: vec![
                YggsyncJob {
                    name: "notes".into(),
                    description: "Keep the working notes tree in sync between laptop and NAS"
                        .into(),
                    r#type: "bisync".into(),
                    local: self.get(&self.yggsync, "notes_local"),
                    remote: self.get(&self.yggsync, "notes_remote"),
                    timeout_seconds: 900,
                    resync_on_exit: Some(vec![7]),
                    resync_flags: Some(vec!["--resync".into()]),
                    exclude: Some(vec!["**/.obsidian/**".into(), "**/.trash/**".into()]),
                    ..Default::default()
                },
                YggsyncJob {
                    name: "camera-roll".into(),
                    description:
                        "Upload camera media first, then prune old locals after remote confirmation"
                            .into(),
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
        write_file(
            &server_repo.join("ygg.local.toml"),
            &toml::to_string_pretty(&yggdrasil).unwrap(),
            force,
            &mut report,
        )?;
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
        write_file(
            &sync_repo.join("ygg_sync.local.toml"),
            &toml::to_string_pretty(&yggsync).unwrap(),
            force,
            &mut report,
        )?;
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
        "yggcli\n\nUsage:\n  yggcli                         Launch interactive TUI\n  yggcli [options]              Run non-interactive workflow\n\nOptions:\n  --workspace PATH              Workspace root (default: {DEFAULT_WORKSPACE})\n  --repo-base URL               Repo base for bootstrap clones (default: {DEFAULT_REPO_BASE})\n  --bootstrap                   Clone missing ecosystem repos\n  --write-defaults              Write local config files using sensible defaults\n  --force                       Overwrite existing local config files\n  --build-iso                   Run yggdrasil build after config generation\n  --smoke                       Run smoke bench explicitly after build/config\n  --profile server|kde|both     Profile for build/smoke (default: both)\n  --skip-smoke                  Skip smoke inside mkconfig build step\n  --with-qemu                   Add QEMU/KVM smoke when running explicit smoke\n  -h, --help                    Show this help\n"
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
    opts.bootstrap || opts.write_defaults || opts.build_iso || opts.smoke || opts.help
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

fn bootstrap_repos(workspace_root: &Path, repo_base: &str) -> io::Result<()> {
    fs::create_dir_all(workspace_root)?;
    for repo in ECOSYSTEM_REPOS {
        let target = workspace_root.join(repo);
        if target.exists() {
            continue;
        }
        let url = format!("{repo_base}/{repo}.git");
        run_cmd(Command::new("git").arg("clone").arg(url).arg(&target))?;
    }
    Ok(())
}

fn run_build(workspace_root: &Path, profile: &str, skip_smoke: bool) -> io::Result<()> {
    let repo = workspace_root.join("yggdrasil");
    let mut cmd = Command::new("./mkconfig.sh");
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

fn run_smoke(workspace_root: &Path, profile: &str, with_qemu: bool) -> io::Result<()> {
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
    if opts.help {
        usage();
        return Ok(());
    }

    if has_non_interactive_action(&opts) {
        let workspace_root = PathBuf::from(&opts.workspace_root);
        if opts.bootstrap {
            bootstrap_repos(&workspace_root, &opts.repo_base)?;
        }

        let app = App::with_workspace(&opts.workspace_root);
        if opts.write_defaults || opts.build_iso || opts.smoke {
            let report = app.save(opts.force)?;
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
            run_build(&workspace_root, &opts.profile, opts.skip_smoke)?;
        }
        if opts.smoke {
            run_smoke(&workspace_root, &opts.profile, opts.with_qemu)?;
        }
        return Ok(());
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let result = run_app(stdout);
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    result
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
        if let Event::Key(key) = event::read()? {
            if handle_key(&mut app, key)? {
                break;
            }
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
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => match app.save(true)
        {
            Ok(report) => {
                app.status = format!(
                    "Saved {} file(s), skipped {}",
                    report.written.len(),
                    report.skipped.len()
                );
            }
            Err(err) => app.status = format!("Save failed: {err}"),
        },
        KeyCode::Char(ch) => {
            if !app.current_mut().bool_field && !key.modifiers.contains(KeyModifiers::CONTROL) {
                app.current_mut().value.push(ch);
            }
        }
        _ => {}
    }
    Ok(false)
}

fn draw(frame: &mut Frame, app: &App) {
    let outer = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(10),
        Constraint::Length(4),
    ])
    .split(frame.area());

    let titles: Vec<Line> = Section::all()
        .iter()
        .map(|s| Line::from(s.title()))
        .collect();
    let tabs = Tabs::new(titles)
        .select(app.section)
        .block(Block::default().borders(Borders::ALL).title("yggcli"))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, outer[0]);

    let fields = app.fields();
    let lines: Vec<Line> = fields
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            let prefix = if idx == app.field_index { ">" } else { " " };
            let value = if field.bool_field {
                format!("[{}]", field.value)
            } else {
                field.value.clone()
            };
            Line::from(format!("{prefix} {:<22} {}", field.label, value))
        })
        .collect();

    let help = match app.section() {
        Section::Workspace => "Choose where yggcli should write native config files for the server, client, and sync repos.",
        Section::Yggdrasil => "Server ISO settings. Keep this generic in public examples and put your private values in ygg.local.toml.",
        Section::Yggclient => "Endpoint profile settings. yggcli writes both yggclient.local.toml and config/profiles.local.env.",
        Section::Yggsync => "Sync engine settings. Start with a few safe jobs before you widen the net.",
    };

    let body = Layout::horizontal([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(outer[1]);
    let fields_widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(app.section().title()),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(fields_widget, body[0]);

    let note = Paragraph::new(help)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Operator Note"),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(note, body[1]);

    let status = Paragraph::new(app.status.as_str())
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .wrap(Wrap { trim: true });
    frame.render_widget(status, outer[2]);
}

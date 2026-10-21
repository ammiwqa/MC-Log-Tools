//! GUI на egui/eframe.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use eframe::egui;
use logtools_app::{CommandResult, Presenter};

// =========================
// PRESENTER
// =========================

enum PresenterEvent {
    Progress { pos: u64, total: u64 },
    Message(String),
    Log(String),
    Error(String),
    ConfirmRequest {
        prompt: String,
        reply_tx: Sender<bool>,
    },
}

struct EguiPresenter {
    tx: Sender<PresenterEvent>,
}

impl Presenter for EguiPresenter {
    fn progress(&mut self, pos: u64, total: u64) {
        let _ = self.tx.send(PresenterEvent::Progress { pos, total });
    }

    fn message(&mut self, msg: &str) {
        let _ = self.tx.send(PresenterEvent::Message(msg.to_string()));
    }

    fn log(&mut self, line: &str) {
        let _ = self.tx.send(PresenterEvent::Log(line.to_string()));
    }

    fn error(&mut self, msg: &str) {
        let _ = self.tx.send(PresenterEvent::Error(msg.to_string()));
    }

    fn confirm(&mut self, prompt: &str) -> bool {
        let (reply_tx, reply_rx) = channel::<bool>();

        let sent = self
            .tx
            .send(PresenterEvent::ConfirmRequest {
                prompt: prompt.to_string(),
                reply_tx,
            })
            .is_ok();

        if !sent {
            return false;
        }

        reply_rx.recv().unwrap_or(false)
    }

    fn open_file(&mut self, path: &PathBuf) {
        open_in_system_app(path);
    }
}

fn open_in_system_app(path: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn();
    }

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

enum GuiCommand {
    CreateBase {
        name: String,
        paths: Vec<String>,
        from_file: Option<String>,
    },
    Search {
        base: String,
        text: String,
    },
    UpdateBase {
        name: String,
    },
    Delete {
        name: String,
        force_yes: bool,
    },
    Discover {
        root: Option<String>,
        out: Option<String>,
        es: Option<String>,
        no_everything: bool,
    },
    ConfigParams {
        params: Vec<(String, String)>,
    },
}


#[derive(Clone, Copy, PartialEq, Eq)]
enum CmdKind {
    CreateBase,
    Search,
    UpdateBase,
    Delete,
    Discover,
    ConfigParams,
}


struct Done {
    result: CommandResult,
}


#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Bases,
    Search,
    Discover,
    Config,
}

impl Screen {
    fn label(self) -> &'static str {
        match self {
            Screen::Bases => "Bases",
            Screen::Search => "Search",
            Screen::Discover => "Discover",
            Screen::Config => "Config",
        }
    }

    const ALL: [Screen; 4] = [
        Screen::Bases,
        Screen::Search,
        Screen::Discover,
        Screen::Config,
    ];
}


struct LogtoolsApp {
    screen: Screen,

    event_tx: Sender<PresenterEvent>,
    event_rx: Receiver<PresenterEvent>,
    done_rx: Receiver<Done>,
    done_tx: Sender<Done>,

    worker: Option<(CmdKind, JoinHandle<()>)>,
    busy: bool,

    progress: Option<(u64, u64)>,
    message: Option<String>,
    log_lines: Vec<String>,
    last_error: Option<String>,

    confirm: Option<(String, Sender<bool>)>,

    bases: Vec<read_base_info::BaseInfo>,

    results_path: Option<String>,
    search_preview: Vec<String>,

    create_modal_open: bool,
    create_name: String,
    create_paths: String,
    create_from_file: bool,
    create_from_file_path: String,

    search_base: String,
    search_text: String,

    disc_root: String,
    disc_use_everything: bool,
    disc_skip_everything: bool,
    disc_es: String,

    cfg_everything_dll: String,
    cfg_confirm_delete: String,
}

impl LogtoolsApp {
    fn reload_bases(&mut self) {
        self.bases = read_base_info::list_bases().unwrap_or_default();
    }

    fn spawn_command(&mut self, kind: CmdKind, cmd: GuiCommand) {
        if self.busy {
            return;
        }

        let tx = self.event_tx.clone();
        let done_tx = self.done_tx.clone();

        let handle = std::thread::spawn(move || {
            let mut presenter = EguiPresenter { tx };

            let result = match cmd {
                GuiCommand::CreateBase {
                    name,
                    paths,
                    from_file,
                } => logtools_app::run_create_base(paths, from_file, name, &mut presenter),
                GuiCommand::Search { base, text } => {
                    logtools_app::run_search(base, text, &mut presenter)
                }
                GuiCommand::UpdateBase { name } => {
                    logtools_app::run_update_base(name, &mut presenter)
                }
                GuiCommand::Delete { name, force_yes } => {
                    logtools_app::run_delete(name, force_yes, &mut presenter)
                }
                GuiCommand::Discover {
                    root,
                    out,
                    es,
                    no_everything,
                } => logtools_app::run_discover(root, out, es, no_everything, &mut presenter),
                GuiCommand::ConfigParams { params } => {
                    let mut result = CommandResult::Ok;

                    for (param, var) in params {
                        result = logtools_app::run_config(Some(param), Some(var), &mut presenter);

                        if result != CommandResult::Ok {
                            break;
                        }
                    }

                    result
                }
            };

            let _ = done_tx.send(Done { result });
        });

        self.worker = Some((kind, handle));
        self.busy = true;
        self.progress = None;
        self.message = None;
        self.log_lines.clear();
        self.search_preview.clear();
        self.results_path = None;
        self.last_error = None;
    }

    fn pump_events(&mut self) {
        while let Ok(ev) = self.event_rx.try_recv() {
            match ev {
                PresenterEvent::Progress { pos, total } => self.progress = Some((pos, total)),
                PresenterEvent::Message(msg) => self.message = Some(msg),
                PresenterEvent::Log(line) => {
                    if let Some(path) = line.strip_prefix("Results file: ") {
                        self.results_path = Some(path.to_string());
                    } else {
                        self.log_lines.push(line);
                    }
                }
                PresenterEvent::Error(msg) => self.last_error = Some(msg),
                PresenterEvent::ConfirmRequest { prompt, reply_tx } => {
                    self.confirm = Some((prompt, reply_tx));
                    return;
                }
            }
        }
    }

    fn pump_done(&mut self) {
        let finished = self
            .worker
            .as_ref()
            .map(|(_, handle)| handle.is_finished())
            .unwrap_or(false);

        if finished {
            let (kind, handle) = self.worker.take().unwrap();
            let _ = handle.join();

            self.busy = false;
            self.progress = None;
            self.message = None;

            let done = self.done_rx.try_recv();

            match kind {
                CmdKind::CreateBase | CmdKind::UpdateBase | CmdKind::Delete => self.reload_bases(),
                CmdKind::Search | CmdKind::Discover => self.load_search_preview(),
                CmdKind::ConfigParams => {
                    self.cfg_everything_dll =
                        tools::config::get_config_param("everything_dll").unwrap_or_default();
                    self.cfg_confirm_delete =
                        tools::config::get_config_param("confirm_delete").unwrap_or_default();
                }
            }

            if let Ok(done) = done {
                if let CommandResult::ExitCode(code) = done.result {
                    if code != 0 && self.last_error.is_none() {
                        self.last_error = Some(format!("Command failed (exit code {})", code));
                    }
                }
            }
        }
    }

    fn load_search_preview(&mut self) {
        self.search_preview.clear();

        if let Some(path) = &self.results_path {
            if let Ok(content) = std::fs::read_to_string(path) {
                self.search_preview = content.lines().take(100).map(String::from).collect();
            }
        }
    }

    fn open_results_file(&self) {
        if let Some(path) = &self.results_path {
            open_in_system_app(Path::new(path));
        }
    }

    fn ui_operation_status(&self, ui: &mut egui::Ui) {
        if let Some((pos, total)) = self.progress {
            ui.add(
                egui::ProgressBar::new(if total > 0 {
                    pos as f32 / total as f32
                } else {
                    0.0
                })
                .text(format!("{}/{}", pos, total))
                .desired_width(400.0),
            );
        }

        if let Some(msg) = &self.message {
            ui.label(egui::RichText::new(msg).strong());
        }

        if !self.log_lines.is_empty() {
            let lines = &self.log_lines;

            egui::ScrollArea::vertical()
                .id_salt("status_log")
                .max_height(150.0)
                .auto_shrink(false)
                .show(ui, |ui| {
                    let start = lines.len().saturating_sub(200);

                    for line in &lines[start..] {
                        ui.monospace(line);
                    }
                });
        }
    }

    fn ui_bases(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Bases");
                ui.add_space(16.0);

                if ui.button("Refresh").clicked() {
                    self.reload_bases();
                }

                if ui.button("Create new base").clicked() {
                    self.create_modal_open = true;
                }
            });
        });

        ui.add_space(8.0);
        self.ui_operation_status(ui);
        ui.add_space(8.0);
        ui.separator();

        if self.bases.is_empty() {
            ui.label("No bases found.");
        } else {
            let bases = self.bases.clone();

            egui::ScrollArea::vertical()
                .id_salt("bases_table")
                .auto_shrink(false)
                .show(ui, |ui| {
                    egui::Grid::new("bases_grid").striped(true).show(ui, |ui| {
                        ui.strong("Name");
                        ui.strong("Lines");
                        ui.strong("Actions");
                        ui.end_row();

                        for base in &bases {
                            ui.label(base.name.clone());
                            ui.label(base.lines.to_string());

                            ui.horizontal(|ui| {
                                let name = base.name.clone();

                                if ui
                                    .add_enabled(!self.busy, egui::Button::new("Update"))
                                    .clicked()
                                {
                                    self.spawn_command(
                                        CmdKind::UpdateBase,
                                        GuiCommand::UpdateBase { name },
                                    );
                                }

                                let name = base.name.clone();

                                if ui
                                    .add_enabled(!self.busy, egui::Button::new("Delete"))
                                    .clicked()
                                {
                                    self.spawn_command(
                                        CmdKind::Delete,
                                        GuiCommand::Delete {
                                            name,
                                            force_yes: false,
                                        },
                                    );
                                }
                            });

                            ui.end_row();
                        }
                    });
                });
        }

        if let Some(path) = &self.results_path {
            ui.add_space(8.0);
            ui.label(format!("Results: {}", path));

            if ui.button("Open results file").clicked() {
                self.open_results_file();
            }
        }
    }

    fn ui_search(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.heading("Search");
            ui.add_space(8.0);

            let names: Vec<String> = self.bases.iter().map(|b| b.name.clone()).collect();

            if self.search_base.is_empty() {
                if let Some(first) = names.first() {
                    self.search_base = first.clone();
                }
            }

            egui::ComboBox::from_label("Base")
                .selected_text(self.search_base.clone())
                .show_ui(ui, |ui| {
                    for name in &names {
                        ui.selectable_value(&mut self.search_base, name.clone(), name.clone());
                    }
                });

            ui.horizontal(|ui| {
                ui.label("Text:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.search_text)
                        .desired_width(400.0)
                        .hint_text("supports & | - ! parens and <globs>"),
                );
            });

            let can_search = !self.search_base.is_empty() && !self.search_text.is_empty();

            if ui
                .add_enabled(can_search, egui::Button::new("Search"))
                .clicked()
            {
                self.spawn_command(
                    CmdKind::Search,
                    GuiCommand::Search {
                        base: self.search_base.clone(),
                        text: self.search_text.clone(),
                    },
                );
            }
        });

        ui.add_space(8.0);
        self.ui_operation_status(ui);

        let preview = self.search_preview.clone();

        if !preview.is_empty() {
            ui.add_space(8.0);
            ui.separator();

            ui.horizontal(|ui| {
                ui.heading("Results (first 100 lines)");

                if ui.button("Open results file").clicked() {
                    self.open_results_file();
                }
            });

            egui::ScrollArea::vertical()
                .id_salt("search_preview")
                .auto_shrink(false)
                .show(ui, |ui| {
                    for line in preview {
                        ui.monospace(line);
                    }
                });
        }
    }

    fn ui_discover(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.heading("Discover");
            ui.add_space(8.0);

            ui.label("Root directory:");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.disc_root)
                        .desired_width(400.0)
                        .hint_text("Empty = user home"),
                );

                if ui.button("Browse").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        self.disc_root = dir.to_string_lossy().to_string();
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.disc_use_everything, "Use Everything SDK/CLI");
                ui.checkbox(&mut self.disc_skip_everything, "Skip Everything");
            });

            ui.label("Path to Everything64.dll / es.exe:");
            ui.text_edit_singleline(&mut self.disc_es);

            ui.add_space(8.0);

            if ui.button("Discover").clicked() {
                let no_everything = self.disc_skip_everything || !self.disc_use_everything;

                let root = if self.disc_root.trim().is_empty() {
                    None
                } else {
                    Some(self.disc_root.trim().to_string())
                };

                let es = if self.disc_es.trim().is_empty() {
                    None
                } else {
                    Some(self.disc_es.trim().to_string())
                };

                self.spawn_command(
                    CmdKind::Discover,
                    GuiCommand::Discover {
                        root,
                        out: None,
                        es,
                        no_everything,
                    },
                );
            }
        });

        ui.add_space(8.0);
        self.ui_operation_status(ui);

        let preview = self.search_preview.clone();

        if !preview.is_empty() {
            ui.add_space(8.0);
            ui.separator();

            ui.horizontal(|ui| {
                ui.heading("Found log directories");

                if ui.button("Open results file").clicked() {
                    self.open_results_file();
                }
            });

            egui::ScrollArea::vertical()
                .id_salt("discover_preview")
                .auto_shrink(false)
                .show(ui, |ui| {
                    for line in preview {
                        ui.monospace(line);
                    }
                });
        }
    }

    fn ui_config(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.heading("Config");
            ui.add_space(8.0);

            egui::Grid::new("config_grid").striped(true).show(ui, |ui| {
                ui.strong("everything_dll");
                ui.add(
                    egui::TextEdit::singleline(&mut self.cfg_everything_dll).desired_width(360.0),
                );
                ui.end_row();

                ui.strong("confirm_delete");
                ui.add(
                    egui::TextEdit::singleline(&mut self.cfg_confirm_delete).desired_width(360.0),
                );
                ui.end_row();
            });

            ui.add_space(8.0);

            if ui.button("Save").clicked() {
                let params = vec![
                    (
                        "everything_dll".to_string(),
                        self.cfg_everything_dll.trim().to_string(),
                    ),
                    (
                        "confirm_delete".to_string(),
                        self.cfg_confirm_delete.trim().to_string(),
                    ),
                ];

                self.spawn_command(CmdKind::ConfigParams, GuiCommand::ConfigParams { params });
            }
        });

        ui.add_space(8.0);

        let map = tools::config::load_config();

        if map.is_empty() {
            ui.label("No config parameters set.");
        } else {
            for (k, v) in &map {
                ui.label(format!(
                    "{:<20} {}",
                    k,
                    v.as_str().unwrap_or(&v.to_string())
                ));
            }
        }
    }
}

impl eframe::App for LogtoolsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump_events();
        self.pump_done();

        if self.busy || self.confirm.is_some() {
            ctx.request_repaint();
        }

        if let Some(err) = self.last_error.clone() {
            egui::Modal::new(egui::Id::new("error_modal")).show(ctx, |ui| {
                ui.heading("Error");
                ui.colored_label(egui::Color32::RED, err);

                if ui.button("OK").clicked() {
                    self.last_error = None;
                }
            });
        }

        if let Some((prompt, reply_tx)) = self.confirm.clone() {
            egui::Modal::new(egui::Id::new("confirm_modal")).show(ctx, |ui| {
                ui.heading("Confirm");
                ui.label(prompt);
                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Yes").clicked() {
                        let _ = reply_tx.send(true);
                        self.confirm = None;
                    }

                    if ui.button("No").clicked() {
                        let _ = reply_tx.send(false);
                        self.confirm = None;
                    }
                });
            });
        }

        if self.create_modal_open {
            egui::Modal::new(egui::Id::new("create_modal")).show(ctx, |ui| {
                ui.heading("Create new base");
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut self.create_name);
                });

                ui.label("Paths (one per line):");

                egui::ScrollArea::vertical()
                    .id_salt("create_paths_scroll")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.create_paths).desired_width(420.0),
                        );
                    });

                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.create_from_file, "From file");

                    ui.add_enabled(
                        self.create_from_file,
                        egui::TextEdit::singleline(&mut self.create_from_file_path)
                            .desired_width(280.0)
                            .hint_text("path to file with dirs"),
                    );
                });

                ui.separator();

                ui.horizontal(|ui| {
                    let can_create =
                        !self.create_name.is_empty() && !self.create_paths.trim().is_empty();

                    if ui
                        .add_enabled(can_create, egui::Button::new("Create"))
                        .clicked()
                    {
                        let paths: Vec<String> = self
                            .create_paths
                            .lines()
                            .map(|l| l.trim().to_string())
                            .filter(|l| !l.is_empty())
                            .collect();

                        let from_file = if self.create_from_file {
                            Some(self.create_from_file_path.trim().to_string())
                        } else {
                            None
                        };

                        self.spawn_command(
                            CmdKind::CreateBase,
                            GuiCommand::CreateBase {
                                name: self.create_name.clone(),
                                paths,
                                from_file,
                            },
                        );

                        self.create_modal_open = false;
                        self.create_name.clear();
                        self.create_paths.clear();
                        self.create_from_file = false;
                        self.create_from_file_path.clear();
                    }

                    if ui.button("Cancel").clicked() {
                        self.create_modal_open = false;
                    }
                });
            });
        }

        egui::SidePanel::left("nav_panel")
            .exact_width(140.0)
            .show(ctx, |ui| {
                ui.add_space(16.0);
                ui.heading("logtools");
                ui.add_space(16.0);

                for screen in Screen::ALL {
                    let selected = self.screen == screen;

                    let response = ui.add_enabled(
                        !self.busy,
                        egui::Button::selectable(
                            selected,
                            egui::RichText::new(screen.label()).heading(),
                        ),
                    );

                    if response.clicked() {
                        self.screen = screen;
                    }

                    ui.add_space(8.0);
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| match self.screen {
            Screen::Bases => self.ui_bases(ui),
            Screen::Search => self.ui_search(ui),
            Screen::Discover => self.ui_discover(ui),
            Screen::Config => self.ui_config(ui),
        });
    }
}

// =========================
// MAIN
// =========================

fn main() -> eframe::Result<()> {
    let (event_tx, event_rx) = channel::<PresenterEvent>();
    let (done_tx, done_rx) = channel::<Done>();

    let mut app = LogtoolsApp {
        screen: Screen::Bases,
        event_tx,
        event_rx,
        done_rx,
        done_tx,
        worker: None,
        busy: false,
        progress: None,
        message: None,
        log_lines: Vec::new(),
        last_error: None,
        confirm: None,
        bases: Vec::new(),
        results_path: None,
        search_preview: Vec::new(),
        create_modal_open: false,
        create_name: String::new(),
        create_paths: String::new(),
        create_from_file: false,
        create_from_file_path: String::new(),
        search_base: String::new(),
        search_text: String::new(),
        disc_root: String::new(),
        disc_use_everything: true,
        disc_skip_everything: false,
        disc_es: String::new(),
        cfg_everything_dll: String::new(),
        cfg_confirm_delete: String::new(),
    };

    app.reload_bases();

    app.cfg_everything_dll = tools::config::get_config_param("everything_dll").unwrap_or_default();
    app.cfg_confirm_delete = tools::config::get_config_param("confirm_delete").unwrap_or_default();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("logtools"),
        ..Default::default()
    };

    eframe::run_native(
        "logtools",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(app))
        }),
    )
}

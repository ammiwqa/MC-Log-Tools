use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use eframe::egui;
use logtools_app::CommandResult;
use logtools_app::BaseInfo;

use crate::presenter::{EguiPresenter, PresenterEvent};
use crate::results_window::SearchResultsWindowState;
use crate::screens;
use crate::theme;

#[derive(Clone)]
pub enum GuiCommand {
    CreateBase {
        name: String,
        paths: Vec<String>,
        from_file: Option<String>,
    },
    Search {
        base: String,
        text: String,
        date_from: Option<i64>,
        date_to: Option<i64>,
        use_date_filtering: bool,
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
pub enum CmdKind {
    CreateBase,
    Search,
    UpdateBase,
    Delete,
    Discover,
    ConfigParams,
}

pub(crate) struct Done {
    result: CommandResult,
}

pub struct LogtoolsApp {
    pub screen: screens::Screen,

    pub event_tx: Sender<PresenterEvent>,
    pub event_rx: Receiver<PresenterEvent>,
    pub done_rx: Receiver<Done>,
    pub done_tx: Sender<Done>,

    pub worker: Option<(CmdKind, JoinHandle<()>)>,
    pub busy: bool,

    pub progress: Option<(u64, u64)>,
    pub message: Option<String>,
    pub log_lines: Vec<String>,
    pub last_error: Option<String>,

    pub confirm: Option<(String, Sender<bool>)>,

    pub bases: Vec<BaseInfo>,

    pub results_path: Option<String>,
    pub search_preview: Vec<String>,
    pub search_results: Vec<(u64, String)>,

    pub search_results_window: Option<Arc<Mutex<SearchResultsWindowState>>>,

    pub create_modal_open: bool,
    pub create_name: String,
    pub create_paths: String,
    pub create_from_file: bool,
    pub create_from_file_path: String,

    pub search_base: String,
    pub search_text: String,
    pub search_use_date_filtering: bool,
    pub search_date_from: String,
    pub search_date_to: String,

    pub disc_root: String,
    pub disc_use_everything: bool,
    pub disc_skip_everything: bool,
    pub disc_es: String,

    pub cfg_everything_dll: String,
    pub cfg_confirm_delete: String,
}

impl LogtoolsApp {
    pub fn new() -> Self {
        let (event_tx, event_rx) = channel::<PresenterEvent>();
        let (done_tx, done_rx) = channel::<Done>();

        let mut app = Self {
            screen: screens::Screen::Bases,
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
            search_results: Vec::new(),
            search_results_window: None,
            create_modal_open: false,
            create_name: String::new(),
            create_paths: String::new(),
            create_from_file: false,
            create_from_file_path: String::new(),
            search_base: String::new(),
            search_text: String::new(),
            search_use_date_filtering: false,
            search_date_from: String::new(),
            search_date_to: String::new(),
            disc_root: String::new(),
            disc_use_everything: true,
            disc_skip_everything: false,
            disc_es: String::new(),
            cfg_everything_dll: String::new(),
            cfg_confirm_delete: String::new(),
        };

        app.reload_bases();

        app.cfg_everything_dll =
            tools::config::get_config_param("everything_dll").unwrap_or_default();
        app.cfg_confirm_delete =
            tools::config::get_config_param("confirm_delete").unwrap_or_default();

        app.search_base = tools::config::get_config_param("last_search_base").unwrap_or_default();
        app.search_text = tools::config::get_config_param("last_search_text").unwrap_or_default();

        app
    }

    pub fn reload_bases(&mut self) {
        self.bases = logtools_app::operations::list_bases().unwrap_or_default();
    }

    pub fn spawn_command(&mut self, kind: CmdKind, cmd: GuiCommand) {
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
                } => crate::commands::run_create_base(paths, from_file, name, &mut presenter),
                GuiCommand::Search {
                    base,
                    text,
                    date_from,
                    date_to,
                    use_date_filtering,
                } => crate::commands::run_search(
                    base,
                    text,
                    date_from,
                    date_to,
                    use_date_filtering,
                    &mut presenter,
                ),
                GuiCommand::UpdateBase { name } => {
                    crate::commands::run_update_base(name, &mut presenter)
                }
                GuiCommand::Delete { name, force_yes } => {
                    crate::commands::run_delete(name, force_yes, &mut presenter)
                }
                GuiCommand::Discover {
                    root,
                    out,
                    es,
                    no_everything,
                } => crate::commands::run_discover(root, out, es, no_everything, &mut presenter),
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
        self.search_results.clear();
        self.results_path = None;
        self.last_error = None;
        self.search_results_window = None;
    }

    pub fn pump_events(&mut self) {
        while let Ok(ev) = self.event_rx.try_recv() {
            match ev {
                PresenterEvent::Progress { pos, total } => self.progress = Some((pos, total)),
                PresenterEvent::Message(msg) => self.message = Some(msg),
                PresenterEvent::Log(line) => {
                    if let Some(path) = line.trim_start().strip_prefix("Results file: ") {
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

    pub fn pump_done(&mut self) {
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
                CmdKind::Search => {
                    self.load_search_preview();
                    self.load_search_results();
                    self.search_results_window =
                        Some(Arc::new(Mutex::new(SearchResultsWindowState::new(
                            self.search_base.clone(),
                            self.search_text.clone(),
                            Arc::new(self.search_results.clone()),
                            self.results_path.clone(),
                        ))));
                }
                CmdKind::Discover => self.load_search_preview(),
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

    pub fn load_search_preview(&mut self) {
        self.search_preview.clear();

        if let Some(path) = &self.results_path {
            if let Ok(content) = std::fs::read_to_string(path) {
                self.search_preview = content.lines().take(100).map(String::from).collect();
            }
        }
    }

    pub fn load_search_results(&mut self) {
        self.search_results.clear();

        if let Some(path) = &self.results_path {
            if let Ok(content) = std::fs::read_to_string(path) {
                for line in content.lines() {
                    if let Some((unix, text)) =
                        crate::results_window::parse_search_result_line(line)
                    {
                        self.search_results.push((unix, text));
                    }
                }
            }
        }

        if let Some(state) = &self.search_results_window {
            let mut s = state.lock().unwrap();
            s.search_results = Arc::new(self.search_results.clone());
            s.search_base = self.search_base.clone();
            s.search_text = self.search_text.clone();
            s.results_path = self.results_path.clone();
        }
    }

    pub fn base_date_bounds(&self) -> (Option<i64>, Option<i64>) {
        logtools_app::operations::load_log(&self.search_base)
            .ok()
            .and_then(|cfg| {
                let f = cfg.time.fld_unix.parse::<i64>().ok();
                let l = cfg.time.lld_unix.parse::<i64>().ok();
                Some((f, l))
            })
            .unwrap_or_default()
    }

    pub fn open_results_file(&self) {
        if let Some(path) = &self.results_path {
            crate::theme::open_in_system_app(Path::new(path));
        }
    }

    pub fn has_status_content(&self) -> bool {
        self.progress.is_some() || self.message.is_some() || !self.log_lines.is_empty()
    }

    pub fn render_create_modal(&mut self, ctx: &egui::Context) {
        if !self.create_modal_open {
            return;
        }

        egui::Modal::new(egui::Id::new("create_modal")).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("//")
                        .color(theme::GREEN)
                        .heading()
                        .strong(),
                );
                ui.heading("Create new base");
            });
            ui.add_space(4.0);
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Name:");
                ui.add(egui::TextEdit::singleline(&mut self.create_name).desired_width(340.0));
            });

            ui.label(theme::muted("Paths (one per line):"));

            egui::ScrollArea::vertical()
                .id_salt("create_paths_scroll")
                .max_height(120.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.create_paths)
                            .desired_width(440.0)
                            .hint_text("one path per line; leave empty when using a file"),
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
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                let can_create = !self.create_name.is_empty()
                    && (!self.create_paths.trim().is_empty()
                        || (self.create_from_file
                            && !self.create_from_file_path.trim().is_empty()));

                if ui
                    .add_enabled(can_create, theme::primary_button("Create"))
                    .clicked()
                {
                    let paths: Vec<String> = self
                        .create_paths
                        .lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty())
                        .collect();

                    let from_file = if self.create_from_file {
                        Some(
                            crate::theme::strip_path_quotes(self.create_from_file_path.trim())
                                .to_string(),
                        )
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

                if ui.add(theme::ghost_button("Cancel")).clicked() {
                    self.create_modal_open = false;
                }
            });
        });
    }

    pub fn render_error_modal(&mut self, ctx: &egui::Context) {
        if let Some(err) = self.last_error.clone() {
            egui::Modal::new(egui::Id::new("error_modal")).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("//")
                            .color(theme::RED)
                            .heading()
                            .strong(),
                    );
                    ui.heading("Error");
                });
                ui.add_space(4.0);
                ui.colored_label(theme::RED, err);
                ui.add_space(8.0);

                if ui.add(theme::ghost_button("OK")).clicked() {
                    self.last_error = None;
                }
            });
        }
    }

    pub fn render_confirm_modal(&mut self, ctx: &egui::Context) {
        if let Some((prompt, reply_tx)) = self.confirm.clone() {
            egui::Modal::new(egui::Id::new("confirm_modal")).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("//")
                            .color(theme::AMBER)
                            .heading()
                            .strong(),
                    );
                    ui.heading("Confirm");
                });
                ui.add_space(4.0);
                ui.label(prompt);
                ui.separator();
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    if ui.add(theme::primary_button("Yes")).clicked() {
                        let _ = reply_tx.send(true);
                        self.confirm = None;
                    }

                    if ui.add(theme::ghost_button("No")).clicked() {
                        let _ = reply_tx.send(false);
                        self.confirm = None;
                    }
                });
            });
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

        self.render_error_modal(ctx);
        self.render_confirm_modal(ctx);
        self.render_create_modal(ctx);

        egui::SidePanel::left("nav_panel")
            .exact_width(190.0)
            .show(ctx, |ui| {
                screens::render_nav_panel(self, ui);
            });

        screens::render_status_panel(self, ctx);

        egui::CentralPanel::default().show(ctx, |ui| match self.screen {
            screens::Screen::Bases => screens::bases::render(ui, self),
            screens::Screen::Search => screens::search::render(ui, self),
            screens::Screen::Discover => screens::discover::render(ui, self),
            screens::Screen::Config => screens::config::render(ui, self),
        });

        crate::results_window::render(ctx, self);
    }
}

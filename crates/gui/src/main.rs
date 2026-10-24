//! GUI на egui/eframe.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use eframe::egui;
use logtools_app::{CommandResult, Presenter};

// =========================
// THEME
// =========================

mod theme {
    use crate::egui::Color32;

    pub const BG: Color32 = Color32::from_rgb(0x09, 0x09, 0x0A);
    pub const PANEL: Color32 = Color32::from_rgb(0x0E, 0x0E, 0x10);
    pub const SURFACE: Color32 = Color32::from_rgb(0x16, 0x16, 0x19);
    pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x22);
    pub const SURFACE_ACTIVE: Color32 = Color32::from_rgb(0x27, 0x27, 0x2C);

    pub const BORDER: Color32 = Color32::from_rgb(0x26, 0x26, 0x2B);
    pub const BORDER_HOVER: Color32 = Color32::from_rgb(0x3B, 0x3B, 0x42);

    pub const TEXT: Color32 = Color32::from_rgb(0xEB, 0xEB, 0xE7);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x8B, 0x8B, 0x86);
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x5E, 0x5E, 0x5A);

    pub const GREEN: Color32 = Color32::from_rgb(0x0F, 0xDB, 0x92);
    pub const GREEN_DIM: Color32 = Color32::from_rgb(0x0B, 0x93, 0x66);
    pub const ON_GREEN: Color32 = Color32::from_rgb(0x05, 0x0D, 0x0A);

    pub const LOG_KEYWORD: Color32 = Color32::from_rgb(0x22, 0xAF, 0x77);

    pub const RED: Color32 = Color32::from_rgb(0xEC, 0x6A, 0x88);
    pub const AMBER: Color32 = Color32::from_rgb(0xE0, 0xAF, 0x5F);
}

fn stroke_1(color: egui::Color32) -> egui::Stroke {
    egui::Stroke::new(1.0_f32, color)
}

fn muted(text: &str) -> egui::RichText {
    egui::RichText::new(text).color(theme::TEXT_MUTED)
}

fn faint(text: &str) -> egui::RichText {
    egui::RichText::new(text).color(theme::TEXT_FAINT)
}

fn primary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).color(theme::ON_GREEN).strong())
        .fill(theme::GREEN)
        .min_size(egui::vec2(0.0, 30.0))
}

fn ghost_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text))
        .fill(theme::SURFACE)
        .stroke(stroke_1(theme::BORDER))
        .min_size(egui::vec2(0.0, 30.0))
}

fn danger_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).color(theme::RED))
        .fill(egui::Color32::TRANSPARENT)
        .stroke(stroke_1(theme::BORDER))
        .min_size(egui::vec2(0.0, 24.0))
}

fn small_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).size(12.0))
        .fill(theme::SURFACE)
        .stroke(stroke_1(theme::BORDER))
        .min_size(egui::vec2(0.0, 24.0))
}

fn screen_title(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("//")
                .color(theme::GREEN)
                .heading()
                .strong(),
        );
        ui.heading(egui::RichText::new(title));
    });
    ui.label(muted(subtitle).small());
    ui.add_space(8.0);
}

fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .monospace()
            .small()
            .color(theme::TEXT_FAINT),
    );
}

fn status_dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
}

fn zed_progress_bar(ui: &mut egui::Ui, fraction: f32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());

    ui.painter().rect_filled(rect, 3.0, theme::SURFACE_ACTIVE);

    let width = (rect.width() * fraction).min(rect.width());

    if width > 1.0 {
        let bar = egui::Rect::from_min_size(rect.left_top(), egui::vec2(width, rect.height()));
        let radius = 3.0_f32.min(width * 0.5);
        ui.painter().rect_filled(bar, radius, theme::GREEN);
    }
}

fn log_line_color(line: &str) -> egui::Color32 {
    let lower = line.to_lowercase();

    if lower.contains("error") || lower.contains("failed") {
        theme::RED
    } else if lower.contains("warn") {
        theme::AMBER
    } else {
        theme::TEXT_MUTED
    }
}

fn log_line_job(ui: &egui::Ui, line: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();

    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let color = log_line_color(line);
    let is_alert = color == theme::RED || color == theme::AMBER;

    let (first, rest) = match line.find(char::is_whitespace) {
        Some(idx) => (&line[..idx], &line[idx..]),
        None => (line, ""),
    };

    if is_alert || first.is_empty() {
        job.append(line, 0.0, egui::TextFormat::simple(font, color));
    } else {
        job.append(
            first,
            0.0,
            egui::TextFormat::simple(font.clone(), theme::LOG_KEYWORD),
        );
        job.append(rest, 0.0, egui::TextFormat::simple(font, color));
    }

    job
}

fn install_fonts(ctx: &egui::Context) {
    let candidates: &[&str] = &[
        // Windows
        r"C:\Windows\Fonts\CascadiaMono.ttf",
        r"C:\Windows\Fonts\cascadiamono.ttf",
        r"C:\Windows\Fonts\consola.ttf",
        // macOS
        "/System/Library/Fonts/Menlo.ttc",
        "/System/Library/Fonts/SFMono-Regular.otf",
        "/Library/Fonts/JetBrainsMono-Regular.ttf",
        // Linux
        "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf",
        "/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf",
        "/usr/share/fonts/truetype/cascadia/CascadiaCode-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    ];

    let mut fonts = egui::FontDefinitions::default();

    for path in candidates {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(
                "zed_mono".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(data)),
            );

            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                family.insert(0, "zed_mono".to_owned());
            }

            break;
        }
    }

    ctx.set_fonts(fonts);
}

fn configure_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = theme::PANEL;
    visuals.window_fill = theme::PANEL;
    visuals.extreme_bg_color = theme::BG;
    visuals.text_edit_bg_color = Some(theme::BG);
    visuals.faint_bg_color = theme::SURFACE;
    visuals.window_stroke = stroke_1(theme::BORDER);
    visuals.hyperlink_color = theme::GREEN;
    visuals.weak_text_color = Some(theme::TEXT_MUTED);
    visuals.warn_fg_color = theme::AMBER;
    visuals.error_fg_color = theme::RED;

    visuals.selection.bg_fill = theme::GREEN_DIM;
    visuals.selection.stroke = stroke_1(theme::GREEN);

    let w = &mut visuals.widgets;

    w.noninteractive.bg_fill = theme::PANEL;
    w.noninteractive.bg_stroke = stroke_1(theme::BORDER);
    w.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, theme::TEXT);

    w.inactive.bg_fill = theme::SURFACE;
    w.inactive.bg_stroke = stroke_1(theme::BORDER);
    w.inactive.fg_stroke = egui::Stroke::new(1.0_f32, theme::TEXT);

    w.hovered.bg_fill = theme::SURFACE_HOVER;
    w.hovered.bg_stroke = stroke_1(theme::BORDER_HOVER);
    w.hovered.fg_stroke = egui::Stroke::new(1.0_f32, theme::TEXT);

    w.active.bg_fill = theme::SURFACE_ACTIVE;
    w.active.bg_stroke = stroke_1(theme::GREEN_DIM);
    w.active.fg_stroke = egui::Stroke::new(1.0_f32, theme::TEXT);

    w.open.bg_fill = theme::SURFACE;
    w.open.bg_stroke = stroke_1(theme::BORDER);
    w.open.fg_stroke = egui::Stroke::new(1.0_f32, theme::TEXT);

    let mut style = egui::Style::default();
    style.visuals = visuals;
    style.text_styles = [
        (egui::TextStyle::Heading, egui::FontId::monospace(20.0)),
        (egui::TextStyle::Body, egui::FontId::proportional(13.5)),
        (egui::TextStyle::Button, egui::FontId::monospace(13.0)),
        (egui::TextStyle::Small, egui::FontId::proportional(10.5)),
        (egui::TextStyle::Monospace, egui::FontId::monospace(12.5)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(14.0, 6.0);

    ctx.set_style(style);
}

// =========================
// PRESENTER
// =========================

enum PresenterEvent {
    Progress {
        pos: u64,
        total: u64,
    },
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

    fn subtitle(self) -> &'static str {
        match self {
            Screen::Bases => "Manage indexed log bases",
            Screen::Search => "Search across indexed log bases",
            Screen::Discover => "Find log directories on disk",
            Screen::Config => "Global tool settings",
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

    fn has_status_content(&self) -> bool {
        self.progress.is_some() || self.message.is_some() || !self.log_lines.is_empty()
    }

    fn show_status_panel(&self, ctx: &egui::Context) {
        if !self.has_status_content() {
            return;
        }

        egui::TopBottomPanel::bottom("status_panel").show(ctx, |ui| {
            ui.add_space(4.0);

            if self.message.is_some() || self.progress.is_some() {
                ui.horizontal(|ui| {
                    if self.busy {
                        ui.add(egui::Spinner::new().size(14.0));
                        ui.add_space(2.0);
                    }

                    if let Some(msg) = &self.message {
                        ui.label(egui::RichText::new(msg).monospace().strong());
                    }

                    if let Some((pos, total)) = self.progress {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format!("{} / {}", pos, total))
                                    .monospace()
                                    .color(theme::TEXT_MUTED),
                            );
                        });
                    }
                });

                ui.add_space(4.0);
            }

            if let Some((pos, total)) = self.progress {
                let fraction = if total > 0 {
                    (pos as f32 / total as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                zed_progress_bar(ui, fraction);
            }

            if !self.log_lines.is_empty() {
                let lines = &self.log_lines;

                ui.add_space(4.0);

                ui.group(|ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("status_log")
                        .max_height(180.0)
                        .auto_shrink(false)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            let start = lines.len().saturating_sub(200);

                            for line in &lines[start..] {
                                ui.add(egui::Label::new(log_line_job(ui, line)));
                            }
                        });
                });
            }

            ui.add_space(4.0);
        });
    }

    fn ui_bases(&mut self, ui: &mut egui::Ui) {
        screen_title(ui, "Bases", Screen::Bases.subtitle());

        ui.add_enabled_ui(!self.busy, |ui| {
            ui.horizontal(|ui| {
                if ui.add(ghost_button("Refresh")).clicked() {
                    self.reload_bases();
                }

                if ui.add(primary_button("Create new base")).clicked() {
                    self.create_modal_open = true;
                }
            });
        });

        ui.add_space(8.0);
        ui.separator();

        if self.bases.is_empty() {
            ui.group(|ui| {
                ui.set_min_size(egui::vec2(ui.available_width(), 90.0));

                ui.centered_and_justified(|ui| {
                    ui.label(muted("No bases found. Create one to start searching."));
                });
            });
        } else {
            let bases = self.bases.clone();

            egui::ScrollArea::vertical()
                .id_salt("bases_table")
                .auto_shrink(false)
                .show(ui, |ui| {
                    egui::Grid::new("bases_grid").striped(true).show(ui, |ui| {
                        section_label(ui, "NAME");
                        section_label(ui, "LINES");
                        section_label(ui, "ACTIONS");
                        ui.end_row();

                        for base in &bases {
                            ui.label(egui::RichText::new(base.name.clone()).monospace());
                            ui.label(muted(&base.lines.to_string()).monospace());

                            let name = base.name.clone();

                            if ui.add_enabled(!self.busy, small_button("Update")).clicked() {
                                self.spawn_command(
                                    CmdKind::UpdateBase,
                                    GuiCommand::UpdateBase { name },
                                );
                            }

                            let name = base.name.clone();

                            if ui
                                .add_enabled(!self.busy, danger_button("Delete"))
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

                            ui.end_row();
                        }
                    });
                });
        }

        if let Some(path) = &self.results_path {
            ui.add_space(8.0);
            ui.separator();

            ui.horizontal(|ui| {
                ui.label(muted("Results file:").monospace());
                ui.label(egui::RichText::new(path.as_str()).monospace());

                if ui.add(ghost_button("Open results file")).clicked() {
                    self.open_results_file();
                }
            });
        }
    }

    fn ui_search(&mut self, ui: &mut egui::Ui) {
        screen_title(ui, "Search", Screen::Search.subtitle());

        ui.add_enabled_ui(!self.busy, |ui| {
            let names: Vec<String> = self.bases.iter().map(|b| b.name.clone()).collect();

            if self.search_base.is_empty() {
                if let Some(first) = names.first() {
                    self.search_base = first.clone();
                }
            } else if !names.contains(&self.search_base) {
                self.search_base = names.first().cloned().unwrap_or_default();
            }

            ui.horizontal(|ui| {
                ui.label(muted("Base:"));

                egui::ComboBox::from_id_salt("search_base_combo")
                    .width(320.0)
                    .selected_text(egui::RichText::new(self.search_base.as_str()).monospace())
                    .show_ui(ui, |ui| {
                        for name in &names {
                            ui.selectable_value(&mut self.search_base, name.clone(), name.clone());
                        }
                    });
            });

            ui.add_space(6.0);

            ui.add(
                egui::TextEdit::singleline(&mut self.search_text)
                    .desired_width(ui.available_width())
                    .hint_text("supports & | - ! parens and <globs>"),
            );

            ui.add_space(6.0);

            let can_search = !self.search_base.is_empty() && !self.search_text.is_empty();

            if ui
                .add_enabled(can_search, primary_button("Search"))
                .clicked()
            {
                let _ = tools::config::save_config_param("last_search_base", &self.search_base);
                let _ = tools::config::save_config_param("last_search_text", &self.search_text);

                self.spawn_command(
                    CmdKind::Search,
                    GuiCommand::Search {
                        base: self.search_base.clone(),
                        text: self.search_text.clone(),
                    },
                );
            }
        });

        let preview = self.search_preview.clone();

        if !preview.is_empty() {
            ui.add_space(8.0);
            ui.separator();

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("//")
                        .color(theme::GREEN)
                        .heading()
                        .strong(),
                );
                ui.heading("Results");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(muted("first 100 lines").small());

                    if ui.add(ghost_button("Open results file")).clicked() {
                        self.open_results_file();
                    }
                });
            });

            egui::ScrollArea::vertical()
                .id_salt("search_preview")
                .auto_shrink(false)
                .show(ui, |ui| {
                    for line in preview {
                        ui.monospace(
                            egui::RichText::new(line.as_str()).color(log_line_color(&line)),
                        );
                    }
                });
        }
    }

    fn ui_discover(&mut self, ui: &mut egui::Ui) {
        screen_title(ui, "Discover", Screen::Discover.subtitle());

        ui.add_enabled_ui(!self.busy, |ui| {
            ui.label(muted("Root directory:"));

            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.disc_root)
                        .desired_width(420.0)
                        .hint_text("Empty = user home"),
                );

                if ui.add(ghost_button("Browse")).clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        self.disc_root = dir.to_string_lossy().to_string();
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.disc_use_everything, "Use Everything SDK/CLI");
                ui.checkbox(&mut self.disc_skip_everything, "Skip Everything");
            });

            ui.label(muted("Path to Everything64.dll / es.exe:"));
            ui.add(egui::TextEdit::singleline(&mut self.disc_es).desired_width(420.0));

            ui.add_space(8.0);

            if ui.add(primary_button("Discover")).clicked() {
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

        let preview = self.search_preview.clone();

        if !preview.is_empty() {
            ui.add_space(8.0);
            ui.separator();

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("//")
                        .color(theme::GREEN)
                        .heading()
                        .strong(),
                );
                ui.heading("Found log directories");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(ghost_button("Open results file")).clicked() {
                        self.open_results_file();
                    }
                });
            });

            egui::ScrollArea::vertical()
                .id_salt("discover_preview")
                .auto_shrink(false)
                .show(ui, |ui| {
                    for line in preview {
                        ui.monospace(
                            egui::RichText::new(line.as_str()).color(log_line_color(&line)),
                        );
                    }
                });
        }
    }

    fn ui_config(&mut self, ui: &mut egui::Ui) {
        screen_title(ui, "Config", Screen::Config.subtitle());

        ui.add_enabled_ui(!self.busy, |ui| {
            section_label(ui, "everything_dll");

            ui.add(
                egui::TextEdit::singleline(&mut self.cfg_everything_dll)
                    .desired_width(ui.available_width()),
            );

            ui.add_space(8.0);

            section_label(ui, "confirm_delete");

            ui.add(
                egui::TextEdit::singleline(&mut self.cfg_confirm_delete)
                    .desired_width(ui.available_width()),
            );

            ui.add_space(8.0);

            if ui.add(primary_button("Save")).clicked() {
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
        ui.separator();

        let map = tools::config::load_config();

        if map.is_empty() {
            ui.label(muted("No config parameters set."));
        } else {
            section_label(ui, "CURRENT VALUES");

            for (k, v) in &map {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{:<20}", k))
                            .monospace()
                            .color(theme::GREEN),
                    );
                    ui.label(
                        egui::RichText::new(v.as_str().unwrap_or(&v.to_string()).to_string())
                            .monospace(),
                    );
                });
            }
        }
    }
}

// =========================
// APP / LAYOUT
// =========================

impl eframe::App for LogtoolsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump_events();
        self.pump_done();

        if self.busy || self.confirm.is_some() {
            ctx.request_repaint();
        }

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

                if ui.add(ghost_button("OK")).clicked() {
                    self.last_error = None;
                }
            });
        }

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
                    if ui.add(primary_button("Yes")).clicked() {
                        let _ = reply_tx.send(true);
                        self.confirm = None;
                    }

                    if ui.add(ghost_button("No")).clicked() {
                        let _ = reply_tx.send(false);
                        self.confirm = None;
                    }
                });
            });
        }

        if self.create_modal_open {
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

                ui.label(muted("Paths (one per line):"));

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
                        .add_enabled(can_create, primary_button("Create"))
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

                    if ui.add(ghost_button("Cancel")).clicked() {
                        self.create_modal_open = false;
                    }
                });
            });
        }

        egui::SidePanel::left("nav_panel")
            .exact_width(190.0)
            .show(ctx, |ui| {
                ui.add_space(18.0);

                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 3.0, theme::GREEN);

                    ui.label(
                        egui::RichText::new("LogTools")
                            .monospace()
                            .strong()
                            .size(18.0),
                    );
                });
                ui.label(faint("MC log search toolkit").small());
                ui.add_space(20.0);

                for screen in Screen::ALL {
                    let selected = self.screen == screen;

                    let text = egui::RichText::new(screen.label())
                        .monospace()
                        .size(15.0)
                        .color(if selected {
                            theme::TEXT
                        } else {
                            theme::TEXT_MUTED
                        });

                    let button = egui::Button::selectable(selected, text)
                        .fill(if selected {
                            theme::SURFACE
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .stroke(if selected {
                            stroke_1(theme::BORDER)
                        } else {
                            egui::Stroke::NONE
                        })
                        .min_size(egui::vec2(ui.available_width(), 34.0));

                    let response = ui.add_enabled(!self.busy, button);

                    if selected {
                        let rect = response.rect;
                        let bar = egui::Rect::from_min_max(
                            egui::pos2(rect.left(), rect.top() + 8.0),
                            egui::pos2(rect.left() + 2.0, rect.bottom() - 8.0),
                        );
                        ui.painter().rect_filled(bar, 1.0, theme::GREEN);
                    }

                    if response.clicked() {
                        self.screen = screen;
                    }

                    ui.add_space(4.0);
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.horizontal(|ui| {
                        status_dot(
                            ui,
                            if self.busy {
                                theme::AMBER
                            } else {
                                theme::GREEN
                            },
                        );

                        ui.label(
                            egui::RichText::new(if self.busy { "working..." } else { "ready" })
                                .monospace()
                                .small()
                                .color(theme::TEXT_FAINT),
                        );
                    });

                    ui.add_space(6.0);
                });
            });

        self.show_status_panel(ctx);

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

    app.search_base = tools::config::get_config_param("last_search_base").unwrap_or_default();
    app.search_text = tools::config::get_config_param("last_search_text").unwrap_or_default();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([840.0, 560.0])
            .with_title("LogTools"),
        ..Default::default()
    };

    eframe::run_native(
        "LogTools",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            configure_style(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

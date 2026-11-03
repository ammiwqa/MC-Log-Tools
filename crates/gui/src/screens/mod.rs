use crate::theme;
use eframe::egui;

pub mod bases;
pub mod config;
pub mod discover;
pub mod search;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Bases,
    Search,
    Discover,
    Config,
}

impl Screen {
    pub fn label(self) -> &'static str {
        match self {
            Screen::Bases => "Bases",
            Screen::Search => "Search",
            Screen::Discover => "Discover",
            Screen::Config => "Config",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Screen::Bases => "Manage indexed log bases",
            Screen::Search => "Search across indexed log bases",
            Screen::Discover => "Find log directories on disk",
            Screen::Config => "Global tool settings",
        }
    }

    pub const ALL: [Screen; 4] = [
        Screen::Bases,
        Screen::Search,
        Screen::Discover,
        Screen::Config,
    ];
}

pub fn render_nav_panel(
    app: &mut crate::app::LogtoolsApp,
    ui: &mut eframe::egui::Ui,
) {
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
    ui.label(crate::theme::faint("MC log search toolkit").small());
    ui.add_space(20.0);

    for screen in Screen::ALL {
        let selected = app.screen == screen;

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
                theme::stroke_1(theme::BORDER)
            } else {
                egui::Stroke::NONE
            })
            .min_size(egui::vec2(ui.available_width(), 34.0));

        let response = ui.add_enabled(!app.busy, button);

        if selected {
            let rect = response.rect;
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top() + 8.0),
                egui::pos2(rect.left() + 2.0, rect.bottom() - 8.0),
            );
            ui.painter().rect_filled(bar, 1.0, theme::GREEN);
        }

        if response.clicked() {
            app.screen = screen;
        }

        ui.add_space(4.0);
    }

    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.horizontal(|ui| {
            theme::status_dot(
                ui,
                if app.busy {
                    theme::AMBER
                } else {
                    theme::GREEN
                },
            );

            ui.label(
                egui::RichText::new(if app.busy { "working..." } else { "ready" })
                    .monospace()
                    .small()
                    .color(theme::TEXT_FAINT),
            );
        });

        ui.add_space(6.0);
    });
}

pub fn render_status_panel(app: &mut crate::app::LogtoolsApp, ctx: &eframe::egui::Context) {
    if !app.has_status_content() {
        return;
    }

    egui::TopBottomPanel::bottom("status_panel").show(ctx, |ui| {
        ui.add_space(4.0);

        if app.message.is_some() || app.progress.is_some() {
            ui.horizontal(|ui| {
                if app.busy {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.add_space(2.0);
                }

                if let Some(msg) = &app.message {
                    ui.label(egui::RichText::new(msg).monospace().strong());
                }

                if let Some((pos, total)) = app.progress {
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

        if let Some((pos, total)) = app.progress {
            let fraction = if total > 0 {
                (pos as f32 / total as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };

            theme::zed_progress_bar(ui, fraction);
        }

        if !app.log_lines.is_empty() {
            let lines = &app.log_lines;

            ui.add_space(4.0);

            ui.group(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt("status_log")
                    .auto_shrink(false)
                    .max_height(180.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        let start = lines.len().saturating_sub(200);

                        for line in &lines[start..] {
                            ui.add(egui::Label::new(crate::theme::log_line_job(ui, line)));
                        }
                    });
            });
        }

        ui.add_space(4.0);
    });
}

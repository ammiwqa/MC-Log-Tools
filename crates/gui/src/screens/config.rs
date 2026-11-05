use crate::app::LogtoolsApp;
use crate::theme;
use eframe::egui;

const COLUMN_MAX_WIDTH: f32 = 640.0;

const COLUMN_INDENT: f32 = 12.0;

const INPUT_MARGIN_Y: f32 = 6.0;
const INPUT_MARGIN_X: f32 = 10.0;

fn input_width(ui: &egui::Ui) -> f32 {
    ui.available_width()
        .min(COLUMN_MAX_WIDTH)
        .max(240.0)
        .min(ui.available_width())
}

fn draw_separator(ui: &mut egui::Ui) {
    let width = input_width(ui);
    if width <= 0.0 {
        return;
    }
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width + COLUMN_INDENT, 1.0), egui::Sense::hover());
    let line_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + COLUMN_INDENT, rect.top()),
        rect.max,
    );
    ui.painter().rect_filled(line_rect, 0.0, theme::BORDER);
}

pub fn render(ui: &mut egui::Ui, app: &mut LogtoolsApp) {
    theme::screen_title(ui, "Config", crate::screens::Screen::Config.subtitle());

    ui.add_enabled_ui(!app.busy, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let defs = theme::config_param_defs();

                for (idx, def) in defs.iter().enumerate() {
                    if idx > 0 {
                        ui.add_space(8.0);
                        draw_separator(ui);
                        ui.add_space(8.0);
                    }

                    render_param_block(ui, app, def);
                }

                ui.add_space(10.0);
                draw_separator(ui);
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.add_space(COLUMN_INDENT);

                    if ui.add(theme::primary_button("Save")).clicked() {
                        let params = vec![
                            (
                                "everything_dll".to_string(),
                                app.cfg_everything_dll.trim().to_string(),
                            ),
                            (
                                "confirm_delete".to_string(),
                                app.cfg_confirm_delete.trim().to_string(),
                            ),
                            (
                                "chunk_lines".to_string(),
                                app.cfg_chunk_lines.trim().to_string(),
                            ),
                            (
                                "sort_run_lines".to_string(),
                                app.cfg_sort_run_lines.trim().to_string(),
                            ),
                        ];

                        app.spawn_command(
                            crate::app::CmdKind::ConfigParams,
                            crate::app::GuiCommand::ConfigParams { params },
                        );
                    }

                    ui.add_space(4.0);
                    ui.label(
                        theme::faint("Reset returns a parameter to its default value.").small(),
                    );
                });
            });
    });

    ui.add_space(14.0);
    draw_separator(ui);
    ui.add_space(8.0);

    let map = tools::config::load_config();
    let state = tools::state::load_state();

    egui::CollapsingHeader::new(
        egui::RichText::new("Current Config Values")
            .monospace()
            .small()
            .color(theme::TEXT_MUTED),
    )
    .default_open(false)
    .show(ui, |ui| {
        if map.is_empty() {
            ui.label(theme::muted("No config parameters set."));
        } else {
            for (k, v) in &map {
                ui.horizontal(|ui| {
                    ui.add_space(COLUMN_INDENT);
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
    });

    egui::CollapsingHeader::new(
        egui::RichText::new("App State")
            .monospace()
            .small()
            .color(theme::TEXT_MUTED),
    )
    .default_open(false)
    .show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(COLUMN_INDENT);
            ui.label(theme::muted(
                "Internal memory of the app (not a config, edited automatically).",
            ));
        });
        ui.add_space(4.0);

        if state.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(COLUMN_INDENT);
                ui.label(theme::muted("No app state stored."));
            });
        } else {
            for (k, v) in &state {
                ui.horizontal(|ui| {
                    ui.add_space(COLUMN_INDENT);
                    ui.label(
                        egui::RichText::new(format!("{:<20}", k))
                            .monospace()
                            .color(theme::BLUE),
                    );
                    ui.label(
                        egui::RichText::new(v.as_str().unwrap_or(&v.to_string()).to_string())
                            .monospace(),
                    );
                });
            }
        }
    });
}

fn render_param_block(ui: &mut egui::Ui, app: &mut LogtoolsApp, def: &theme::ConfigParamDef) {
    let current_value: &mut String = match def.key {
        "everything_dll" => &mut app.cfg_everything_dll,
        "chunk_lines" => &mut app.cfg_chunk_lines,
        "sort_run_lines" => &mut app.cfg_sort_run_lines,
        "confirm_delete" => &mut app.cfg_confirm_delete,
        _ => return,
    };

    let is_modified = !def.is_default(current_value);

    ui.vertical(|ui| {
        ui.add_space(1.0);

        ui.horizontal(|ui| {
            ui.add_space(COLUMN_INDENT);

            ui.label(
                egui::RichText::new(def.display_name)
                    .size(17.0)
                    .strong()
                    .color(theme::TEXT),
            );

            let size = egui::vec2(18.0, 18.0);

            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

            ui.painter().rect(
                rect,
                egui::CornerRadius::same(3),
                theme::SURFACE,
                theme::stroke_1(theme::BORDER),
                egui::StrokeKind::Inside,
            );

            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "\u{21BA}",
                egui::FontId::proportional(9.0),
                if is_modified {
                    theme::TEXT
                } else {
                    theme::TEXT_FAINT
                },
            );

            if response.hovered() {
                response.clone().on_hover_text(if is_modified {
                    "Reset to default"
                } else {
                    "Already at default"
                });
            }

            if response.clicked() && is_modified {
                current_value.clear();
            }
        });

        ui.horizontal(|ui| {
            ui.add_space(COLUMN_INDENT);
            ui.label(
                egui::RichText::new(def.description)
                    .size(12.0)
                    .color(theme::TEXT_MUTED),
            );
        });

        ui.add_space(0.0);

        // --- Input field / Numeric controls / Boolean toggle ---
        ui.horizontal(|ui| {
            ui.add_space(COLUMN_INDENT);

            let available = input_width(ui);

            // Numeric parameters: square minus | short input | square plus
            if def.key == "chunk_lines" || def.key == "sort_run_lines" {
                let button_size = egui::vec2(24.0, 24.0);
                let field_width = 90.0;

                ui.spacing_mut().item_spacing.x = 0.0;

                // Minus button
                let (minus_rect, minus_response) =
                    ui.allocate_exact_size(button_size, egui::Sense::click());

                ui.painter().rect(
                    minus_rect,
                    egui::CornerRadius::same(3),
                    theme::SURFACE,
                    theme::stroke_1(theme::BORDER),
                    egui::StrokeKind::Inside,
                );

                ui.painter().text(
                    minus_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "−",
                    egui::FontId::proportional(15.0),
                    theme::TEXT,
                );

                if minus_response.clicked() {
                    let value = current_value
                        .parse::<u64>()
                        .or_else(|_| def.default_value.parse::<u64>())
                        .unwrap_or(0);

                    *current_value = value.saturating_sub(1).to_string();
                }

                // Numeric input
                let response = ui.add_sized(
                    egui::vec2(field_width, 24.0),
                    egui::TextEdit::singleline(current_value)
                        .font(egui::TextStyle::Monospace)
                        .margin(egui::vec2(5.0, 3.0)),
                );

                if response.changed() {
                    current_value.retain(|c| c.is_ascii_digit());
                }

                // Plus button
                let (plus_rect, plus_response) =
                    ui.allocate_exact_size(button_size, egui::Sense::click());

                ui.painter().rect(
                    plus_rect,
                    egui::CornerRadius::same(3),
                    theme::SURFACE,
                    theme::stroke_1(theme::BORDER),
                    egui::StrokeKind::Inside,
                );

                ui.painter().text(
                    plus_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "+",
                    egui::FontId::proportional(15.0),
                    theme::TEXT,
                );

                if plus_response.clicked() {
                    let value = current_value
                        .parse::<u64>()
                        .or_else(|_| def.default_value.parse::<u64>())
                        .unwrap_or(0);

                    *current_value = value.saturating_add(1).to_string();
                }
            }
            // Boolean parameter: compact OFF / ON switch
            else if def.key == "confirm_delete" {
                let is_on = if current_value.trim().is_empty() {
                    def.default_value.eq_ignore_ascii_case("true")
                } else {
                    current_value.trim().eq_ignore_ascii_case("true")
                };

                let switch_width = 76.0;
                let switch_height = 24.0;
                let half_width = switch_width / 2.0;

                ui.spacing_mut().item_spacing.x = 0.0;

                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(switch_width, switch_height),
                    egui::Sense::click(),
                );

                let left_rect =
                    egui::Rect::from_min_size(rect.min, egui::vec2(half_width, switch_height));

                let right_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.left() + half_width, rect.top()),
                    egui::vec2(half_width, switch_height),
                );

                // Background
                ui.painter().rect(
                    rect,
                    egui::CornerRadius::same(4),
                    theme::SURFACE,
                    theme::stroke_1(theme::BORDER),
                    egui::StrokeKind::Inside,
                );

                // Active state background
                let active_rect = if is_on { right_rect } else { left_rect };

                ui.painter().rect_filled(
                    active_rect.shrink(2.0),
                    egui::CornerRadius::same(3),
                    if is_on {
                        theme::GREEN_DIM
                    } else {
                        theme::BORDER
                    },
                );

                // OFF label
                ui.painter().text(
                    left_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "OFF",
                    egui::FontId::proportional(11.0),
                    if !is_on {
                        theme::TEXT
                    } else {
                        theme::TEXT_MUTED
                    },
                );

                // ON label
                ui.painter().text(
                    right_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "ON",
                    egui::FontId::proportional(11.0),
                    if is_on {
                        theme::TEXT
                    } else {
                        theme::TEXT_MUTED
                    },
                );

                if response.clicked() {
                    *current_value = if is_on {
                        "false".to_string()
                    } else {
                        "true".to_string()
                    };
                }
            }
            // Regular text input: file path
            else {
                ui.add(
                    egui::TextEdit::singleline(current_value)
                        .desired_width(available)
                        .font(egui::TextStyle::Monospace)
                        .frame(true)
                        .margin(egui::vec2(INPUT_MARGIN_X, INPUT_MARGIN_Y))
                        .hint_text("Path to file (empty = auto-detect)"),
                );
            }

            // Modified marker
            if is_modified {
                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new("\u{25CF}")
                        .size(9.0)
                        .color(theme::GREEN_DIM),
                )
                .on_hover_text("Modified (not saved yet)");
            }
        });
    });
}

use crate::app::LogtoolsApp;
use crate::theme;
use eframe::egui;

pub fn render(ui: &mut egui::Ui, app: &mut LogtoolsApp) {
    theme::screen_title(ui, "Search", crate::screens::Screen::Search.subtitle());

    ui.add_enabled_ui(!app.busy, |ui| {
        let names: Vec<String> = app.bases.iter().map(|b| b.name.clone()).collect();
        let prev_base = app.search_base.clone();

        if app.search_base.is_empty() {
            if let Some(first) = names.first() {
                app.search_base = first.clone();
            }
        } else if !names.contains(&app.search_base) {
            app.search_base = names.first().cloned().unwrap_or_default();
        }

        ui.horizontal(|ui| {
            ui.label(theme::muted("Base:"));

            egui::ComboBox::from_id_salt("search_base_combo")
                .width(320.0)
                .selected_text(egui::RichText::new(app.search_base.as_str()).monospace())
                .show_ui(ui, |ui| {
                    for name in &names {
                        ui.selectable_value(&mut app.search_base, name.clone(), name.clone());
                    }
                });
        });

        if app.search_base != prev_base {
            let (f, l) = app.base_date_bounds();
            app.search_date_from =
                f.map(|v| crate::results_window::format_unix_time_gui(v as u64))
                    .unwrap_or_default();
            app.search_date_to =
                l.map(|v| crate::results_window::format_unix_time_gui(v as u64))
                    .unwrap_or_default();
        }

        ui.add_space(6.0);

        ui.checkbox(&mut app.search_use_date_filtering, "Use date filtering");

        ui.add_enabled_ui(app.search_use_date_filtering, |ui| {
            ui.label(theme::muted("From:").small());
            ui.add(
                egui::TextEdit::singleline(&mut app.search_date_from)
                    .desired_width(170.0)
                    .hint_text("YYYY-MM-DD или unix"),
            );
            ui.label(theme::muted("To:").small());
            ui.add(
                egui::TextEdit::singleline(&mut app.search_date_to)
                    .desired_width(170.0)
                    .hint_text("YYYY-MM-DD или unix"),
            );
            ui.label(theme::muted("пусто = всю базу").small());
        });

        ui.add_space(6.0);

        ui.add(
            egui::TextEdit::singleline(&mut app.search_text)
                .desired_width(ui.available_width())
                .hint_text("supports & | - ! parens and <globs>"),
        );

        ui.add_space(6.0);

        let can_search = !app.search_base.is_empty() && !app.search_text.is_empty();

        if ui
            .add_enabled(can_search, theme::primary_button("Search"))
            .clicked()
        {
            let _ = tools::config::save_config_param("last_search_base", &app.search_base);
            let _ = tools::config::save_config_param("last_search_text", &app.search_text);

            let (date_from, date_to) = if app.search_use_date_filtering {
                (
                    crate::results_window::parse_date_input(&app.search_date_from),
                    crate::results_window::parse_date_input(&app.search_date_to),
                )
            } else {
                (None, None)
            };

            app.spawn_command(
                crate::app::CmdKind::Search,
                crate::app::GuiCommand::Search {
                    base: app.search_base.clone(),
                    text: app.search_text.clone(),
                    date_from,
                    date_to,
                    use_date_filtering: app.search_use_date_filtering,
                },
            );
        }
    });

    let has_results = !app.search_results.is_empty();

    if has_results {
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
                ui.label(theme::muted(&format!("{} found", app.search_results.len())).small());

                if ui.add(theme::ghost_button("Open results window")).clicked() {
                    if let Some(state) = &app.search_results_window {
                        let mut s = state.lock().unwrap();
                        s.open = true;
                    } else {
                        app.search_results_window =
                            Some(std::sync::Arc::new(std::sync::Mutex::new(
                                crate::results_window::SearchResultsWindowState {
                                    open: true,
                                    search_base: app.search_base.clone(),
                                    search_text: app.search_text.clone(),
                                    search_results: std::sync::Arc::new(app.search_results.clone()),
                                    results_path: app.results_path.clone(),
                                    nav_target_row: None,
                                    context_lines: 5,
                                    context_marks: std::collections::HashSet::new(),
                                    find_open: false,
                                    find_text: String::new(),
                                    find_matches: Vec::new(),
                                    find_match_idx: 0,
                                    find_last_text: String::new(),
                                    find_was_open: false,
                                },
                            )));
                    }
                }

                if let Some(path) = &app.results_path {
                    ui.label(theme::muted("file:").small());
                    ui.label(
                        egui::RichText::new(path.as_str()).monospace().small(),
                    );

                    if ui.add(theme::ghost_button("Open results file")).clicked() {
                        app.open_results_file();
                    }
                }
            });
        });
    }
}

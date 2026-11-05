use crate::app::LogtoolsApp;
use crate::theme;
use eframe::egui;

pub fn render(ui: &mut egui::Ui, app: &mut LogtoolsApp) {
    theme::screen_title(ui, "Config", crate::screens::Screen::Config.subtitle());

    ui.add_enabled_ui(!app.busy, |ui| {
        theme::section_label(ui, "everything_dll");

        ui.add(
            egui::TextEdit::singleline(&mut app.cfg_everything_dll)
                .desired_width(ui.available_width()),
        );

        ui.label(theme::muted("Path to Everything64.dll or es.exe"));

        ui.add_space(8.0);

        theme::section_label(ui, "chunk_lines");

        ui.add(
            egui::TextEdit::singleline(&mut app.cfg_chunk_lines)
                .desired_width(ui.available_width())
                .hint_text("lines per chunk (default 500000)"),
        );

        ui.add_space(8.0);

        theme::section_label(ui, "sort_run_lines");

        ui.add(
            egui::TextEdit::singleline(&mut app.cfg_sort_run_lines)
                .desired_width(ui.available_width())
                .hint_text("in-memory sort buffer (default 250000)"),
        );

        ui.label(theme::muted(
            "Lower sort_run_lines uses less RAM; higher may be faster for small bases.",
        ));

        ui.add_space(8.0);

        theme::section_label(ui, "confirm_delete");

        ui.add(
            egui::TextEdit::singleline(&mut app.cfg_confirm_delete)
                .desired_width(ui.available_width()),
        );

        ui.add_space(8.0);

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
    });

    ui.add_space(8.0);
    ui.separator();

    let map = tools::config::load_config();

    if map.is_empty() {
        ui.label(theme::muted("No config parameters set."));
    } else {
        theme::section_label(ui, "CURRENT VALUES");

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

    ui.add_space(8.0);
    ui.separator();

    theme::section_label(ui, "APP STATE");
    ui.label(theme::muted(
        "Internal memory of the app (not a config, edited automatically).",
    ));

    let state = tools::state::load_state();

    if state.is_empty() {
        ui.label(theme::muted("No app state stored."));
    } else {
        for (k, v) in &state {
            ui.horizontal(|ui| {
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
}

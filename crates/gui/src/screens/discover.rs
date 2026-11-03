use crate::app::LogtoolsApp;
use crate::theme;
use eframe::egui;

pub fn render(ui: &mut egui::Ui, app: &mut LogtoolsApp) {
    theme::screen_title(ui, "Discover", crate::screens::Screen::Discover.subtitle());

    ui.add_enabled_ui(!app.busy, |ui| {
        ui.label(theme::muted("Root directory:"));

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut app.disc_root)
                    .desired_width(420.0)
                    .hint_text("Empty = user home"),
            );

            if ui.add(theme::ghost_button("Browse")).clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    app.disc_root = dir.to_string_lossy().to_string();
                }
            }
        });

        ui.horizontal(|ui| {
            ui.checkbox(&mut app.disc_use_everything, "Use Everything SDK/CLI");
            ui.checkbox(&mut app.disc_skip_everything, "Skip Everything");
        });

        ui.label(theme::muted("Path to Everything64.dll / es.exe:"));
        ui.add(egui::TextEdit::singleline(&mut app.disc_es).desired_width(420.0));

        ui.add_space(8.0);

        if ui.add(theme::primary_button("Discover")).clicked() {
            let no_everything = app.disc_skip_everything || !app.disc_use_everything;

            let root = if app.disc_root.trim().is_empty() {
                None
            } else {
                Some(app.disc_root.trim().to_string())
            };

            let es = if app.disc_es.trim().is_empty() {
                None
            } else {
                Some(app.disc_es.trim().to_string())
            };

            app.spawn_command(
                crate::app::CmdKind::Discover,
                crate::app::GuiCommand::Discover {
                    root,
                    out: None,
                    es,
                    no_everything,
                },
            );
        }
    });

    let preview = app.search_preview.clone();

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
                if ui.add(theme::ghost_button("Open results file")).clicked() {
                    app.open_results_file();
                }
            });
        });

        egui::ScrollArea::vertical()
            .id_salt("discover_preview")
            .auto_shrink(false)
            .show(ui, |ui| {
                for line in preview {
                    ui.monospace(
                        egui::RichText::new(line.as_str()).color(crate::theme::log_line_color(
                            &line,
                        )),
                    );
                }
            });
    }
}

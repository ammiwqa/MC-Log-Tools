use crate::app::LogtoolsApp;
use crate::theme;
use eframe::egui;

pub fn render(ui: &mut egui::Ui, app: &mut LogtoolsApp) {
    theme::screen_title(ui, "Bases", crate::screens::Screen::Bases.subtitle());

    ui.add_enabled_ui(!app.busy, |ui| {
        ui.horizontal(|ui| {
            if ui.add(theme::ghost_button("Refresh")).clicked() {
                app.reload_bases();
            }

            if ui.add(theme::primary_button("Create new base")).clicked() {
                app.create_modal_open = true;
            }
        });
    });

    ui.add_space(8.0);
    ui.separator();

    if app.bases.is_empty() {
        ui.group(|ui| {
            ui.set_min_size(egui::vec2(ui.available_width(), 90.0));

            ui.centered_and_justified(|ui| {
                ui.label(theme::muted("No bases found. Create one to start searching."));
            });
        });
    } else {
        let bases = app.bases.clone();

        egui::ScrollArea::vertical()
            .id_salt("bases_table")
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Grid::new("bases_grid").striped(true).show(ui, |ui| {
                    theme::section_label(ui, "NAME");
                    theme::section_label(ui, "LINES");
                    theme::section_label(ui, "ACTIONS");
                    ui.end_row();

                    for base in &bases {
                        ui.label(egui::RichText::new(base.name.clone()).monospace());
                        ui.label(theme::muted(&base.lines.to_string()).monospace());

                        let name = base.name.clone();

                        if ui.add_enabled(!app.busy, theme::small_button("Update")).clicked() {
                            app.spawn_command(
                                crate::app::CmdKind::UpdateBase,
                                crate::app::GuiCommand::UpdateBase { name },
                            );
                        }

                        let name = base.name.clone();

                        if ui
                            .add_enabled(!app.busy, theme::danger_button("Delete"))
                            .clicked()
                        {
                            app.spawn_command(
                                crate::app::CmdKind::Delete,
                                crate::app::GuiCommand::Delete {
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

    if let Some(path) = &app.results_path {
        ui.add_space(8.0);
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(theme::muted("Results file:").monospace());
            ui.label(egui::RichText::new(path.as_str()).monospace());

            if ui.add(theme::ghost_button("Open results file")).clicked() {
                app.open_results_file();
            }
        });
    }
}

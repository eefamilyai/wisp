//! The application frame: window chrome, navigation, page header and footer.
//!
//! Pages themselves live in the tabs at the bottom of this file; everything
//! around them is here.

use eframe::egui;

use crate::ui;
use crate::{accent32, App, Tab};

/// One entry in the navigation column, and the page it opens.
pub struct Nav {
    pub tab: Tab,
    /// Segoe Fluent Icons glyph.
    pub glyph: &'static str,
    pub label: &'static str,
    /// Heading shown above the navigation block.
    pub group: &'static str,
    /// Title and one-line description at the top of the page.
    pub title: &'static str,
    pub subtitle: &'static str,
}

pub const NAV: &[Nav] = &[
    Nav {
        tab: Tab::Home,
        glyph: "\u{E80F}",
        label: "Home",
        group: "Overview",
        title: "Home",
        subtitle: "How it's doing, and everything it can do for you",
    },
    Nav {
        tab: Tab::General,
        glyph: "\u{E713}",
        label: "General",
        group: "The assistant",
        title: "General",
        subtitle: "Name, wake word and how this window looks",
    },
    Nav {
        tab: Tab::Voice,
        glyph: "\u{E767}",
        label: "Voice & personality",
        group: "The assistant",
        title: "Voice & personality",
        subtitle: "How it sounds and what it says back",
    },
    Nav {
        tab: Tab::Listening,
        glyph: "\u{E720}",
        label: "Listening",
        group: "The assistant",
        title: "Listening",
        subtitle: "Microphone, wake word and the microphone check",
    },
    Nav {
        tab: Tab::Schedule,
        glyph: "\u{E787}",
        label: "Alarms & reminders",
        group: "Behaviour",
        title: "Alarms & reminders",
        subtitle: "Alarms, reminders, to-dos and calendar events",
    },
    Nav {
        tab: Tab::Brain,
        glyph: "\u{E99A}",
        label: "Brain",
        group: "Behaviour",
        title: "Brain",
        subtitle: "The model that turns a sentence into actions, and whose it is",
    },
    Nav {
        tab: Tab::Functions,
        glyph: "\u{E945}",
        label: "Functions",
        group: "What it can do",
        title: "Functions",
        subtitle: "Every function, one switch each, and the ones you wrote",
    },
    Nav {
        tab: Tab::Computer,
        glyph: "\u{E7F4}",
        label: "Computer access",
        group: "What it can do",
        title: "Computer access",
        subtitle: "What it is allowed to touch on this PC, and how that is kept safe",
    },
    Nav {
        tab: Tab::Apps,
        glyph: "\u{E71D}",
        label: "Apps",
        group: "What it can do",
        title: "Apps",
        subtitle: "Which apps it can find, and the shortcuts it learns",
    },
    Nav {
        tab: Tab::Browser,
        glyph: "\u{E774}",
        label: "Browser",
        group: "What it can do",
        title: "Browser",
        subtitle: "Where links and searches open",
    },
    Nav {
        tab: Tab::Test,
        glyph: "\u{E9D9}",
        label: "Test a command",
        group: "Tools",
        title: "Test a command",
        subtitle: "See what it would do, without saying it out loud",
    },
];

impl Nav {
    pub fn current(&self, tab: Tab) -> bool {
        self.tab == tab
    }
}

impl App {
    /// Window chrome: dragging, the icon buttons, and the resize edges.
    fn title_bar(&mut self, ui: &mut egui::Ui, theme: &ui::Theme) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("chrome")
            .exact_size(42.0)
            .frame(
                egui::Frame::new()
                    .fill(theme.sidebar)
                    .stroke(egui::Stroke::NONE)
                    .inner_margin(egui::Margin { left: 0, right: 0, top: 7, bottom: 0 }),
            )
            .show(ui, |ui| {
                let maximised = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui::mark(ui, theme.accent, 18.0);
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Wisp").size(13.0).strong().color(theme.text));
                    ui.label(egui::RichText::new("Settings").size(12.0).color(theme.faint));
                    ui.label(
                        egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(10.5)
                            .color(theme.faint),
                    );

                    // The empty middle of the bar is the drag handle. A response
                    // over the whole bar would swallow the buttons.
                    let buttons = 150.0;
                    let space = (ui.available_width() - buttons).max(16.0);
                    let (_, drag) = ui.allocate_exact_size(egui::vec2(space, 26.0), egui::Sense::click_and_drag());
                    if drag.double_clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximised));
                    } else if drag.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(6.0);
                        if ui::caption_button(ui, ui::ICON_CLOSE, theme.danger, theme).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        let (icon, tip) = if maximised {
                            (ui::ICON_RESTORE, "Restore")
                        } else {
                            (ui::ICON_MAXIMIZE, "Maximise")
                        };
                        if ui::caption_button(ui, icon, theme.accent, theme).on_hover_text(tip).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximised));
                        }
                        if ui::caption_button(ui, ui::ICON_MINIMIZE, theme.accent, theme)
                            .on_hover_text("Minimise")
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        ui.add_space(6.0);
                        let (colour, text) = if self.agent_running {
                            (theme.ok, format!("Listening as \"{}\"", self.cfg.wake_phrase()))
                        } else {
                            (theme.faint, "Assistant stopped".to_string())
                        };
                        ui::pill(ui, &text, colour);
                    });
                });
            });
    }

    /// The navigation column: grouped pages, and the assistant's controls.
    fn sidebar(&mut self, ui: &mut egui::Ui, theme: &ui::Theme) {
        egui::Panel::left("nav")
            .exact_size(214.0)
            .frame(egui::Frame::new().fill(theme.sidebar).inner_margin(egui::Margin::symmetric(10, 10)))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                // The pages scroll if the window is short; the assistant's own
                // controls stay pinned at the bottom.
                const BOTTOM: f32 = 96.0;
                let list_height = (ui.available_height() - BOTTOM).max(120.0);
                egui::ScrollArea::vertical()
                    .max_height(list_height)
                    .auto_shrink([false, false])
                    .id_salt("nav")
                    .show(ui, |ui| {
                        let mut last_group = "";
                        for entry in NAV {
                            if entry.group != last_group {
                                ui::nav_group(ui, entry.group);
                                last_group = entry.group;
                            }
                            let shortcut = entry.tab.index() + 1;
                            let response = ui::nav(ui, entry.glyph, entry.label, entry.current(self.tab));
                            let response =
                                if shortcut <= 9 { response.on_hover_text(format!("Ctrl+{shortcut}")) } else { response };
                            if response.clicked() {
                                self.tab = entry.tab;
                            }
                        }
                    });

                // The assistant's controls live at the bottom of the column.
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.add_space(2.0);

                    egui::Frame::new()
                        .fill(theme.inset)
                        .stroke(theme.hairline())
                        .corner_radius(egui::CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            // The column stacks upwards; put the card's own
                            // contents back into reading order.
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.horizontal(|ui| {
                                ui::dot(ui, if self.agent_running { theme.ok } else { theme.faint }, 7.0);
                                ui.label(
                                    egui::RichText::new(if self.agent_running { "Running" } else { "Not running" })
                                        .size(12.5)
                                        .color(if self.agent_running { theme.text } else { theme.muted }),
                                );
                            });
                            ui.add_space(8.0);
                            // A button you can see without hunting for it, and
                            // that says what is actually true of the assistant.
                            let (label, enabled) = match (self.agent_running, self.starting_since) {
                                (true, _) => ("Stop", true),
                                (false, Some(_)) => ("Starting…", false),
                                (false, None) => ("Start assistant", true),
                            };
                            let width = ui.available_width();
                            let button = egui::Button::new(
                                egui::RichText::new(label).size(13.0).strong().color(if enabled {
                                    theme.text
                                } else {
                                    theme.muted
                                }),
                            )
                            .fill(if self.agent_running { theme.inset } else { theme.accent_soft(60) })
                            .stroke(egui::Stroke::new(1.0, if self.agent_running { theme.border_strong } else { theme.accent }))
                            .corner_radius(egui::CornerRadius::same(9))
                            .min_size(egui::vec2(width, 30.0));
                            if ui.add_enabled(enabled, button).clicked() {
                                if self.agent_running {
                                    self.stop_agent();
                                } else {
                                    self.start_agent();
                                }
                            }
                        });
                        });
                });
            });
    }

    /// The sticky footer: unsaved changes on the left, actions on the right.
    fn footer(&mut self, ui: &mut egui::Ui, theme: &ui::Theme) {
        let dirty = self.cfg != self.saved;
        egui::Panel::bottom("footer")
            .exact_size(56.0)
            .frame(
                egui::Frame::new()
                    .fill(theme.sidebar)
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(e) = self.load_error.clone() {
                        ui.label(egui::RichText::new(format!("⚠ {e}")).size(12.0).color(theme.danger));
                    } else if let Some((msg, at, ok)) = self.status.clone() {
                        let age = at.elapsed().as_secs_f32();
                        if age < 8.0 {
                            ui::dot(ui, if ok { theme.ok } else { theme.danger }, 7.0);
                            let colour = if ok { theme.text } else { theme.danger };
                            ui.label(egui::RichText::new(msg).size(12.5).color(colour));
                        }
                    } else if dirty {
                        ui::pill(ui, "Unsaved changes", theme.warn);
                    } else {
                        ui.label(egui::RichText::new("All changes saved").size(12.0).color(theme.faint));
                    }

                    // Any running job reports itself here.
                    for (key, job) in self.jobs.iter() {
                        let state = job.0.lock().map(|s| (s.label.clone(), s.progress)).unwrap_or_default();
                        let _ = key;
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(state.0).size(12.0).color(theme.muted));
                        if state.1 >= 0.0 {
                            ui.add(
                                egui::ProgressBar::new(state.1.max(0.0))
                                    .desired_width(120.0)
                                    .fill(theme.accent),
                            );
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui::primary(ui, "Save & apply", dirty).clicked() {
                            self.save();
                        }
                        ui.add_space(2.0);
                        ui::kbd(ui, "Ctrl+S");
                        if ui::ghost(ui, "Revert").clicked() {
                            self.cfg = self.saved.clone();
                            // The text boxes are rebuilt from the config, so the
                            // half-typed values in them have to go with it.
                            self.drafts.clear();
                            self.status = Some(("Put back the saved settings".into(), std::time::Instant::now(), true));
                        }
                    });
                });
            });
    }

    /// The page itself.
    fn content(&mut self, ui: &mut egui::Ui, theme: &ui::Theme, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme.bg).inner_margin(egui::Margin::symmetric(22, 14)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let width = ui.available_width().min(820.0);
                        let margin = ((ui.available_width() - width) / 2.0).max(0.0);
                        ui.horizontal_top(|ui| {
                            ui.add_space(margin);
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                if let Some(entry) = NAV.iter().find(|n| n.current(self.tab)) {
                                    ui::page(ui, entry.title, entry.subtitle, |_| {});
                                }
                                match self.tab {
                                    Tab::Home => self.tab_home(ui, ctx),
                                    Tab::Computer => self.tab_computer(ui),
                                    Tab::General => self.tab_general(ui, theme.accent),
                                    Tab::Voice => self.tab_voice(ui, ctx, theme.accent),
                                    Tab::Listening => self.tab_listening(ui, ctx, theme.accent),
                                    Tab::Schedule => self.tab_schedule(ui, theme.accent),
                                    Tab::Brain => self.tab_brain(ui, ctx),
                                    Tab::Browser => self.tab_browser(ui, theme.accent),
                                    Tab::Apps => self.tab_apps(ui, ctx, theme.accent),
                                    Tab::Functions => self.tab_functions(ui, ctx, theme.accent),
                                    Tab::Test => self.tab_test(ui, ctx),
                                }
                                ui.add_space(24.0);
                            });
                        });
                    });
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let theme = ui::Theme::new(self.cfg.appearance.is_dark(), accent32(self.cfg.accent_rgb()));
        ui::set(&ctx, theme);
        ui::style(&ctx, &theme);
        self.poll_jobs();
        self.refresh_pendings();

        // Keyboard: the pages in order, and save. Handled before any widget so
        // a focused text box cannot swallow them.
        // A text box with the focus owns the number keys; Ctrl+S is always ours.
        let typing = ctx.memory(|m| m.focused().is_some());
        let mut go: Option<Tab> = None;
        let mut save = false;
        ctx.input_mut(|i| {
            const NUMBER_KEYS: [egui::Key; 9] = [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
                egui::Key::Num6,
                egui::Key::Num7,
                egui::Key::Num8,
                egui::Key::Num9,
            ];
            for (n, key) in NUMBER_KEYS.iter().enumerate() {
                if i.consume_key(egui::Modifiers::CTRL, *key) && !typing {
                    go = Tab::ALL.get(n).copied();
                }
            }
            if i.consume_key(egui::Modifiers::CTRL, egui::Key::S) {
                save = true;
            }
        });
        if let Some(tab) = go {
            self.tab = tab;
        }
        if save {
            self.save();
        }

        ui::resize_edges(ui);
        self.title_bar(ui, &theme);
        // The footer is placed before the sidebar so it runs the full width and
        // the navigation column stops above it, instead of being cut off by the
        // bottom of the window.
        self.footer(ui, &theme);
        self.sidebar(ui, &theme);
        self.content(ui, &theme, &ctx);

        // Structure lines: a frameless window has none of its own, and the
        // fills are too close in value to read as separate surfaces.
        let lines = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("chrome-lines")));
        let rect = ctx.viewport_rect();
        let chrome_bottom = rect.top() + 42.0;
        lines.line_segment(
            [egui::pos2(rect.left(), chrome_bottom), egui::pos2(rect.right(), chrome_bottom)],
            theme.hairline(),
        );
        let nav_right = rect.left() + 214.0;
        let nav_bottom = rect.bottom() - 56.0;
        lines.line_segment(
            [egui::pos2(nav_right, chrome_bottom), egui::pos2(nav_right, nav_bottom)],
            theme.hairline(),
        );

        // The dashboard's numbers and the orb's pulse only move if the window
        // keeps being drawn. egui redraws on input otherwise, which left the
        // home page frozen on whatever it looked like at the first frame.
        ctx.request_repaint_after(std::time::Duration::from_millis(if self.tab == Tab::Home { 110 } else { 600 }));

        // A frameless window has no border of its own, so draw one.
        let outline = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("outline")));
        outline.rect_stroke(
            ctx.viewport_rect(),
            egui::CornerRadius::ZERO,
            egui::Stroke::new(1.0, theme.border_strong),
            egui::StrokeKind::Inside,
        );
    }
}

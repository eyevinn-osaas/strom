// ! Login UI component.

use egui::{Align2, Context, Vec2, Window};

/// Login screen state
pub struct LoginScreen {
    /// Username input
    pub username: String,
    /// Password input
    pub password: String,
    /// Error message
    pub error: Option<String>,
    /// Whether login is in progress
    pub logging_in: bool,
}

impl Default for LoginScreen {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            error: None,
            logging_in: false,
        }
    }
}

impl LoginScreen {
    /// Show the login screen and return true if login was requested
    pub fn show(&mut self, ctx: &Context) -> bool {
        let mut login_requested = false;

        Window::new("Login")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.heading("Strom Authentication");
                ui.add_space(10.0);

                if let Some(ref error) = self.error {
                    ui.colored_label(egui::Color32::RED, error);
                    ui.add_space(5.0);
                }

                ui.label("Username:");
                let username_response = ui.text_edit_singleline(&mut self.username);

                ui.label("Password:");
                let password_response = ui.add(
                    egui::TextEdit::singleline(&mut self.password)
                        .password(true)
                );

                // Submit on Enter in either field
                if (username_response.lost_focus() || password_response.lost_focus())
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    login_requested = true;
                }

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    if ui.button("Login").clicked() || login_requested {
                        login_requested = true;
                    }

                    if self.logging_in {
                        ui.spinner();
                        ui.label("Logging in...");
                    }
                });

                ui.add_space(5.0);
                ui.label("Default credentials can be configured via environment variables:");
                ui.monospace("STROM_ADMIN_USER and STROM_ADMIN_PASSWORD_HASH");
            });

        login_requested && !self.logging_in
    }

    /// Clear any error messages
    pub fn clear_error(&mut self) {
        self.error = None;
    }

    /// Set an error message
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.logging_in = false;
    }

    /// Set logging in state
    pub fn set_logging_in(&mut self, logging_in: bool) {
        self.logging_in = logging_in;
        if logging_in {
            self.error = None;
        }
    }
}

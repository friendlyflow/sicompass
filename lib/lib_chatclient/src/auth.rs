//! Matrix login, registration, and UIA methods for ChatClientProvider.

use super::{AuthResult, ChatClientProvider, parse_auth_response};

impl ChatClientProvider {
    pub(crate) fn handle_register_result(&mut self, result: AuthResult) {
        if result.success {
            self.access_token = result.access_token.clone();
            self.user_id = result.user_id.clone();
            self.uia_session.clear();
            self.register_error = None;
            self.register_mode = false;
            self.save_access_token(&result.access_token);
            self.save_user_id(&result.user_id);
            self.save_setting("chatHomeserver", &self.homeserver.clone());
            self.save_setting("chatUsername", &self.username.clone());
            self.save_setting("chatEmail", &self.email.clone());
            self.maybe_start_sync();
        } else if result.requires_auth && !result.session.is_empty() {
            self.uia_session = result.session.clone();
            self.register_mode = true;
            let stage_hint = if result.next_stage.is_empty() {
                "authentication required in browser".to_owned()
            } else {
                format!(
                    "complete {} in browser, then click Complete registration",
                    result.next_stage
                )
            };
            let prior_err = self.register_error.take();
            self.register_error = Some(match prior_err {
                Some(e) => format!("{stage_hint} (note: {e})"),
                None => stage_hint,
            });
            #[cfg(not(test))]
            {
                let fallback_url = format!(
                    "{}/_matrix/client/v3/auth/{}/fallback/web?session={}",
                    self.homeserver.trim_end_matches('/'),
                    result.next_stage,
                    result.session,
                );
                sicompass_sdk::platform::open_with_default(&fallback_url);
            }
        } else {
            self.uia_session.clear();
            self.register_error = Some(format!("registration failed: {}", result.error));
        }
    }

    pub(crate) fn do_login(&mut self) -> AuthResult {
        if self.homeserver.is_empty() || self.username.is_empty() || self.password.is_empty() {
            return AuthResult {
                error: "homeserver, username, and password are required".to_owned(),
                ..Default::default()
            };
        }
        let client = match self.client() {
            Ok(c) => c,
            Err(e) => {
                return AuthResult {
                    error: format!("HTTP client error: {e}"),
                    ..Default::default()
                }
            }
        };
        let url = self.api("/_matrix/client/v3/login");
        let payload = serde_json::json!({
            "type": "m.login.password",
            "identifier": { "type": "m.id.user", "user": self.username },
            "password": self.password,
        });
        let resp = match client.post(&url).json(&payload).send() {
            Ok(r) => r,
            Err(e) => {
                return AuthResult {
                    error: format!("request failed: {e}"),
                    ..Default::default()
                }
            }
        };
        match resp.json::<serde_json::Value>() {
            Ok(body) => {
                let result = parse_auth_response(body);
                if result.success {
                    self.access_token = result.access_token.clone();
                    self.user_id = result.user_id.clone();
                }
                result
            }
            Err(_) => AuthResult {
                error: "failed to parse server response".to_owned(),
                ..Default::default()
            },
        }
    }

    pub(crate) fn do_register(&self) -> AuthResult {
        if self.homeserver.is_empty() || self.username.is_empty() || self.password.is_empty() {
            return AuthResult {
                error: "homeserver, username, and password are required".to_owned(),
                ..Default::default()
            };
        }
        let client = match self.client() {
            Ok(c) => c,
            Err(e) => {
                return AuthResult {
                    error: format!("HTTP client error: {e}"),
                    ..Default::default()
                }
            }
        };
        let url = self.api("/_matrix/client/v3/register");

        // Step 1: probe UIA flows without auth to discover what the server requires.
        let probe_payload = serde_json::json!({
            "username": self.username,
            "password": self.password,
        });
        let resp = match client.post(&url).json(&probe_payload).send() {
            Ok(r) => r,
            Err(e) => {
                return AuthResult {
                    error: format!("request failed: {e}"),
                    ..Default::default()
                }
            }
        };
        let probe_body: serde_json::Value = match resp.json() {
            Ok(b) => b,
            Err(_) => {
                return AuthResult {
                    error: "failed to parse server response".to_owned(),
                    ..Default::default()
                }
            }
        };
        let probe = parse_auth_response(probe_body);

        if probe.success {
            return probe;
        }

        // Step 2: if the only required stage is m.login.dummy, complete it immediately.
        if probe.requires_auth && probe.next_stage == "m.login.dummy" {
            let auth_payload = serde_json::json!({
                "auth": { "type": "m.login.dummy", "session": probe.session },
                "username": self.username,
                "password": self.password,
            });
            let resp2 = match client.post(&url).json(&auth_payload).send() {
                Ok(r) => r,
                Err(e) => {
                    return AuthResult {
                        error: format!("request failed: {e}"),
                        ..Default::default()
                    }
                }
            };
            return match resp2.json::<serde_json::Value>() {
                Ok(b) => parse_auth_response(b),
                Err(_) => AuthResult {
                    error: "failed to parse server response".to_owned(),
                    ..Default::default()
                },
            };
        }

        // Any other UIA stage (CAPTCHA, email, etc.) or an error: return probe result.
        probe
    }

    pub(crate) fn do_register_complete(&self, session: &str) -> AuthResult {
        if self.homeserver.is_empty()
            || session.is_empty()
            || self.username.is_empty()
            || self.password.is_empty()
        {
            return AuthResult {
                error: "homeserver, session, username, and password are required".to_owned(),
                ..Default::default()
            };
        }
        let client = match self.client() {
            Ok(c) => c,
            Err(e) => {
                return AuthResult {
                    error: format!("HTTP client error: {e}"),
                    ..Default::default()
                }
            }
        };
        let url = self.api("/_matrix/client/v3/register");
        let payload = serde_json::json!({
            "auth": { "session": session },
            "username": self.username,
            "password": self.password,
        });
        let resp = match client.post(&url).json(&payload).send() {
            Ok(r) => r,
            Err(e) => {
                return AuthResult {
                    error: format!("request failed: {e}"),
                    ..Default::default()
                }
            }
        };
        match resp.json::<serde_json::Value>() {
            Ok(body) => parse_auth_response(body),
            Err(_) => AuthResult {
                error: "failed to parse server response".to_owned(),
                ..Default::default()
            },
        }
    }
}

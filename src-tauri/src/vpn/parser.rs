use std::{
    collections::{BTreeSet, HashMap},
    net::IpAddr,
    path::PathBuf,
};

use crate::error::AppError;

const RECOGNIZED_DIRECTIVES: &[&str] = &[
    "remote",
    "proto",
    "dev",
    "ca",
    "cert",
    "key",
    "auth-user-pass",
    "cipher",
    "data-ciphers",
    "redirect-gateway",
    "route",
    "route-ipv6",
    "dhcp-option",
    "pull-filter",
];

const EXTERNAL_FILE_DIRECTIVES: &[&str] = &[
    "ca",
    "cert",
    "key",
    "tls-auth",
    "tls-crypt",
    "tls-crypt-v2",
    "pkcs12",
];

const INLINE_DATA_TAGS: &[&str] = &[
    "ca",
    "cert",
    "key",
    "tls-auth",
    "tls-crypt",
    "tls-crypt-v2",
    "pkcs12",
    "extra-certs",
];

// RoutePilot launches OpenVPN with administrator privileges on macOS so it can
// create the utun interface. Only data-only client options are accepted in that
// mode. In particular, script hooks, plugins, log/status output paths, daemon
// controls, and alternate management interfaces must never cross the privilege
// boundary from an imported configuration.
const PRIVILEGED_CLIENT_DIRECTIVES: &[&str] = &[
    "allow-compression",
    "allow-deprecated-insecure-static-crypto",
    "auth",
    "auth-nocache",
    "auth-retry",
    "auth-user-pass",
    "block-ipv6",
    "block-outside-dns",
    "ca",
    "capath",
    "cert",
    "cipher",
    "client",
    "client-nat",
    "comp-lzo",
    "compat-mode",
    "compress",
    "connect-retry",
    "connect-retry-max",
    "connect-timeout",
    "crl-verify",
    "data-ciphers",
    "data-ciphers-fallback",
    "dev",
    "dev-type",
    "dhcp-option",
    "disable-occ",
    "explicit-exit-notify",
    "extra-certs",
    "fast-io",
    "float",
    "fragment",
    "hand-window",
    "http-proxy-option",
    "ifconfig",
    "ifconfig-ipv6",
    "ifconfig-noexec",
    "ifconfig-nowarn",
    "ignore-unknown-option",
    "inactive",
    "keepalive",
    "key",
    "key-direction",
    "link-mtu",
    "lport",
    "machine-readable-output",
    "mssfix",
    "mtu-disc",
    "mute",
    "mute-replay-warnings",
    "nobind",
    "ns-cert-type",
    "passtos",
    "peer-fingerprint",
    "persist-key",
    "persist-local-ip",
    "persist-remote-ip",
    "persist-tun",
    "ping",
    "ping-exit",
    "ping-restart",
    "ping-timer-rem",
    "pkcs12",
    "port",
    "proto",
    "proto-force",
    "pull",
    "pull-filter",
    "push-peer-info",
    "rcvbuf",
    "redirect-gateway",
    "redirect-private",
    "register-dns",
    "remote",
    "remote-cert-eku",
    "remote-cert-ku",
    "remote-cert-tls",
    "remote-random",
    "remote-random-hostname",
    "remap-usr1",
    "reneg-bytes",
    "reneg-pkts",
    "reneg-sec",
    "resolv-retry",
    "route",
    "route-delay",
    "route-gateway",
    "route-ipv6",
    "route-method",
    "route-metric",
    "route-noexec",
    "route-nopull",
    "rport",
    "server-poll-timeout",
    "setenv",
    "sndbuf",
    "socket-flags",
    "suppress-timestamps",
    "tcp-nodelay",
    "tls-auth",
    "tls-cert-profile",
    "tls-cipher",
    "tls-ciphersuites",
    "tls-client",
    "tls-crypt",
    "tls-crypt-v2",
    "tls-timeout",
    "tls-version-max",
    "tls-version-min",
    "topology",
    "tran-window",
    "tun-mtu",
    "tun-mtu-extra",
    "txqueuelen",
    "verb",
    "verify-hash",
    "verify-x509-name",
    "x509-username-field",
];

#[derive(Clone, PartialEq, Eq)]
struct ConfigDirective {
    name: String,
    line_number: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OvpnRemote {
    pub host: String,
    pub port: Option<u16>,
    pub protocol: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OvpnRouteDirective {
    pub network: String,
    pub netmask: Option<String>,
    pub gateway: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OvpnPullFilter {
    pub action: String,
    pub text: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ExternalFileReference {
    pub line_index: usize,
    pub line_number: usize,
    pub directive: String,
    pub path: PathBuf,
    trailing_arguments: Vec<String>,
}

/// Parsed OpenVPN configuration metadata.
///
/// This type deliberately does not implement `Debug` or serialization because
/// its private source lines can contain inline private keys and certificates.
pub struct ParsedOvpnConfig {
    pub remote: Option<OvpnRemote>,
    pub protocol: Option<String>,
    pub device: Option<String>,
    pub auth_user_pass: bool,
    pub cipher: Option<String>,
    pub data_ciphers: Option<String>,
    pub redirect_gateway: bool,
    pub routes: Vec<OvpnRouteDirective>,
    pub dns_servers: Vec<IpAddr>,
    pub pull_filters: Vec<OvpnPullFilter>,
    pub recognized_directives: BTreeSet<String>,
    external_files: Vec<ExternalFileReference>,
    auth_user_pass_file_lines: BTreeSet<usize>,
    directives: Vec<ConfigDirective>,
    source_lines: Vec<String>,
    had_trailing_newline: bool,
}

impl ParsedOvpnConfig {
    pub fn parse(source: &str) -> Result<Self, AppError> {
        let source_lines = source
            .lines()
            .map(|line| line.trim_end_matches('\r').to_owned())
            .collect::<Vec<_>>();

        let mut parsed = Self {
            remote: None,
            protocol: None,
            device: None,
            auth_user_pass: false,
            cipher: None,
            data_ciphers: None,
            redirect_gateway: false,
            routes: Vec::new(),
            dns_servers: Vec::new(),
            pull_filters: Vec::new(),
            recognized_directives: BTreeSet::new(),
            external_files: Vec::new(),
            auth_user_pass_file_lines: BTreeSet::new(),
            directives: Vec::new(),
            source_lines,
            had_trailing_newline: source.ends_with('\n'),
        };

        let mut inline_data_tag: Option<String> = None;

        for (line_index, line) in parsed.source_lines.iter().enumerate() {
            let trimmed = line.trim();

            if let Some(tag) = inline_data_tag.as_deref() {
                if is_closing_tag(trimmed, tag) {
                    inline_data_tag = None;
                }
                continue;
            }

            if let Some(tag) = opening_inline_data_tag(trimmed) {
                inline_data_tag = Some(tag.to_owned());
                continue;
            }

            let tokens = tokenize_line(line, line_index + 1)?;
            let Some(directive) = tokens.first().map(|token| token.to_ascii_lowercase()) else {
                continue;
            };
            parsed.directives.push(ConfigDirective {
                name: directive.clone(),
                line_number: line_index + 1,
            });

            if RECOGNIZED_DIRECTIVES.contains(&directive.as_str())
                || EXTERNAL_FILE_DIRECTIVES.contains(&directive.as_str())
            {
                parsed.recognized_directives.insert(directive.clone());
            }

            match directive.as_str() {
                "remote" if parsed.remote.is_none() => {
                    if let Some(host) = tokens.get(1) {
                        parsed.remote = Some(OvpnRemote {
                            host: host.clone(),
                            port: tokens.get(2).and_then(|port| port.parse::<u16>().ok()),
                            protocol: tokens.get(3).cloned(),
                        });
                    }
                }
                "proto" if parsed.protocol.is_none() => {
                    parsed.protocol = tokens.get(1).cloned();
                }
                "dev" if parsed.device.is_none() => {
                    parsed.device = tokens.get(1).cloned();
                }
                "auth-user-pass" => {
                    parsed.auth_user_pass = true;
                    if tokens.len() > 1 {
                        parsed.auth_user_pass_file_lines.insert(line_index);
                    }
                }
                "cipher" if parsed.cipher.is_none() => {
                    parsed.cipher = tokens.get(1).cloned();
                }
                "data-ciphers" if parsed.data_ciphers.is_none() => {
                    parsed.data_ciphers = tokens.get(1).cloned();
                }
                "redirect-gateway" => parsed.redirect_gateway = true,
                "route" => {
                    if let Some(network) = tokens.get(1) {
                        parsed.routes.push(OvpnRouteDirective {
                            network: network.clone(),
                            netmask: tokens.get(2).cloned(),
                            gateway: tokens.get(3).cloned(),
                        });
                    }
                }
                "route-ipv6" => {
                    if let Some(network) = tokens.get(1) {
                        parsed.routes.push(OvpnRouteDirective {
                            network: network.clone(),
                            netmask: None,
                            gateway: tokens.get(2).cloned(),
                        });
                    }
                }
                "dhcp-option" => {
                    if tokens
                        .get(1)
                        .is_some_and(|option| option.eq_ignore_ascii_case("DNS"))
                    {
                        if let Some(address) = tokens
                            .get(2)
                            .and_then(|address| address.parse::<IpAddr>().ok())
                        {
                            parsed.dns_servers.push(address);
                        }
                    }
                }
                "pull-filter" => {
                    if let (Some(action), Some(text)) = (tokens.get(1), tokens.get(2)) {
                        parsed.pull_filters.push(OvpnPullFilter {
                            action: action.clone(),
                            text: text.clone(),
                        });
                    }
                }
                _ => {}
            }

            if EXTERNAL_FILE_DIRECTIVES.contains(&directive.as_str()) {
                if let Some(path) = tokens.get(1) {
                    if !is_inline_reference(path) {
                        parsed.external_files.push(ExternalFileReference {
                            line_index,
                            line_number: line_index + 1,
                            directive,
                            path: PathBuf::from(path),
                            trailing_arguments: tokens.iter().skip(2).cloned().collect(),
                        });
                    }
                }
            }
        }

        if let Some(tag) = inline_data_tag {
            return Err(AppError::ConfigInvalid {
                reason: format!("inline <{tag}> block is not closed"),
            });
        }

        Ok(parsed)
    }

    pub fn external_files(&self) -> &[ExternalFileReference] {
        &self.external_files
    }

    pub fn validate_privileged_client_config(&self) -> Result<(), AppError> {
        if let Some(directive) = self
            .directives
            .iter()
            .find(|directive| !PRIVILEGED_CLIENT_DIRECTIVES.contains(&directive.name.as_str()))
        {
            return Err(AppError::ConfigInvalid {
                reason: format!(
                    "directive '{}' at line {} cannot run with administrator privileges",
                    directive.name, directive.line_number
                ),
            });
        }

        Ok(())
    }

    /// Builds the app-owned configuration without changing the imported source.
    /// Credential-file references are reduced to `auth-user-pass` so passwords are
    /// not copied into application storage.
    pub(crate) fn render_imported(
        &self,
        replacements: &HashMap<usize, String>,
    ) -> Result<String, AppError> {
        if replacements.len() != self.external_files.len()
            || self
                .external_files
                .iter()
                .any(|reference| !replacements.contains_key(&reference.line_index))
        {
            return Err(AppError::ConfigInvalid {
                reason: "not all external profile files were relocated".to_owned(),
            });
        }

        let references_by_line = self
            .external_files
            .iter()
            .map(|reference| (reference.line_index, reference))
            .collect::<HashMap<_, _>>();

        let mut output = String::new();

        for (line_index, source_line) in self.source_lines.iter().enumerate() {
            if let Some(replacement) = replacements.get(&line_index) {
                let reference =
                    references_by_line
                        .get(&line_index)
                        .ok_or_else(|| AppError::ConfigInvalid {
                            reason: format!("invalid file replacement at line {}", line_index + 1),
                        })?;

                output.push_str(&reference.directive);
                output.push(' ');
                output.push_str(&quote_argument(replacement));
                for argument in &reference.trailing_arguments {
                    output.push(' ');
                    output.push_str(&quote_argument(argument));
                }
            } else if self.auth_user_pass_file_lines.contains(&line_index) {
                output.push_str("auth-user-pass");
            } else {
                output.push_str(source_line);
            }

            if line_index + 1 < self.source_lines.len() || self.had_trailing_newline {
                output.push('\n');
            }
        }

        Ok(output)
    }
}

pub(super) fn tokenize_line(line: &str, line_number: usize) -> Result<Vec<String>, AppError> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for character in line.chars() {
        if escaped {
            token.push(character);
            escaped = false;
            continue;
        }

        if character == '\\' {
            escaped = true;
            continue;
        }

        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            } else {
                token.push(character);
            }
            continue;
        }

        match character {
            '\'' | '"' => quote = Some(character),
            '#' | ';' if token.is_empty() => break,
            character if character.is_whitespace() => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            _ => token.push(character),
        }
    }

    if escaped {
        token.push('\\');
    }

    if quote.is_some() {
        return Err(AppError::ConfigInvalid {
            reason: format!("unterminated quote at line {line_number}"),
        });
    }

    if !token.is_empty() {
        tokens.push(token);
    }

    Ok(tokens)
}

fn opening_inline_data_tag(line: &str) -> Option<&str> {
    let tag = line.strip_prefix('<')?.strip_suffix('>')?;
    if tag.starts_with('/') || tag.contains(char::is_whitespace) {
        return None;
    }

    INLINE_DATA_TAGS
        .iter()
        .find(|candidate| tag.eq_ignore_ascii_case(candidate))
        .copied()
}

fn is_closing_tag(line: &str, tag: &str) -> bool {
    line.strip_prefix("</")
        .and_then(|value| value.strip_suffix('>'))
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(tag))
}

fn is_inline_reference(path: &str) -> bool {
    matches!(path.to_ascii_lowercase().as_str(), "[inline]" | "inline")
}

fn quote_argument(argument: &str) -> String {
    let escaped = argument.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::ParsedOvpnConfig;

    const SAMPLE_CONFIG: &str = r#"client
dev tun
proto tcp-client
remote vpn.example.com 1194
ca "certificates/company ca.crt"
cert client.crt
key client.key
auth-user-pass credentials.txt
cipher AES-128-CBC
data-ciphers AES-256-GCM:AES-128-GCM
redirect-gateway def1
route 10.10.0.0 255.255.0.0
dhcp-option DNS 10.10.0.53
pull-filter ignore "redirect-gateway"
"#;

    #[test]
    fn parses_common_profile_directives() {
        let parsed = ParsedOvpnConfig::parse(SAMPLE_CONFIG).expect("config should parse");

        let remote = parsed.remote.as_ref().expect("remote should be present");
        assert_eq!(remote.host, "vpn.example.com");
        assert_eq!(remote.port, Some(1194));
        assert_eq!(parsed.protocol.as_deref(), Some("tcp-client"));
        assert_eq!(parsed.device.as_deref(), Some("tun"));
        assert!(parsed.auth_user_pass);
        assert_eq!(parsed.cipher.as_deref(), Some("AES-128-CBC"));
        assert_eq!(
            parsed.data_ciphers.as_deref(),
            Some("AES-256-GCM:AES-128-GCM")
        );
        assert!(parsed.redirect_gateway);
        assert_eq!(parsed.routes.len(), 1);
        assert_eq!(parsed.dns_servers[0].to_string(), "10.10.0.53");
        assert_eq!(parsed.pull_filters[0].text, "redirect-gateway");
        assert_eq!(parsed.external_files().len(), 3);

        for directive in [
            "remote",
            "proto",
            "dev",
            "ca",
            "cert",
            "key",
            "auth-user-pass",
            "cipher",
            "data-ciphers",
            "redirect-gateway",
            "route",
            "dhcp-option",
            "pull-filter",
        ] {
            assert!(parsed.recognized_directives.contains(directive));
        }
    }

    #[test]
    fn ignores_directives_inside_inline_private_key_blocks() {
        let source = "client\n<key>\nremote attacker.example 443\nsecret-data\n</key>\nremote vpn.example 1194\n";

        let parsed = ParsedOvpnConfig::parse(source).expect("config should parse");

        assert_eq!(
            parsed
                .remote
                .as_ref()
                .expect("remote should be present")
                .host,
            "vpn.example"
        );
        assert!(parsed.external_files().is_empty());
    }

    #[test]
    fn relocates_external_files_and_drops_credential_file_reference() {
        let parsed = ParsedOvpnConfig::parse(SAMPLE_CONFIG).expect("config should parse");
        let replacements = parsed
            .external_files()
            .iter()
            .enumerate()
            .map(|(index, reference)| {
                (
                    reference.line_index,
                    format!("{}-{:02}.pem", reference.directive, index + 1),
                )
            })
            .collect::<HashMap<_, _>>();

        let relocated = parsed
            .render_imported(&replacements)
            .expect("config should render");

        assert!(relocated.contains("ca \"ca-01.pem\""));
        assert!(relocated.contains("cert \"cert-02.pem\""));
        assert!(relocated.contains("key \"key-03.pem\""));
        assert!(relocated.contains("auth-user-pass\n"));
        assert!(!relocated.contains("credentials.txt"));
        assert!(relocated.contains("pull-filter ignore \"redirect-gateway\""));
    }

    #[test]
    fn rejects_unterminated_quotes_without_echoing_line_contents() {
        let error = ParsedOvpnConfig::parse("key \"secret.key\n")
            .err()
            .expect("config should fail");

        assert_eq!(error.code(), "CONFIG_INVALID");
        assert!(!error.to_string().contains("secret.key"));
    }
}

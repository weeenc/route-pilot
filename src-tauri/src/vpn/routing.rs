use std::{
    fs,
    io::{self, Write},
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
};

use ipnet::IpNet;
use tokio::net::lookup_host;
use uuid::Uuid;

use crate::{
    domain::{Route, RouteSource},
    error::AppError,
    platform::{create_private_file, replace_file_atomically, sync_directory},
};

use super::parser::{tokenize_line, OvpnRouteDirective, ParsedOvpnConfig};

const RUNTIME_CONFIG_FILE: &str = "runtime.ovpn";
const IGNORE_REDIRECT_GATEWAY: &str = "pull-filter ignore \"redirect-gateway\"";
const IGNORE_PUSHED_ROUTE: &str = "pull-filter ignore \"route \"";
const IGNORE_PUSHED_ROUTE_IPV6: &str = "pull-filter ignore \"route-ipv6 \"";
const SPLIT_TUNNEL_DNS_PORT: u16 = 443;

/// App-owned OpenVPN configuration used for one process lifetime.
///
/// It is written beside `config.ovpn` so relative certificate paths keep the
/// same meaning. Dropping the guard removes only the generated runtime file.
pub struct RuntimeConfig {
    path: PathBuf,
    routes: Vec<Route>,
    ignores_server_routes: bool,
}

impl RuntimeConfig {
    pub async fn create(
        config_path: &Path,
        ignore_redirect_gateway: bool,
        split_tunnel_domains: &[String],
    ) -> Result<Self, AppError> {
        let config_path = fs::canonicalize(config_path).map_err(|_| AppError::ConfigInvalid {
            reason: "OpenVPN configuration does not exist".to_owned(),
        })?;
        if config_path
            .file_name()
            .is_some_and(|name| name == RUNTIME_CONFIG_FILE)
        {
            return Err(AppError::ConfigInvalid {
                reason: "the source configuration cannot be the runtime configuration".to_owned(),
            });
        }
        if !fs::metadata(&config_path)?.is_file() {
            return Err(AppError::ConfigInvalid {
                reason: "OpenVPN configuration is not a regular file".to_owned(),
            });
        }

        let source = fs::read_to_string(&config_path).map_err(|error| match error.kind() {
            io::ErrorKind::InvalidData => AppError::ConfigInvalid {
                reason: "OpenVPN configuration is not valid UTF-8".to_owned(),
            },
            _ => AppError::from(error),
        })?;
        let parsed = ParsedOvpnConfig::parse(&source)?;
        #[cfg(target_os = "macos")]
        parsed.validate_privileged_client_config()?;
        let directory = config_path
            .parent()
            .ok_or_else(|| AppError::ConfigInvalid {
                reason: "OpenVPN configuration has no parent directory".to_owned(),
            })?;
        validate_external_files(directory, &parsed)?;
        let mut routes: Vec<Route> = parsed
            .routes
            .iter()
            .filter_map(|directive| {
                parse_route_directive(directive, RouteSource::Config)
                    .ok()
                    .flatten()
            })
            .filter(|route| split_tunnel_domains.is_empty() || route.network.prefix_len() > 1)
            .collect();

        let split_tunnel_routes = resolve_split_tunnel_domains(split_tunnel_domains).await?;
        for route in &split_tunnel_routes {
            if !routes
                .iter()
                .any(|existing: &Route| existing.network == route.network)
            {
                routes.push(route.clone());
            }
        }

        let mut runtime_source = source;
        if !split_tunnel_domains.is_empty() {
            runtime_source = remove_local_full_tunnel_directives(&runtime_source);
        }
        let already_ignores_redirect = parsed.pull_filters.iter().any(|filter| {
            filter.action.eq_ignore_ascii_case("ignore")
                && filter.text.eq_ignore_ascii_case("redirect-gateway")
        });
        if (ignore_redirect_gateway || !split_tunnel_domains.is_empty())
            && !already_ignores_redirect
        {
            append_runtime_directive(&mut runtime_source, IGNORE_REDIRECT_GATEWAY);
        }
        if !split_tunnel_domains.is_empty() {
            append_pull_filter_if_missing(
                &mut runtime_source,
                &parsed,
                "route ",
                IGNORE_PUSHED_ROUTE,
            );
            append_pull_filter_if_missing(
                &mut runtime_source,
                &parsed,
                "route-ipv6 ",
                IGNORE_PUSHED_ROUTE_IPV6,
            );
        }
        append_split_tunnel_routes(&mut runtime_source, &split_tunnel_routes);

        let path = directory.join(RUNTIME_CONFIG_FILE);
        let temporary_path = directory.join(format!(".runtime-{}.tmp", Uuid::new_v4()));
        let write_result = write_runtime_file(&temporary_path, &path, &runtime_source, directory);
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary_path);
            let _ = fs::remove_file(&path);
        }
        write_result?;

        Ok(Self {
            path,
            routes,
            ignores_server_routes: !split_tunnel_domains.is_empty(),
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    #[must_use]
    pub fn ignores_server_routes(&self) -> bool {
        self.ignores_server_routes
    }
}

fn append_runtime_directive(source: &mut String, directive: &str) {
    if !source.is_empty() && !source.ends_with('\n') {
        source.push('\n');
    }
    source.push_str(directive);
    source.push('\n');
}

fn append_pull_filter_if_missing(
    source: &mut String,
    parsed: &ParsedOvpnConfig,
    text: &str,
    directive: &str,
) {
    let already_present = parsed.pull_filters.iter().any(|filter| {
        filter.action.eq_ignore_ascii_case("ignore") && filter.text.eq_ignore_ascii_case(text)
    });
    if !already_present {
        append_runtime_directive(source, directive);
    }
}

async fn resolve_split_tunnel_domains(domains: &[String]) -> Result<Vec<Route>, AppError> {
    let mut routes = Vec::new();

    for domain in domains {
        let addresses = if let Ok(address) = domain.parse::<IpAddr>() {
            vec![address]
        } else {
            lookup_host((domain.as_str(), SPLIT_TUNNEL_DNS_PORT))
                .await
                .map_err(|_| AppError::ConfigInvalid {
                    reason: "split-tunnel domain could not be resolved".to_owned(),
                })?
                .map(|address| address.ip())
                .collect::<Vec<_>>()
        };

        if addresses.is_empty() {
            return Err(AppError::ConfigInvalid {
                reason: "split-tunnel domain has no address".to_owned(),
            });
        }

        for address in addresses {
            let prefix = if address.is_ipv4() { 32 } else { 128 };
            let route = Route::new(address, prefix, None, None, RouteSource::SplitTunnel)?;
            if !routes
                .iter()
                .any(|existing: &Route| existing.network == route.network)
            {
                routes.push(route);
            }
        }
    }

    routes.sort_by_key(|route| route.network.to_string());
    Ok(routes)
}

fn remove_local_full_tunnel_directives(source: &str) -> String {
    let mut filtered = source
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') || trimmed.starts_with(';') {
                return true;
            }

            !is_local_full_tunnel_directive(trimmed)
        })
        .collect::<Vec<_>>()
        .join("\n");

    if source.ends_with('\n') {
        filtered.push('\n');
    }
    filtered
}

fn is_local_full_tunnel_directive(line: &str) -> bool {
    let Ok(tokens) = tokenize_line(line, 0) else {
        return false;
    };
    let Some(directive) = tokens.first() else {
        return false;
    };

    if directive.eq_ignore_ascii_case("redirect-gateway")
        || directive.eq_ignore_ascii_case("redirect-private")
    {
        return true;
    }

    let route = match directive.to_ascii_lowercase().as_str() {
        "route" => Some(OvpnRouteDirective {
            network: tokens.get(1).cloned().unwrap_or_default(),
            netmask: tokens.get(2).cloned(),
            gateway: tokens.get(3).cloned(),
        }),
        "route-ipv6" => Some(OvpnRouteDirective {
            network: tokens.get(1).cloned().unwrap_or_default(),
            netmask: None,
            gateway: tokens.get(2).cloned(),
        }),
        _ => None,
    };

    route
        .and_then(|directive| {
            parse_route_directive(&directive, RouteSource::Config)
                .ok()
                .flatten()
        })
        .is_some_and(|route| route.network.prefix_len() <= 1)
}

fn append_split_tunnel_routes(source: &mut String, routes: &[Route]) {
    if routes.is_empty() {
        return;
    }
    if !source.is_empty() && !source.ends_with('\n') {
        source.push('\n');
    }

    for route in routes {
        match route.network.addr() {
            IpAddr::V4(address) => {
                source.push_str(&format!("route {address} 255.255.255.255\n"));
            }
            IpAddr::V6(address) => {
                source.push_str(&format!("route-ipv6 {address}/128\n"));
            }
        }
    }
}

fn validate_external_files(
    config_directory: &Path,
    parsed: &ParsedOvpnConfig,
) -> Result<(), AppError> {
    for reference in parsed.external_files() {
        let candidate = if reference.path.is_absolute() {
            reference.path.clone()
        } else {
            config_directory.join(&reference.path)
        };
        let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                AppError::ConfigInvalid {
                    reason: format!(
                        "referenced {} file at line {} is missing",
                        reference.directive, reference.line_number
                    ),
                }
            } else {
                error.into()
            }
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(AppError::ConfigInvalid {
                reason: format!(
                    "referenced {} path at line {} is not a regular file",
                    reference.directive, reference.line_number
                ),
            });
        }

        let canonical = fs::canonicalize(&candidate)?;
        if !canonical.starts_with(config_directory) {
            return Err(AppError::ConfigInvalid {
                reason: format!(
                    "referenced {} path at line {} escaped the profile directory",
                    reference.directive, reference.line_number
                ),
            });
        }
    }
    Ok(())
}

impl Drop for RuntimeConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn write_runtime_file(
    temporary_path: &Path,
    destination: &Path,
    contents: &str,
    directory: &Path,
) -> Result<(), AppError> {
    let mut file = create_private_file(temporary_path)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    drop(file);
    replace_file_atomically(temporary_path, destination)?;
    sync_directory(directory)?;
    Ok(())
}

/// Parses one OpenVPN `route` or `route-ipv6` directive into a normalized
/// network. Hostnames and OpenVPN's dynamic gateway keywords cannot be resolved
/// without runtime state, so they are represented by `None` rather than guessed.
pub fn parse_route_directive(
    directive: &OvpnRouteDirective,
    source: RouteSource,
) -> Result<Option<Route>, AppError> {
    let (network, prefix) = if let Ok(network) = directive.network.parse::<IpNet>() {
        (network.addr(), network.prefix_len())
    } else {
        let Ok(address) = directive.network.parse::<IpAddr>() else {
            return Ok(None);
        };
        let prefix = match address {
            IpAddr::V4(_) => parse_ipv4_netmask(directive.netmask.as_deref())?,
            IpAddr::V6(_) => 128,
        };
        (address, prefix)
    };

    let gateway = directive.gateway.as_deref().and_then(parse_literal_gateway);
    Route::new(network, prefix, gateway, None, source).map(Some)
}

fn parse_ipv4_netmask(netmask: Option<&str>) -> Result<u8, AppError> {
    let Some(netmask) = netmask.filter(|value| !value.eq_ignore_ascii_case("default")) else {
        return Ok(32);
    };
    let mask = netmask
        .parse::<Ipv4Addr>()
        .map(u32::from)
        .map_err(|_| AppError::ConfigInvalid {
            reason: "route netmask is not a valid IPv4 netmask".to_owned(),
        })?;
    let prefix = mask.leading_ones() as u8;
    let expected = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    if mask != expected {
        return Err(AppError::ConfigInvalid {
            reason: "route netmask is not contiguous".to_owned(),
        });
    }
    Ok(prefix)
}

fn parse_literal_gateway(value: &str) -> Option<IpAddr> {
    if matches!(
        value.to_ascii_lowercase().as_str(),
        "default" | "vpn_gateway" | "net_gateway" | "remote_host"
    ) {
        return None;
    }
    value.parse().ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushReply {
    pub routes: Vec<Route>,
    pub requested_redirect_gateway: bool,
}

/// Extracts route options from an OpenVPN `PUSH_REPLY`. The input may be either
/// the control message itself or the message text carried by a management log.
#[must_use]
pub fn parse_push_reply(message: &str) -> Option<PushReply> {
    let start = message.find("PUSH_REPLY")?;
    let reply = &message[start..];
    if !reply
        .strip_prefix("PUSH_REPLY")
        .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(','))
    {
        return None;
    }

    let payload = reply
        .strip_prefix("PUSH_REPLY")?
        .strip_prefix(',')
        .unwrap_or_default()
        .trim_end_matches(['\'', '"']);
    let mut routes = Vec::new();
    let mut requested_redirect_gateway = false;

    for option in split_push_options(payload) {
        let Ok(tokens) = tokenize_line(option.trim(), 1) else {
            continue;
        };
        let Some(name) = tokens.first().map(|token| token.to_ascii_lowercase()) else {
            continue;
        };
        match name.as_str() {
            "route" => {
                let Some(network) = tokens.get(1) else {
                    continue;
                };
                let directive = OvpnRouteDirective {
                    network: network.clone(),
                    netmask: tokens.get(2).cloned(),
                    gateway: tokens.get(3).cloned(),
                };
                if let Ok(Some(route)) = parse_route_directive(&directive, RouteSource::ServerPush)
                {
                    routes.push(route);
                }
            }
            "route-ipv6" => {
                let Some(network) = tokens.get(1) else {
                    continue;
                };
                let directive = OvpnRouteDirective {
                    network: network.clone(),
                    netmask: None,
                    gateway: tokens.get(2).cloned(),
                };
                if let Ok(Some(route)) = parse_route_directive(&directive, RouteSource::ServerPush)
                {
                    routes.push(route);
                }
            }
            "redirect-gateway" => requested_redirect_gateway = true,
            _ => {}
        }
    }

    routes.sort_by_key(|route| route.network.to_string());
    routes.dedup();
    Some(PushReply {
        routes,
        requested_redirect_gateway,
    })
}

fn split_push_options(payload: &str) -> Vec<String> {
    let mut options = Vec::new();
    let mut option = String::new();
    let mut quote = None;
    let mut escaped = false;

    for character in payload.chars() {
        if escaped {
            option.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' {
            option.push(character);
            escaped = true;
            continue;
        }
        if let Some(active_quote) = quote {
            option.push(character);
            if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
            option.push(character);
        } else if character == ',' {
            options.push(std::mem::take(&mut option));
        } else {
            option.push(character);
        }
    }
    options.push(option);
    options
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{
        parse_push_reply, parse_route_directive, RuntimeConfig, IGNORE_PUSHED_ROUTE,
        IGNORE_PUSHED_ROUTE_IPV6, IGNORE_REDIRECT_GATEWAY,
    };
    use crate::{
        domain::RouteSource,
        vpn::parser::{OvpnRouteDirective, ParsedOvpnConfig},
    };

    #[test]
    fn parses_ipv4_routes_with_contiguous_netmasks_and_gateways() {
        let route = parse_route_directive(
            &OvpnRouteDirective {
                network: "10.10.15.20".to_owned(),
                netmask: Some("255.255.0.0".to_owned()),
                gateway: Some("10.8.0.1".to_owned()),
            },
            RouteSource::Config,
        )
        .expect("route should parse")
        .expect("literal route should be represented");

        assert_eq!(route.network.to_string(), "10.10.0.0/16");
        assert_eq!(
            route.gateway.expect("gateway should parse").to_string(),
            "10.8.0.1"
        );
        assert_eq!(route.source, RouteSource::Config);

        let invalid = parse_route_directive(
            &OvpnRouteDirective {
                network: "10.0.0.0".to_owned(),
                netmask: Some("255.0.255.0".to_owned()),
                gateway: None,
            },
            RouteSource::Config,
        );
        assert!(invalid.is_err());
    }

    #[test]
    fn skips_dynamic_route_names_without_guessing_their_addresses() {
        let route = parse_route_directive(
            &OvpnRouteDirective {
                network: "remote_host".to_owned(),
                netmask: Some("255.255.255.255".to_owned()),
                gateway: Some("net_gateway".to_owned()),
            },
            RouteSource::Config,
        )
        .expect("dynamic route should be accepted");

        assert_eq!(route, None);
    }

    #[test]
    fn parses_routes_and_redirect_request_from_management_push_log() {
        let reply = parse_push_reply(
            ">LOG:1700000000,I,PUSH: Received control message: 'PUSH_REPLY,route 10.0.0.0 255.0.0.0,route 172.20.10.5 255.255.0.0 10.8.0.1,route-ipv6 fd00:abcd::/48,redirect-gateway def1,dhcp-option DNS 10.8.0.1'",
        )
        .expect("push reply should be recognized");

        assert!(reply.requested_redirect_gateway);
        assert_eq!(reply.routes.len(), 3);
        assert_eq!(reply.routes[0].network.to_string(), "10.0.0.0/8");
        assert_eq!(reply.routes[1].network.to_string(), "172.20.0.0/16");
        assert_eq!(reply.routes[2].network.to_string(), "fd00:abcd::/48");
        assert!(reply
            .routes
            .iter()
            .all(|route| route.source == RouteSource::ServerPush));
    }

    #[tokio::test]
    async fn creates_isolated_runtime_config_and_removes_it_on_drop() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let config_path = workspace.path().join("config.ovpn");
        let original = "client\nroute 10.10.0.0 255.255.0.0\n";
        fs::write(&config_path, original).expect("source config should be written");

        let runtime = RuntimeConfig::create(&config_path, true, &[])
            .await
            .expect("runtime config should be generated");
        let runtime_path = runtime.path().to_path_buf();
        let generated =
            fs::read_to_string(&runtime_path).expect("runtime config should be readable");

        assert_eq!(fs::read_to_string(&config_path).unwrap(), original);
        assert!(generated.contains("pull-filter ignore \"redirect-gateway\""));
        assert!(!generated.contains("route-nopull"));
        assert_eq!(runtime.routes()[0].network.to_string(), "10.10.0.0/16");

        drop(runtime);
        assert!(!runtime_path.exists());
    }

    #[tokio::test]
    async fn does_not_duplicate_existing_redirect_filter() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let config_path = workspace.path().join("config.ovpn");
        fs::write(
            &config_path,
            "client\npull-filter ignore \"redirect-gateway\"\n",
        )
        .expect("source config should be written");

        let runtime = RuntimeConfig::create(&config_path, true, &[])
            .await
            .expect("runtime config should be generated");
        let generated =
            fs::read_to_string(runtime.path()).expect("runtime config should be readable");

        assert_eq!(generated.matches(IGNORE_REDIRECT_GATEWAY).count(), 1);
    }

    #[tokio::test]
    async fn leaves_redirect_filter_out_when_profile_setting_is_disabled() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let config_path = workspace.path().join("config.ovpn");
        fs::write(&config_path, "client\n").expect("source config should be written");

        let runtime = RuntimeConfig::create(&config_path, false, &[])
            .await
            .expect("runtime config should be generated");
        let generated =
            fs::read_to_string(runtime.path()).expect("runtime config should be readable");

        assert_eq!(generated, "client\n");
        assert!(!generated.contains("redirect-gateway"));
        assert!(!generated.contains("route-nopull"));
    }

    #[tokio::test]
    async fn creates_split_tunnel_routes_and_removes_local_full_tunnel_options() {
        let workspace = TempDir::new().expect("temporary workspace should be created");
        let config_path = workspace.path().join("config.ovpn");
        let original = "client\nREDIRECT-GATEWAY def1\nredirect-private\nroute 0.0.0.0 128.0.0.0\nroute 10.0.0.0 255.0.0.0\n";
        fs::write(&config_path, original).expect("source config should be written");

        let domains = vec!["192.0.2.10".to_owned(), "2001:db8::10".to_owned()];
        let runtime = RuntimeConfig::create(&config_path, false, &domains)
            .await
            .expect("split-tunnel runtime config should be generated");
        let generated = fs::read_to_string(runtime.path()).expect("runtime config should exist");

        assert!(!generated.contains("REDIRECT-GATEWAY def1"));
        assert!(!generated.contains("redirect-private"));
        assert!(!generated.contains("route 0.0.0.0 128.0.0.0"));
        assert!(generated.contains(IGNORE_REDIRECT_GATEWAY));
        assert!(generated.contains(IGNORE_PUSHED_ROUTE));
        assert!(generated.contains(IGNORE_PUSHED_ROUTE_IPV6));
        let parsed = ParsedOvpnConfig::parse(&generated)
            .expect("generated split-tunnel config should parse");
        assert!(!parsed.pull_filters.iter().any(|filter| {
            filter.action.eq_ignore_ascii_case("ignore")
                && "route-gateway 192.0.2.1".starts_with(&filter.text)
        }));
        assert!(generated.contains("route 192.0.2.10 255.255.255.255"));
        assert!(generated.contains("route-ipv6 2001:db8::10/128"));
        assert!(runtime.ignores_server_routes());
        assert!(runtime
            .routes()
            .iter()
            .any(|route| route.source == RouteSource::SplitTunnel));
        assert_eq!(
            runtime
                .routes()
                .iter()
                .filter(|route| route.network.prefix_len() <= 1)
                .count(),
            0
        );
    }

    #[test]
    fn keeps_comments_when_removing_local_full_tunnel_options() {
        let source = "# redirect-gateway def1\n; redirect-private\nredirect-gateway def1\n";

        assert_eq!(
            super::remove_local_full_tunnel_directives(source),
            "# redirect-gateway def1\n; redirect-private\n"
        );
    }

    #[tokio::test]
    async fn rejects_invalid_runtime_config_without_touching_the_source() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let config_path = workspace.path().join("config.ovpn");
        let original = "client\nca \"unterminated.crt\n";
        fs::write(&config_path, original).expect("source config should be written");

        let error = RuntimeConfig::create(&config_path, true, &[])
            .await
            .err()
            .expect("invalid config should be rejected");

        assert_eq!(error.code(), "CONFIG_INVALID");
        assert_eq!(fs::read_to_string(&config_path).unwrap(), original);
        assert!(!workspace.path().join("runtime.ovpn").exists());
    }

    #[tokio::test]
    async fn rejects_missing_external_certificate_before_process_start() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let config_path = workspace.path().join("config.ovpn");
        fs::write(&config_path, "client\nca missing-ca.crt\n")
            .expect("source config should be written");

        let error = RuntimeConfig::create(&config_path, true, &[])
            .await
            .err()
            .expect("missing certificate should be rejected");

        assert_eq!(error.code(), "CONFIG_INVALID");
        assert!(!workspace.path().join("runtime.ovpn").exists());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn rejects_script_hooks_before_privileged_process_start() {
        let workspace = TempDir::new().expect("temporary directory should be created");
        let config_path = workspace.path().join("config.ovpn");
        fs::write(
            &config_path,
            "client\nscript-security 2\nup /tmp/untrusted-script\n",
        )
        .expect("source config should be written");

        let error = RuntimeConfig::create(&config_path, false, &[])
            .await
            .err()
            .expect("privileged script hook should be rejected");

        assert_eq!(error.code(), "CONFIG_INVALID");
        assert!(!workspace.path().join("runtime.ovpn").exists());
    }
}

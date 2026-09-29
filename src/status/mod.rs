// SPDX-License-Identifier: Apache-2.0

use std::{net::Ipv4Addr, str::FromStr};

use anyhow::{anyhow, Context};

#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub use unix::{get_shutdown_eventfd, status_listener};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UriScheme {
    Tcp,
    Unix,
    #[default]
    None,
}

impl FromStr for UriScheme {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tcp" => Ok(Self::Tcp),
            "unix" => Ok(Self::Unix),
            "none" => Ok(Self::None),
            _ => Err(anyhow!("invalid scheme")),
        }
    }
}

/// Socket address in which the restful URI socket should listen on. Identical to Rust's
/// SocketAddrV4, but requires a modified FromStr implementation due to how the address is
/// presented on the command line.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum RestfulUri {
    Tcp(Ipv4Addr, u16),
    Unix(String),
    #[default]
    None,
}

impl FromStr for RestfulUri {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let expression = regex::Regex::new(r"^(?P<scheme>none|tcp|unix)://(?P<value>.*)").unwrap();
        let Some(cap) = expression.captures(s) else {
            return Err(anyhow!("invalid scheme input"));
        };
        let scheme = &cap["scheme"];
        let value = &cap["value"];
        match UriScheme::from_str(scheme)? {
            UriScheme::Tcp => {
                let (ip_addr, port) = parse_tcp_input(value)?;
                Ok(Self::Tcp(ip_addr, port))
            }
            UriScheme::Unix => {
                if value.is_empty() {
                    return Err(anyhow!("empty unix socket path"));
                }
                Ok(Self::Unix(value.to_string()))
            }
            UriScheme::None => Ok(Self::None),
        }
    }
}

fn parse_tcp_input(input: &str) -> Result<(Ipv4Addr, u16), anyhow::Error> {
    let mut parts: Vec<String> = input.split(':').map(|s| s.to_string()).collect();
    if parts.len() != 2 {
        return Err(anyhow!("restful URI formatted incorrectly"));
    }

    // Ipv4Address's FromStr does not understand that the "localhost" IP address translates to
    // 127.0.0.1, this must be manually translated.
    if &parts[0][..] == "localhost" {
        parts[0] = String::from("127.0.0.1");
    }

    let ip_addr =
        Ipv4Addr::from_str(&parts[0]).context("restful URI IP address formatted incorrectly")?;
    let port = u16::from_str(&parts[1]).context("restful URI port number formatted incorrectly")?;
    Ok((ip_addr, port))
}

#[allow(unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_unix_scheme() {
        assert_eq!(
            RestfulUri::Unix("/tmp/path".to_string()),
            RestfulUri::from_str("unix:///tmp/path").unwrap()
        );
    }

    #[test]
    fn parse_unix_scheme_missing_path() {
        assert_eq!(
            anyhow!("empty unix socket path").to_string(),
            RestfulUri::from_str("unix://").err().unwrap().to_string()
        );
    }

    #[test]
    fn parse_unix_scheme_missing_slashes() {
        assert_eq!(
            anyhow!("invalid scheme input").to_string(),
            RestfulUri::from_str("unix:").err().unwrap().to_string()
        );
    }

    #[test]
    fn parse_unix_scheme_misspelling() {
        assert_eq!(
            anyhow!("invalid scheme input").to_string(),
            RestfulUri::from_str("uni://path")
                .err()
                .unwrap()
                .to_string()
        );
    }

    #[test]
    fn parse_valid_tcp_scheme() {
        assert_eq!(
            RestfulUri::Tcp(Ipv4Addr::new(127, 0, 0, 1), 8080),
            RestfulUri::from_str("tcp://localhost:8080").unwrap(),
        );
    }

    #[test]
    fn parse_tcp_scheme_missing_port() {
        assert_eq!(
            anyhow!("restful URI formatted incorrectly").to_string(),
            RestfulUri::from_str("tcp://localhost")
                .err()
                .unwrap()
                .to_string()
        );
    }

    #[test]
    fn parse_tcp_scheme_with_unix_path() {
        assert_eq!(
            anyhow!("restful URI formatted incorrectly").to_string(),
            RestfulUri::from_str("tcp:///tmp/path")
                .err()
                .unwrap()
                .to_string(),
        );
    }

    #[test]
    fn parse_valid_none_scheme() {
        assert_eq!(RestfulUri::None, RestfulUri::from_str("none://").unwrap());
    }

    #[test]
    fn parse_none_scheme_missing_postfix() {
        assert_eq!(
            anyhow!("invalid scheme input").to_string(),
            RestfulUri::from_str("none").err().unwrap().to_string(),
        );
    }

    #[test]
    fn parse_random_string_scheme() {
        assert_eq!(
            anyhow!("invalid scheme input").to_string(),
            RestfulUri::from_str("foobar").err().unwrap().to_string(),
        );
    }
}

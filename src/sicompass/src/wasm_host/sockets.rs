//! TCP for a plugin with a `sockets` grant: the approved `host:port` pairs.
//!
//! `wasi:sockets/ip-name-lookup` is never linked, because it resolves any name
//! and a lookup of a made-up name is enough to leak data. So a plugin resolves
//! through `sicompass:plugin/sockets.resolve` (only approved pairs) and connects
//! by address, and [`Endpoints::permits`] lets a connection through only to an
//! address one of the approved names resolves to, on that name's port. Listed
//! endpoints may be local (a mail bridge on `localhost` is a real use); the user
//! approved them by name.

use std::net::{SocketAddr, ToSocketAddrs};

use wasmtime_wasi::sockets::SocketAddrUse;

use super::HostState;
use super::sicompass::plugin as wit;

/// The approved endpoints, parsed.
#[derive(Debug, Clone)]
pub struct Endpoints(Vec<(String, u16)>);

impl Endpoints {
    /// Parse `host:port` entries (a bracketed IPv6 host is accepted).
    pub fn parse(entries: &[String]) -> Result<Self, String> {
        let mut out = Vec::new();
        for e in entries {
            let (host, port) = e
                .rsplit_once(':')
                .ok_or_else(|| format!("`{e}` is not host:port"))?;
            let port: u16 = port.parse().map_err(|_| format!("`{e}` has no valid port"))?;
            let host = host.trim_start_matches('[').trim_end_matches(']');
            if host.is_empty() {
                return Err(format!("`{e}` has no host"));
            }
            out.push((host.to_ascii_lowercase(), port));
        }
        Ok(Endpoints(out))
    }

    /// Whether `host:port` is one of the approved endpoints.
    pub fn lists(&self, host: &str, port: u16) -> bool {
        let host = host.to_ascii_lowercase();
        self.0.iter().any(|(h, p)| *h == host && *p == port)
    }

    /// The host's socket check. A connection only to a resolved approved
    /// address; a bind only as the implicit one `connect` makes. Nothing else:
    /// no listening, no accepting, no UDP.
    pub fn permits(&self, addr: SocketAddr, usage: SocketAddrUse) -> bool {
        match usage {
            SocketAddrUse::TcpConnect => self.0.iter().any(|(h, p)| {
                *p == addr.port()
                    && (h.as_str(), *p)
                        .to_socket_addrs()
                        .is_ok_and(|mut it| it.any(|a| a.ip() == addr.ip()))
            }),
            SocketAddrUse::TcpBind => addr.ip().is_unspecified() && addr.port() == 0,
            _ => false,
        }
    }
}

impl wit::sockets::Host for HostState {
    fn resolve(&mut self, host: String, port: u16) -> Result<Vec<String>, String> {
        let endpoints = Endpoints::parse(&self.sockets_allowed)?;
        if !endpoints.lists(&host, port) {
            return Err(format!(
                "`{host}:{port}` is not among the connections this plugin may open"
            ));
        }
        let addrs: Vec<String> = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| format!("{host}: {e}"))?
            .map(|a| a.ip().to_string())
            .collect();
        if addrs.is_empty() {
            Err(format!("{host} resolved to no address"))
        } else {
            Ok(addrs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(v: &[&str]) -> Endpoints {
        Endpoints::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn only_approved_addresses_and_ports_connect() {
        let e = ep(&["127.0.0.1:993"]);
        assert!(e.permits("127.0.0.1:993".parse().unwrap(), SocketAddrUse::TcpConnect));
        assert!(!e.permits("127.0.0.1:994".parse().unwrap(), SocketAddrUse::TcpConnect));
        assert!(!e.permits("10.0.0.1:993".parse().unwrap(), SocketAddrUse::TcpConnect));
    }

    #[test]
    fn nothing_but_connect_and_its_implicit_bind() {
        let e = ep(&["127.0.0.1:993"]);
        assert!(e.permits("0.0.0.0:0".parse().unwrap(), SocketAddrUse::TcpBind));
        assert!(!e.permits("0.0.0.0:8080".parse().unwrap(), SocketAddrUse::TcpBind));
        assert!(!e.permits("127.0.0.1:993".parse().unwrap(), SocketAddrUse::TcpListen));
        assert!(!e.permits("127.0.0.1:993".parse().unwrap(), SocketAddrUse::UdpBind));
    }

    #[test]
    fn entries_parse_and_bad_ones_are_named() {
        assert!(ep(&["IMAP.example.org:993", "[::1]:25"]).lists("imap.example.org", 993));
        assert!(Endpoints::parse(&["example.org".to_owned()]).is_err());
        assert!(Endpoints::parse(&["example.org:http".to_owned()]).is_err());
        assert!(Endpoints::parse(&[":993".to_owned()]).is_err());
    }
}

//! TCP for a plugin with a `sockets` grant: the approved `host:port` pairs.
//!
//! `wasi:sockets/ip-name-lookup` is never linked, because it resolves any name
//! and a lookup of a made-up name is enough to leak data. So a plugin resolves
//! through `sicompass:plugin/sockets.resolve` (only approved pairs) and connects
//! by address, and [`Endpoints::permits`] lets a connection through only to an
//! address one of the approved names resolves to, on that name's port. Listed
//! endpoints may be local (a mail bridge on `localhost` is a real use); the user
//! approved them by name.
//!
//! `*:<port>` is any public server on that port, for a plugin whose servers the
//! user types in (a mail client). Internal addresses stay out of reach there:
//! a name is resolved first and only its public addresses are answered, so a
//! name pointing inward gets nowhere either.

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

    /// Whether `host:port` is one of the approved endpoints, by name.
    pub fn lists(&self, host: &str, port: u16) -> bool {
        let host = host.to_ascii_lowercase();
        self.0.iter().any(|(h, p)| *h == host && *p == port)
    }

    /// Whether any public server may be reached on `port` (`*:<port>`).
    pub fn any_on(&self, port: u16) -> bool {
        self.0.iter().any(|(h, p)| h == ANY_HOST && *p == port)
    }

    /// The host's socket check. A connection only to a resolved approved
    /// address; a bind only as the implicit one `connect` makes. Nothing else:
    /// no listening, no accepting, no UDP.
    pub fn permits(&self, addr: SocketAddr, usage: SocketAddrUse) -> bool {
        match usage {
            SocketAddrUse::TcpConnect => {
                (self.any_on(addr.port()) && !is_internal(addr.ip()))
                    || self.0.iter().any(|(h, p)| {
                        h != ANY_HOST
                            && *p == addr.port()
                            && (h.as_str(), *p)
                                .to_socket_addrs()
                                .is_ok_and(|mut it| it.any(|a| a.ip() == addr.ip()))
                    })
            }
            SocketAddrUse::TcpBind => addr.ip().is_unspecified() && addr.port() == 0,
            _ => false,
        }
    }
}

/// The host part of an entry meaning any public server.
const ANY_HOST: &str = "*";

/// Whether `ip` is inside the machine or its network.
fn is_internal(ip: std::net::IpAddr) -> bool {
    super::host_fetch::is_internal_host(&ip.to_string())
}

impl wit::sockets::Host for HostState {
    fn resolve(&mut self, host: String, port: u16) -> Result<Vec<String>, String> {
        let endpoints = Endpoints::parse(&self.sockets_allowed)?;
        let named = endpoints.lists(&host, port);
        if !named && !endpoints.any_on(port) {
            return Err(format!(
                "`{host}:{port}` is not among the connections this plugin may open"
            ));
        }
        let addrs: Vec<String> = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| format!("{host}: {e}"))?
            // Reached only as "any server": its internal addresses are not.
            .filter(|a| named || !is_internal(a.ip()))
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
    fn any_server_on_a_port_is_public_servers_on_that_port_only() {
        let e = ep(&["*:993"]);
        assert!(e.any_on(993) && !e.any_on(994));
        assert!(e.permits("93.184.215.14:993".parse().unwrap(), SocketAddrUse::TcpConnect));
        assert!(!e.permits("93.184.215.14:994".parse().unwrap(), SocketAddrUse::TcpConnect));
        for inside in ["127.0.0.1:993", "10.0.0.1:993", "192.168.1.2:993", "[::1]:993"] {
            assert!(
                !e.permits(inside.parse().unwrap(), SocketAddrUse::TcpConnect),
                "{inside} is internal"
            );
        }
    }

    #[test]
    fn resolving_for_any_server_answers_no_internal_address() {
        let mut s = HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new());
        s.sockets_allowed = vec!["*:993".to_owned()];
        use wit::sockets::Host;
        let err = s.resolve("localhost".to_owned(), 993).unwrap_err();
        assert!(err.contains("no address"), "{err}");
        assert!(s.resolve("localhost".to_owned(), 25).is_err(), "not that port");
        // A listed name keeps its internal addresses: the user approved it.
        s.sockets_allowed = vec!["localhost:993".to_owned()];
        assert!(!s.resolve("localhost".to_owned(), 993).unwrap().is_empty());
    }

    #[test]
    fn entries_parse_and_bad_ones_are_named() {
        assert!(ep(&["IMAP.example.org:993", "[::1]:25"]).lists("imap.example.org", 993));
        assert!(Endpoints::parse(&["example.org".to_owned()]).is_err());
        assert!(Endpoints::parse(&["example.org:http".to_owned()]).is_err());
        assert!(Endpoints::parse(&[":993".to_owned()]).is_err());
    }
}

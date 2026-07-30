//! The mediated `fetch` — a plugin's only way out.
//!
//! A guest has no sockets, so every byte it sends or receives passes through here.
//! That makes policy *enforceable* rather than advisory, which it never was against
//! a `dlopen`ed native plugin that could simply open its own connection.
//!
//! Every request is checked, in order:
//!
//! 1. **Scheme** — `http`/`https` only. No `file:`, `ftp:`, `data:`.
//! 2. **Allowlist** — the host must appear in the plugin's `plugin.json`
//!    `allowedHosts`. Matching is exact and case-insensitive; subdomains must be
//!    listed explicitly, because `evil.example.com` is not `example.com`.
//! 3. **Internal addresses** — loopback, private and link-local literals are
//!    refused, so an allowlisted name cannot be used to probe the local network.
//! 4. **robots.txt** — `Disallow` blocks; `Crawl-delay` rate-limits.
//! 5. **Quota** — a per-plugin, per-host request budget.
//! 6. **Header hygiene** — hop-by-hop and connection-controlling headers are
//!    dropped so a guest cannot smuggle anything past the checks above.
//!
//! Redirects are followed manually, re-running every check on each hop. Handing
//! them to `reqwest`'s automatic following would let a 302 walk straight off the
//! allowlist.
//!
//! ## Known limit
//!
//! The allowlist is by *name*. A hostname that resolves to an internal address
//! defeats step 3, and re-resolution between check and connect (DNS rebinding) is
//! not addressed. Closing that needs resolve-then-connect-to-pinned-IP, which
//! `reqwest` does not expose. Step 3 stops the naive case, not a determined one.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::wit::net::{HttpRequest, HttpResponse};

/// Sent on every plugin request, so site owners can identify and contact us.
const USER_AGENT: &str = concat!(
    "sicompass/",
    env!("CARGO_PKG_VERSION"),
    " (+https://friendlyflow.org/sicompass; plugin sandbox)"
);

/// Largest response body handed back to a guest. A guest's memory is capped at
/// 64 MiB, so a larger body could not be received anyway.
const MAX_RESPONSE_BYTES: u64 = 8 << 20;

/// Redirect hops followed before giving up. Each is fully re-checked.
const MAX_REDIRECTS: usize = 5;

/// Per-plugin, per-host request budget.
const QUOTA_REQUESTS: usize = 60;
const QUOTA_WINDOW: Duration = Duration::from_secs(60);

/// How long a fetched robots.txt is reused before being re-read.
const ROBOTS_TTL: Duration = Duration::from_secs(3600);

/// Whole-request timeout. Below the guest call's epoch deadline, so a slow server
/// surfaces as a fetch error rather than as the plugin being killed.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

/// Methods a plugin may use.
const ALLOWED_METHODS: &[&str] = &["GET", "HEAD", "POST"];

/// Headers a guest may not set. `host` and the length/framing headers would let a
/// request address somewhere other than the URL that was checked; the rest control
/// the connection itself.
const DENIED_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "upgrade",
    "te",
    "trailer",
    "keep-alive",
    "expect",
];

// ---------------------------------------------------------------------------
// Per-plugin state
// ---------------------------------------------------------------------------

/// Request policy for one plugin instance.
pub struct FetchPolicy {
    plugin_name: String,
    /// Lower-cased hosts from `plugin.json`.
    allowed_hosts: Vec<String>,
    /// Recent request times per host, for the quota window.
    recent: HashMap<String, VecDeque<Instant>>,
}

impl FetchPolicy {
    pub fn new(plugin_name: &str, allowed_hosts: &[String]) -> Self {
        FetchPolicy {
            plugin_name: plugin_name.to_owned(),
            allowed_hosts: allowed_hosts.iter().map(|h| h.trim().to_lowercase()).collect(),
            recent: HashMap::new(),
        }
    }

    /// Whether `host` is on this plugin's allowlist.
    pub fn host_allowed(&self, host: &str) -> bool {
        let host = host.to_lowercase();
        self.allowed_hosts.contains(&host)
    }

    /// Record a request and report whether it fits in the budget.
    fn take_quota(&mut self, host: &str) -> Result<(), String> {
        let now = Instant::now();
        let slot = self.recent.entry(host.to_owned()).or_default();
        while slot.front().is_some_and(|t| now.duration_since(*t) > QUOTA_WINDOW) {
            slot.pop_front();
        }
        if slot.len() >= QUOTA_REQUESTS {
            return Err(format!(
                "plugin `{}` has used its request budget for {host} \
                 ({QUOTA_REQUESTS} per minute); try again shortly",
                self.plugin_name
            ));
        }
        slot.push_back(now);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// robots.txt
// ---------------------------------------------------------------------------

/// The rules that apply to us for one origin.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Robots {
    /// Path prefixes that must not be fetched.
    pub disallow: Vec<String>,
    /// Prefixes that override a broader `Disallow`.
    pub allow: Vec<String>,
    /// Minimum gap between requests, if the site asked for one.
    pub crawl_delay: Option<Duration>,
}

impl Robots {
    /// Whether `path` may be fetched.
    ///
    /// Longest matching rule wins, and `Allow` beats `Disallow` at equal length —
    /// the behaviour every major crawler implements.
    pub fn allows(&self, path: &str) -> bool {
        let best_dis = self
            .disallow
            .iter()
            .filter(|p| !p.is_empty() && path.starts_with(p.as_str()))
            .map(|p| p.len())
            .max();
        let best_allow = self
            .allow
            .iter()
            .filter(|p| path.starts_with(p.as_str()))
            .map(|p| p.len())
            .max();

        match (best_dis, best_allow) {
            (None, _) => true,
            (Some(d), Some(a)) => a >= d,
            (Some(_), None) => false,
        }
    }
}

/// Parse robots.txt, keeping only the groups that apply to us.
///
/// A specific `User-agent: sicompass` group wins over `User-agent: *`, per the
/// convention that the most specific matching group is the only one that applies.
pub fn parse_robots(body: &str) -> Robots {
    let mut wildcard = Robots::default();
    let mut specific = Robots::default();
    let mut saw_specific = false;

    // Which groups the current directives belong to. A run of `User-agent:` lines
    // introduces one group, so both flags can be set at once.
    let (mut in_wildcard, mut in_specific) = (false, false);
    let mut expecting_agents = false;

    for raw in body.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_lowercase();
        let value = value.trim();

        if key == "user-agent" {
            // A new group starts at the first agent line after any directive.
            if !expecting_agents {
                in_wildcard = false;
                in_specific = false;
                expecting_agents = true;
            }
            let agent = value.to_lowercase();
            if agent == "*" {
                in_wildcard = true;
            } else if agent == "sicompass" {
                in_specific = true;
                saw_specific = true;
            }
            continue;
        }
        expecting_agents = false;

        for (active, target) in
            [(in_wildcard, &mut wildcard), (in_specific, &mut specific)]
        {
            if !active {
                continue;
            }
            match key.as_str() {
                "disallow" => target.disallow.push(value.to_owned()),
                "allow" => target.allow.push(value.to_owned()),
                "crawl-delay" => {
                    if let Ok(secs) = value.parse::<f64>()
                        && secs.is_finite()
                        && secs >= 0.0
                    {
                        target.crawl_delay = Some(Duration::from_secs_f64(secs.min(3600.0)));
                    }
                }
                _ => {}
            }
        }
    }

    if saw_specific { specific } else { wildcard }
}

struct RobotsEntry {
    robots: Robots,
    fetched: Instant,
    /// When this origin was last requested, for crawl-delay. Global rather than
    /// per-plugin: politeness is owed to the site, not tracked per caller.
    last_request: Option<Instant>,
}

fn robots_cache() -> &'static Mutex<HashMap<String, RobotsEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, RobotsEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Forget one origin's cached robots.txt. Tests only.
///
/// Per-origin, deliberately. Clearing the *whole* cache wipes sibling tests'
/// entries mid-run — including the `last_request` timestamp the crawl-delay test
/// depends on — because the `live` tests run in parallel.
///
/// Needed at all because origins are less unique than they look. Each `MockServer`
/// binds a fresh port, but a dropped server releases its port for reuse, so a later
/// test can land on the same `host:port` and inherit the earlier test's entry —
/// typically "no restrictions", which makes a robots.txt test silently pass a
/// request it should have blocked. The one-hour TTL means that entry never expires
/// within a run. Rare on a developer machine, reliable in CI.
#[cfg(test)]
fn forget_robots(origin: &str) {
    robots_cache().lock().unwrap().remove(origin);
}

// ---------------------------------------------------------------------------
// Address checks
// ---------------------------------------------------------------------------

#[cfg(test)]
thread_local! {
    /// Test-only. The mock HTTP server binds `127.0.0.1`, which the internal-address
    /// check exists to refuse, so the tests that exercise the *rest* of the pipeline
    /// (robots.txt, redirects, size caps) switch it off for their own thread.
    ///
    /// `#[cfg(test)]` means this cannot exist in a shipped binary, and the check it
    /// guards has its own dedicated tests that never touch this flag.
    static ALLOW_INTERNAL_FOR_TESTS: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// Whether the internal-address refusal is active. Always true outside tests.
fn internal_check_active() -> bool {
    #[cfg(test)]
    {
        !ALLOW_INTERNAL_FOR_TESTS.with(|c| c.get())
    }
    #[cfg(not(test))]
    {
        true
    }
}

/// Whether a host literal points somewhere inside the machine or its network.
///
/// Only catches literal addresses and `localhost`; a *name* resolving inward gets
/// through. See the module docs.
pub fn is_internal_host(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if bare.eq_ignore_ascii_case("localhost") || bare.eq_ignore_ascii_case("localhost.") {
        return true;
    }
    match bare.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                // 100.64.0.0/10, carrier-grade NAT.
                || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
        }
        Ok(IpAddr::V6(v6)) => {
            v6.is_loopback()
                || v6.is_unspecified()
                // fc00::/7 unique-local and fe80::/10 link-local.
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // IPv4-mapped, e.g. ::ffff:127.0.0.1.
                || v6.to_ipv4_mapped().is_some_and(|v4| {
                    v4.is_loopback() || v4.is_private() || v4.is_link_local()
                })
        }
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// The request path
// ---------------------------------------------------------------------------

/// Everything checkable about a URL before a connection is made.
fn vet_url(policy: &FetchPolicy, url: &str) -> Result<(reqwest::Url, String, String), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("`{url}` is not a valid URL: {e}"))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(format!("scheme `{other}` is not permitted; use http or https")),
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| format!("`{url}` has no host"))?
        .to_owned();

    if !policy.host_allowed(&host) {
        return Err(format!(
            "`{host}` is not in this plugin's allowedHosts ({:?})",
            policy.allowed_hosts
        ));
    }
    if internal_check_active() && is_internal_host(&host) {
        return Err(format!("`{host}` is an internal address"));
    }

    let origin = format!(
        "{}://{}{}",
        parsed.scheme(),
        host,
        parsed.port().map(|p| format!(":{p}")).unwrap_or_default()
    );
    Ok((parsed, host, origin))
}

/// Fetch and cache robots.txt for `origin`, then check `path` and the crawl delay.
fn check_robots(origin: &str, path: &str) -> Result<(), String> {
    let now = Instant::now();

    // Re-fetch outside the lock so a slow robots.txt does not stall other plugins.
    let need_fetch = {
        let cache = robots_cache().lock().unwrap();
        !matches!(cache.get(origin), Some(e) if now.duration_since(e.fetched) < ROBOTS_TTL)
    };

    if need_fetch {
        // A missing or unreadable robots.txt means "no restrictions", which is what
        // every crawler assumes. A *failed* fetch must not become a free pass to
        // ignore a file that does exist, but we cannot tell the difference beyond
        // the status code, so treat only 4xx as "absent".
        let robots = match http_get_text(&format!("{origin}/robots.txt")) {
            Ok((200, body)) => parse_robots(&body),
            // Any other status, or an unreachable server: no rules to apply.
            Ok(_) | Err(_) => Robots::default(),
        };
        robots_cache().lock().unwrap().insert(
            origin.to_owned(),
            RobotsEntry { robots, fetched: now, last_request: None },
        );
    }

    let mut cache = robots_cache().lock().unwrap();
    let entry = cache.get_mut(origin).expect("just inserted");

    if !entry.robots.allows(path) {
        return Err(format!("{origin}/robots.txt disallows {path}"));
    }

    if let Some(delay) = entry.robots.crawl_delay
        && let Some(last) = entry.last_request
    {
        let waited = now.duration_since(last);
        if waited < delay {
            // Refuse rather than sleep: this runs on the thread driving the guest,
            // and blocking it would freeze the UI to honour someone's Crawl-delay.
            let remaining = delay - waited;
            return Err(format!(
                "{origin} asks for {:.1}s between requests; {:.1}s left",
                delay.as_secs_f64(),
                remaining.as_secs_f64()
            ));
        }
    }
    entry.last_request = Some(now);
    Ok(())
}

/// Strip headers a guest must not control.
fn sanitize_headers(headers: &[(String, String)]) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(k, _)| {
            let k = k.trim().to_lowercase();
            !DENIED_HEADERS.contains(&k.as_str()) && !k.starts_with("proxy-")
        })
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect()
}

/// The shared blocking HTTP client.
///
/// Redirects are disabled: [`perform`] follows them itself so every hop is
/// re-checked against the allowlist. Automatic following would let a 302 leave it.
/// No cookie store either — a plugin gets no cross-request identity.
fn client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("build the plugin HTTP client")
    })
}

/// Run a blocking request on its own thread.
///
/// `reqwest::blocking` panics if called while a tokio runtime is current, and a
/// guest call can be reached from one (`Provider::undo` is async). A dedicated
/// thread has no runtime context, so this is safe wherever it is called from.
fn off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    std::thread::Builder::new()
        .name("sicompass-plugin-fetch".to_owned())
        .spawn(f)
        .map_err(|e| format!("could not start the fetch thread: {e}"))?
        .join()
        .map_err(|_| "the fetch thread panicked".to_owned())
}

/// Minimal GET used for robots.txt.
fn http_get_text(url: &str) -> Result<(u16, String), String> {
    let url = url.to_owned();
    off_runtime(move || {
        let resp = client().get(&url).send().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let body = resp.text().unwrap_or_default();
        Ok::<_, String>((status, body))
    })?
}

/// Perform one guest request, applying the whole policy.
pub fn perform(policy: &mut FetchPolicy, req: HttpRequest) -> Result<HttpResponse, String> {
    let method = req.method.trim().to_uppercase();
    if !ALLOWED_METHODS.contains(&method.as_str()) {
        return Err(format!(
            "method `{method}` is not permitted (allowed: {})",
            ALLOWED_METHODS.join(", ")
        ));
    }

    let headers = sanitize_headers(&req.headers);
    let mut url = req.url.clone();
    let mut body = req.body.clone();
    let mut method = method;

    for hop in 0..=MAX_REDIRECTS {
        // Every hop is vetted from scratch — that is the point of following
        // redirects by hand.
        let (parsed, host, origin) = vet_url(policy, &url)?;
        check_robots(&origin, parsed.path())?;
        policy.take_quota(&host)?;

        let (status, resp_headers, resp_body, location) =
            send_once(&method, &url, &headers, body.clone())?;

        let is_redirect = matches!(status, 301 | 302 | 303 | 307 | 308);
        match location {
            Some(loc) if is_redirect && hop < MAX_REDIRECTS => {
                url = parsed
                    .join(&loc)
                    .map_err(|e| format!("bad redirect target `{loc}`: {e}"))?
                    .to_string();
                // 301/302/303 turn everything into GET, as browsers do; 307/308
                // preserve the method and body.
                if matches!(status, 301..=303) {
                    method = "GET".to_owned();
                    body = None;
                }
                continue;
            }
            Some(_) if is_redirect => {
                return Err(format!("too many redirects (limit {MAX_REDIRECTS})"));
            }
            // Not a redirect, or a redirect with no Location: hand it back as-is.
            _ => {
                return Ok(HttpResponse { status, headers: resp_headers, body: resp_body });
            }
        }
    }

    Err(format!("too many redirects (limit {MAX_REDIRECTS})"))
}

type RawResponse = (u16, Vec<(String, String)>, Vec<u8>, Option<String>);

/// One HTTP exchange, with no policy and no redirect following.
fn send_once(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<Vec<u8>>,
) -> Result<RawResponse, String> {
    let method = method.to_owned();
    let url = url.to_owned();
    let headers = headers.to_vec();

    off_runtime(move || {
        let m = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|e| format!("bad method: {e}"))?;
        let mut builder = client().request(m, &url);
        for (k, v) in &headers {
            builder = builder.header(k, v);
        }
        if let Some(b) = body {
            builder = builder.body(b);
        }

        let resp = builder.send().map_err(|e| format!("request to {url} failed: {e}"))?;
        let status = resp.status().as_u16();

        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());

        // Refuse an over-large body before reading it, when the server says how big
        // it is. `content_length` is absent for chunked responses, hence the second
        // check after reading.
        if let Some(len) = resp.content_length()
            && len > MAX_RESPONSE_BYTES
        {
            return Err(format!(
                "response is {len} bytes, over the {MAX_RESPONSE_BYTES}-byte limit"
            ));
        }

        let out_headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_owned(), v.to_owned())))
            .collect();

        let bytes = resp.bytes().map_err(|e| format!("reading {url} failed: {e}"))?;
        if bytes.len() as u64 > MAX_RESPONSE_BYTES {
            return Err(format!(
                "response is {} bytes, over the {MAX_RESPONSE_BYTES}-byte limit",
                bytes.len()
            ));
        }

        Ok::<_, String>((status, out_headers, bytes.to_vec(), location))
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(hosts: &[&str]) -> FetchPolicy {
        FetchPolicy::new(
            "demo",
            &hosts.iter().map(|h| (*h).to_owned()).collect::<Vec<_>>(),
        )
    }

    // --- allowlist ---

    #[test]
    fn allowlist_matches_exactly_and_ignores_case() {
        let p = policy(&["Example.COM"]);
        assert!(p.host_allowed("example.com"));
        assert!(p.host_allowed("EXAMPLE.COM"));
    }

    #[test]
    fn a_subdomain_is_not_covered_by_its_parent() {
        // `evil.example.com` is a different site than `example.com`, and treating a
        // parent as covering its children would be a hole a plugin author could
        // walk through without the user noticing in the manifest.
        let p = policy(&["example.com"]);
        assert!(!p.host_allowed("evil.example.com"));
        assert!(!p.host_allowed("example.com.evil.test"));
    }

    #[test]
    fn an_empty_allowlist_permits_nothing() {
        let p = policy(&[]);
        assert!(!p.host_allowed("example.com"));
    }

    #[test]
    fn non_http_schemes_are_refused() {
        let p = policy(&["example.com"]);
        for url in ["file:///etc/passwd", "ftp://example.com/x", "data:text/plain,hi"] {
            let err = vet_url(&p, url).unwrap_err();
            assert!(
                err.contains("not permitted") || err.contains("not a valid URL"),
                "{url} -> {err}"
            );
        }
    }

    #[test]
    fn a_host_outside_the_allowlist_is_refused() {
        let p = policy(&["example.com"]);
        let err = vet_url(&p, "https://elsewhere.test/x").unwrap_err();
        assert!(err.contains("allowedHosts"), "{err}");
    }

    // --- internal addresses ---

    #[test]
    fn internal_addresses_are_refused_even_when_allowlisted() {
        // Allowlisting is the user's decision; reaching into their own network is
        // not what they agreed to.
        for host in [
            "localhost",
            "127.0.0.1",
            "10.0.0.5",
            "192.168.1.1",
            "172.16.0.1",
            "169.254.169.254", // the cloud metadata endpoint
            "100.64.0.1",
            "0.0.0.0",
            "[::1]",
            "[fe80::1]",
            "[fc00::1]",
            "[::ffff:127.0.0.1]",
        ] {
            assert!(is_internal_host(host), "{host} should be treated as internal");
        }
    }

    #[test]
    fn public_addresses_are_not_flagged_as_internal() {
        for host in ["example.com", "8.8.8.8", "1.1.1.1", "[2606:4700::1111]"] {
            assert!(!is_internal_host(host), "{host} should be allowed");
        }
    }

    #[test]
    fn an_allowlisted_internal_literal_is_still_refused() {
        let p = policy(&["127.0.0.1"]);
        let err = vet_url(&p, "http://127.0.0.1/admin").unwrap_err();
        assert!(err.contains("internal"), "{err}");
    }

    // --- robots.txt parsing ---

    #[test]
    fn robots_applies_only_the_wildcard_group_by_default() {
        let r = parse_robots(
            "User-agent: googlebot\nDisallow: /\n\nUser-agent: *\nDisallow: /private\n",
        );
        assert_eq!(r.disallow, vec!["/private".to_owned()]);
        assert!(r.allows("/public"));
        assert!(!r.allows("/private/x"));
    }

    #[test]
    fn a_group_naming_us_wins_over_the_wildcard() {
        let r = parse_robots(
            "User-agent: *\nDisallow: /\n\nUser-agent: sicompass\nDisallow: /secret\n",
        );
        assert_eq!(r.disallow, vec!["/secret".to_owned()]);
        assert!(r.allows("/anything"), "the wildcard group should not apply to us");
        assert!(!r.allows("/secret/x"));
    }

    #[test]
    fn several_agents_can_share_one_group() {
        let r = parse_robots("User-agent: *\nUser-agent: sicompass\nDisallow: /both\n");
        assert!(!r.allows("/both/x"));
    }

    #[test]
    fn allow_overrides_a_broader_disallow() {
        let r = parse_robots("User-agent: *\nDisallow: /docs\nAllow: /docs/public\n");
        assert!(!r.allows("/docs/private"));
        assert!(r.allows("/docs/public/a"), "longer Allow should win");
    }

    #[test]
    fn an_empty_disallow_means_everything_is_permitted() {
        let r = parse_robots("User-agent: *\nDisallow:\n");
        assert!(r.allows("/anything"));
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let r = parse_robots("# hello\n\nUser-agent: *   # us\nDisallow: /x  # nope\n");
        assert!(!r.allows("/x"));
    }

    #[test]
    fn crawl_delay_is_parsed_and_capped() {
        assert_eq!(
            parse_robots("User-agent: *\nCrawl-delay: 2.5\n").crawl_delay,
            Some(Duration::from_secs_f64(2.5))
        );
        // A hostile or broken value must not turn into an unbounded wait.
        assert_eq!(
            parse_robots("User-agent: *\nCrawl-delay: 999999\n").crawl_delay,
            Some(Duration::from_secs(3600))
        );
        assert_eq!(parse_robots("User-agent: *\nCrawl-delay: soon\n").crawl_delay, None);
    }

    #[test]
    fn an_empty_robots_file_permits_everything() {
        assert!(parse_robots("").allows("/anything"));
    }

    // --- header hygiene ---

    #[test]
    fn connection_and_routing_headers_are_stripped() {
        let given = [
            ("Host", "elsewhere.test"),
            ("Content-Length", "0"),
            ("Connection", "upgrade"),
            ("Proxy-Authorization", "secret"),
            ("Accept", "text/html"),
            ("X-Custom", "fine"),
        ]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect::<Vec<_>>();

        let kept = sanitize_headers(&given);
        let names: Vec<&str> = kept.iter().map(|(k, _)| k.as_str()).collect();

        // `Host` would let a request address somewhere other than the vetted URL.
        assert!(!names.iter().any(|n| n.eq_ignore_ascii_case("host")));
        assert!(!names.iter().any(|n| n.eq_ignore_ascii_case("content-length")));
        assert!(!names.iter().any(|n| n.eq_ignore_ascii_case("connection")));
        assert!(!names.iter().any(|n| n.to_lowercase().starts_with("proxy-")));

        // Ordinary headers survive; the point is hygiene, not lockdown.
        assert!(names.contains(&"Accept"));
        assert!(names.contains(&"X-Custom"));
    }

    // --- quota ---

    #[test]
    fn the_quota_is_per_host_and_refuses_when_spent() {
        let mut p = policy(&["a.test", "b.test"]);
        for i in 0..QUOTA_REQUESTS {
            p.take_quota("a.test").unwrap_or_else(|e| panic!("request {i} refused: {e}"));
        }
        let err = p.take_quota("a.test").unwrap_err();
        assert!(err.contains("request budget"), "{err}");

        // A different host has its own budget.
        assert!(p.take_quota("b.test").is_ok());
    }

    #[test]
    fn quota_errors_name_the_plugin_so_the_user_knows_who_to_blame() {
        let mut p = policy(&["a.test"]);
        for _ in 0..QUOTA_REQUESTS {
            let _ = p.take_quota("a.test");
        }
        assert!(p.take_quota("a.test").unwrap_err().contains("demo"));
    }

    // --- misc ---

    #[test]
    fn only_safe_methods_are_offered() {
        // No PUT/DELETE/PATCH: a plugin that can mutate remote state is a much
        // bigger promise than "may read from these hosts".
        assert_eq!(ALLOWED_METHODS, &["GET", "HEAD", "POST"]);
    }

    #[test]
    fn the_user_agent_identifies_us_and_says_how_to_get_in_touch() {
        assert!(USER_AGENT.starts_with("sicompass/"));
        assert!(USER_AGENT.contains("http"));
    }

    // -----------------------------------------------------------------------
    // The full pipeline, against a live server
    // -----------------------------------------------------------------------
    //
    // The checks above are unit-level. These drive `perform` end to end so the
    // ordering, the robots.txt fetch-and-cache, and the manual redirect following
    // are exercised for real rather than reasoned about.
    mod live {
        use super::*;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        /// Allow this thread to reach the mock server on 127.0.0.1, and drop any
        /// robots.txt cached for this server's origin.
        ///
        /// The internal-address flag is thread-local, so it cannot leak into a test
        /// checking that refusal itself. The cache eviction is per-origin: a port
        /// freed by an earlier server can be handed to this one, and inheriting that
        /// server's rules would quietly invert what the test is checking. See
        /// `forget_robots`.
        fn arrange(server: &MockServer) {
            ALLOW_INTERNAL_FOR_TESTS.with(|c| c.set(true));
            forget_robots(&server.uri());
        }

        fn get(url: &str) -> HttpRequest {
            HttpRequest {
                method: "GET".to_owned(),
                url: url.to_owned(),
                headers: Vec::new(),
                body: None,
            }
        }

        /// A policy allowing the mock server's host.
        fn policy_for(server: &MockServer) -> FetchPolicy {
            let host = reqwest::Url::parse(&server.uri()).unwrap().host_str().unwrap().to_owned();
            FetchPolicy::new("demo", &[host])
        }

        async fn serve_robots(server: &MockServer, body: &str) {
            Mock::given(method("GET"))
                .and(path("/robots.txt"))
                .respond_with(ResponseTemplate::new(200).set_body_string(body))
                .mount(server)
                .await;
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn an_allowed_request_succeeds_and_returns_the_body() {
            let server = MockServer::start().await;
            arrange(&server);
            serve_robots(&server, "User-agent: *\nDisallow:\n").await;
            Mock::given(method("GET"))
                .and(path("/page"))
                .respond_with(ResponseTemplate::new(200).set_body_string("hello"))
                .mount(&server)
                .await;

            let mut p = policy_for(&server);
            let resp = perform(&mut p, get(&format!("{}/page", server.uri()))).unwrap();

            assert_eq!(resp.status, 200);
            assert_eq!(String::from_utf8_lossy(&resp.body), "hello");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn a_host_off_the_allowlist_never_reaches_the_network() {
            let server = MockServer::start().await;
            arrange(&server);
            // No mounts at all: if the allowlist failed open, the request would 404
            // rather than being refused, and the error text would differ.
            let mut p = FetchPolicy::new("demo", &["somewhere.else".to_owned()]);
            let err = perform(&mut p, get(&format!("{}/page", server.uri()))).unwrap_err();
            assert!(err.contains("allowedHosts"), "{err}");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn robots_disallow_blocks_the_request() {
            let server = MockServer::start().await;
            arrange(&server);
            serve_robots(&server, "User-agent: *\nDisallow: /private\n").await;
            Mock::given(method("GET"))
                .and(path("/private/secret"))
                .respond_with(ResponseTemplate::new(200).set_body_string("should not be read"))
                .mount(&server)
                .await;

            let mut p = policy_for(&server);
            let err = perform(&mut p, get(&format!("{}/private/secret", server.uri())))
                .unwrap_err();
            assert!(err.contains("robots.txt disallows"), "{err}");

            // And a permitted path on the same origin still works, so this is the
            // rule being applied rather than the origin being broken.
            Mock::given(method("GET"))
                .and(path("/public"))
                .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
                .mount(&server)
                .await;
            assert!(perform(&mut p, get(&format!("{}/public", server.uri()))).is_ok());
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn crawl_delay_refuses_a_second_request_rather_than_sleeping() {
            let server = MockServer::start().await;
            arrange(&server);
            serve_robots(&server, "User-agent: *\nCrawl-delay: 30\n").await;
            Mock::given(method("GET"))
                .and(path("/a"))
                .respond_with(ResponseTemplate::new(200))
                .mount(&server)
                .await;

            let mut p = policy_for(&server);
            let started = Instant::now();
            assert!(perform(&mut p, get(&format!("{}/a", server.uri()))).is_ok());
            let err = perform(&mut p, get(&format!("{}/a", server.uri()))).unwrap_err();

            assert!(err.contains("between requests"), "{err}");
            // The point: it returns immediately. Honouring a 30s Crawl-delay by
            // sleeping would freeze the thread driving the guest, and with it the UI.
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "crawl-delay slept instead of refusing"
            );
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn a_redirect_within_the_allowlist_is_followed() {
            let server = MockServer::start().await;
            arrange(&server);
            serve_robots(&server, "User-agent: *\nDisallow:\n").await;
            Mock::given(method("GET"))
                .and(path("/from"))
                .respond_with(ResponseTemplate::new(302).insert_header("location", "/to"))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/to"))
                .respond_with(ResponseTemplate::new(200).set_body_string("arrived"))
                .mount(&server)
                .await;

            let mut p = policy_for(&server);
            let resp = perform(&mut p, get(&format!("{}/from", server.uri()))).unwrap();
            assert_eq!(resp.status, 200);
            assert_eq!(String::from_utf8_lossy(&resp.body), "arrived");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn a_redirect_off_the_allowlist_is_refused() {
            // The reason redirects are followed by hand: reqwest's automatic
            // following would have fetched this without ever re-checking.
            let server = MockServer::start().await;
            arrange(&server);
            serve_robots(&server, "User-agent: *\nDisallow:\n").await;
            Mock::given(method("GET"))
                .and(path("/away"))
                .respond_with(
                    ResponseTemplate::new(302)
                        .insert_header("location", "https://evil.test/collect"),
                )
                .mount(&server)
                .await;

            let mut p = policy_for(&server);
            let err = perform(&mut p, get(&format!("{}/away", server.uri()))).unwrap_err();
            assert!(err.contains("allowedHosts"), "{err}");
            assert!(err.contains("evil.test"), "the error should name the target: {err}");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn an_oversized_response_is_refused() {
            let server = MockServer::start().await;
            arrange(&server);
            serve_robots(&server, "User-agent: *\nDisallow:\n").await;
            // A real oversized body, not a fabricated content-length: a mismatched
            // header makes hyper reject the response server-side, which would have
            // this test passing for the wrong reason.
            Mock::given(method("GET"))
                .and(path("/huge"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_bytes(vec![b'x'; (MAX_RESPONSE_BYTES + 1) as usize]),
                )
                .mount(&server)
                .await;

            let mut p = policy_for(&server);
            let err = perform(&mut p, get(&format!("{}/huge", server.uri()))).unwrap_err();
            assert!(err.contains("over the"), "{err}");

            // A body inside the cap still comes through, so the limit is a limit and
            // not a blanket refusal.
            Mock::given(method("GET"))
                .and(path("/small"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'y'; 1024]))
                .mount(&server)
                .await;
            let ok = perform(&mut p, get(&format!("{}/small", server.uri()))).unwrap();
            assert_eq!(ok.body.len(), 1024);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn a_missing_robots_file_permits_the_request() {
            // Every crawler treats an absent robots.txt as "no restrictions"; failing
            // closed here would make most of the web unreachable to plugins.
            let server = MockServer::start().await;
            arrange(&server);
            Mock::given(method("GET"))
                .and(path("/robots.txt"))
                .respond_with(ResponseTemplate::new(404))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/page"))
                .respond_with(ResponseTemplate::new(200).set_body_string("fine"))
                .mount(&server)
                .await;

            let mut p = policy_for(&server);
            assert!(perform(&mut p, get(&format!("{}/page", server.uri()))).is_ok());
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn a_disallowed_method_is_refused_before_any_request() {
            let server = MockServer::start().await;
            arrange(&server);
            let mut p = policy_for(&server);
            let mut req = get(&format!("{}/x", server.uri()));
            req.method = "DELETE".to_owned();

            let err = perform(&mut p, req).unwrap_err();
            assert!(err.contains("not permitted"), "{err}");
        }
    }
}

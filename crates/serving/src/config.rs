use crate::{Result, ServingError as E, authority::Leaf, site::Site};
use std::path::{Path, PathBuf};
pub(crate) struct Route {
    pub site: Site,
    pub public: PathBuf,
    pub php: u16,
    pub vite: Option<u16>,
    pub leaf: Option<Leaf>,
}
pub(crate) fn quoted(path: &Path) -> Result<String> {
    let path = dunce::simplified(path)
        .to_str()
        .ok_or(E::InvalidConfiguration)?
        .replace('\\', "/");
    if path.chars().any(char::is_control) {
        return Err(E::InvalidConfiguration);
    }
    Ok(serde_json::to_string(&path).unwrap())
}
pub(crate) fn caddy(
    routes: &[Route],
    http: u16,
    https: u16,
    nonce: &str,
    data: &Path,
) -> Result<String> {
    let mut text = format!(
        "{{\n admin off\n auto_https off\n skip_install_trust\n persist_config off\n storage file_system {{\n  root {}\n }}\n}}\n",
        quoted(data)?
    );
    for route in routes {
        for secure in [false, true] {
            if secure && !route.site.https {
                continue;
            }
            let scheme = if secure { "https" } else { "http" };
            let port = if secure { https } else { http };
            text.push_str(&format!("{scheme}://{}:{port}, {scheme}://{}:{port} {{\n bind 127.0.0.1\n root * {}\n @health path /__devforge_health_{nonce}\n respond @health \"{nonce}\" 200\n @hidden path_regexp hidden \"(?i)(^|/)\\\\.\"\n respond @hidden 404\n request_body {{\n  max_size 64MB\n }}\n",route.site.domain,route.site.alias(),quoted(&route.public)?));
            if secure {
                let leaf = route.leaf.as_ref().ok_or(E::AuthorityInvalid)?;
                text.push_str(&format!(
                    " tls {} {}\n",
                    quoted(&leaf.certificate)?,
                    quoted(&leaf.key)?
                ));
            }
            text.push_str(&format!(
                " php_fastcgi 127.0.0.1:{}\n file_server\n}}\n",
                route.php
            ));
            if let Some(vite) = route.vite {
                text.push_str(&format!(
                    "{scheme}://{}:{port}, {scheme}://{}:{port} {{\n bind 127.0.0.1\n",
                    route.site.vite_domain(),
                    route.site.vite_alias()
                ));
                if secure {
                    let leaf = route.leaf.as_ref().ok_or(E::AuthorityInvalid)?;
                    text.push_str(&format!(
                        " tls {} {}\n",
                        quoted(&leaf.certificate)?,
                        quoted(&leaf.key)?
                    ));
                }
                text.push_str(&format!(" reverse_proxy 127.0.0.1:{vite}\n}}\n"));
            }
        }
    }
    Ok(text)
}
pub(crate) fn fpm(port: u16) -> String {
    format!(
        "[global]\ndaemonize=no\nerror_log=/dev/stderr\n[devforge]\nlisten=127.0.0.1:{port}\nlisten.allowed_clients=127.0.0.1\npm=ondemand\npm.max_children=4\npm.process_idle_timeout=10s\nclear_env=no\ncatch_workers_output=yes\ndecorate_workers_output=no\nsecurity.limit_extensions=.php\n"
    )
}

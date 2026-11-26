use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::{LoopbackAddress, Platform};

/// The files `fun local setup` writes as root, rendered from constants and root-owned tool
/// paths only.
///
/// On Linux a oneshot unit gives a dummy link `~test` as its only routing domain; it is
/// `PartOf` systemd-resolved and reruns on its restart, so every step is idempotent and
/// nothing deletes the link. On macOS `/etc/resolver/test` and a LaunchDaemon do the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineFiles {
    files: Vec<(PathBuf, String)>,
}

/// Where `setup` records which user owns this machine's local origins.
pub const OWNER_FILE: &str = "/etc/fundamental/local.toml";
/// The unit that routes `.test` on Linux.
pub const UNIT_NAME: &str = "fundamental-local-dns.service";
/// The NetworkManager drop-in that leaves the routing link alone.
pub const NETWORK_MANAGER_DROP_IN: &str = "/etc/NetworkManager/conf.d/90-fundamental-local.conf";
/// The macOS resolver file for `.test`.
pub const MACOS_RESOLVER: &str = "/etc/resolver/test";
/// The macOS LaunchDaemon that aliases the loopback pool.
pub const MACOS_DAEMON: &str = "/Library/LaunchDaemons/dev.fundamental.local-loopback.plist";

/// The absolute tools a Linux unit runs, resolved to root-owned paths by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxTools {
    /// `ip`.
    pub ip: PathBuf,
    /// `resolvectl`.
    pub resolvectl: PathBuf,
}

impl MachineFiles {
    /// What Linux needs: the owner file, the unit, and when NetworkManager runs its drop-in.
    #[must_use]
    pub fn linux(owner: u32, state: &Path, tools: &LinuxTools, network_manager: bool) -> Self {
        let link = Platform::ROUTING_LINK;
        let address = Platform::ROUTING_ADDRESS;
        let ip = tools.ip.display();
        let resolvectl = tools.resolvectl.display();
        let unit = format!(
            "# Written by `fun local setup`: routes `.test`, and nothing else, to the local resolver.\n\
             [Unit]\n\
             Description=Route .test to the fundamental local resolver\n\
             After=systemd-resolved.service\n\
             PartOf=systemd-resolved.service\n\
             Before=docker.service\n\
             \n\
             [Service]\n\
             Type=oneshot\n\
             RemainAfterExit=yes\n\
             ExecStartPre=-{ip} link add {link} type dummy\n\
             ExecStart={ip} address replace {address}/32 dev {link}\n\
             ExecStart={ip} link set {link} up\n\
             ExecStart={resolvectl} dns {link} {address}\n\
             ExecStart={resolvectl} domain {link} ~test\n\
             ExecStart={resolvectl} default-route {link} false\n\
             \n\
             [Install]\n\
             WantedBy=multi-user.target systemd-resolved.service\n"
        );
        let mut files = vec![
            (PathBuf::from(OWNER_FILE), owner_file(owner, state)),
            (Path::new("/etc/systemd/system").join(UNIT_NAME), unit),
        ];
        if network_manager {
            files.push((
                PathBuf::from(NETWORK_MANAGER_DROP_IN),
                format!(
                    "# Written by `fun local setup`: the routing link is the local resolver's.\n[keyfile]\nunmanaged-devices=interface-name:{link}\n"
                ),
            ));
        }
        Self { files }
    }

    /// What macOS needs: the owner file, the resolver file, and the LaunchDaemon.
    #[must_use]
    pub fn macos(owner: u32, state: &Path) -> Self {
        let mut aliases = String::new();
        for address in LoopbackAddress::pool() {
            let _ = write!(aliases, "/sbin/ifconfig lo0 alias {address} up; ");
        }
        let daemon = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n<dict>\n\
             \x20 <key>Label</key><string>dev.fundamental.local-loopback</string>\n\
             \x20 <key>ProgramArguments</key>\n\
             \x20 <array><string>/bin/sh</string><string>-c</string><string>{}</string></array>\n\
             \x20 <key>RunAtLoad</key><true/>\n\
             </dict>\n</plist>\n",
            aliases.trim_end()
        );
        Self {
            files: vec![
                (PathBuf::from(OWNER_FILE), owner_file(owner, state)),
                (
                    PathBuf::from(MACOS_RESOLVER),
                    "# Written by `fun local setup`.\nnameserver 127.0.0.1\nport 53\n".to_owned(),
                ),
                (PathBuf::from(MACOS_DAEMON), daemon),
            ],
        }
    }

    /// Every file, with its contents.
    #[must_use]
    pub fn files(&self) -> &[(PathBuf, String)] {
        &self.files
    }
}

fn owner_file(owner: u32, state: &Path) -> String {
    let state = toml::Value::String(state.display().to_string());
    format!(
        "# Written by `fun local setup`: the one user whose local origins this machine runs, and\n\
         # the registry the machine's resolver serves.\nowner = {owner}\nstate = {state}\n"
    )
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{LinuxTools, MachineFiles};

    #[test]
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "readability baseline"
    )]
    fn setup_routes_only_test() {
        let tools = LinuxTools {
            ip: PathBuf::from("/usr/sbin/ip"),
            resolvectl: PathBuf::from("/usr/bin/resolvectl"),
        };
        let linux = MachineFiles::linux(
            1001,
            Path::new("/home/dev/.local/state/fundamental/local"),
            &tools,
            true,
        );
        let unit = &linux.files()[1].1;
        assert!(
            unit.contains("resolvectl dns fundamental-dns 192.0.2.53"),
            "{unit}"
        );
        assert!(
            unit.contains("resolvectl domain fundamental-dns ~test\n"),
            "{unit}"
        );
        assert!(
            unit.contains("default-route fundamental-dns false"),
            "{unit}"
        );
        assert!(!unit.contains("DNS="), "never a global DNS server: {unit}");
        assert!(!unit.contains("~.\n"), "never every domain: {unit}");
        assert!(unit.contains("PartOf=systemd-resolved.service"), "{unit}");
        assert!(unit.contains("Before=docker.service"), "{unit}");
        assert!(
            !unit.contains("link delete"),
            "a restart must not take the resolver's address: {unit}"
        );
        assert!(unit.contains("address replace 192.0.2.53/32"), "{unit}");
        assert!(
            linux.files()[2]
                .1
                .contains("unmanaged-devices=interface-name:fundamental-dns")
        );
        assert_eq!(
            MachineFiles::linux(
                1001,
                Path::new("/home/dev/.local/state/fundamental/local"),
                &tools,
                false
            )
            .files()
            .len(),
            2
        );
        assert!(linux.files()[0].1.contains("owner = 1001"));
        assert!(
            linux.files()[0]
                .1
                .contains("state = \"/home/dev/.local/state/fundamental/local\""),
            "{}",
            linux.files()[0].1
        );

        let macos =
            MachineFiles::macos(501, Path::new("/Users/dev/.local/state/fundamental/local"));
        let resolver = &macos.files()[1];
        assert_eq!(resolver.0, PathBuf::from("/etc/resolver/test"));
        assert!(
            resolver.1.contains("nameserver 127.0.0.1\nport 53"),
            "{}",
            resolver.1
        );
        let daemon = &macos.files()[2].1;
        assert!(daemon.contains("lo0 alias 127.0.0.2 up"), "{daemon}");
        assert!(daemon.contains("lo0 alias 127.0.0.254 up"), "{daemon}");
        assert!(!daemon.contains("127.0.0.1 up"), "{daemon}");
    }
}

//! Handing the addresses a document links to over to the system.
//!
//! A document is not to be trusted with what it has opened: an address may
//! name a program, a file on this machine or one on a share as easily as a
//! web page. Only web pages and mail addresses get through, and only in a
//! form that cannot be read as anything more.

/// Explicit settings action; never reachable through a PDF hyperlink.
#[cfg(windows)]
pub fn default_pdf() -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    let exe = std::env::current_exe()?;
    let command = format!("\"{}\" \"%1\"", exe.display());
    let icon = format!("\"{}\",0", exe.display());
    let entries = [
        ("Software\\Classes\\FolioFlow.PDF", "", "Folio Flow PDF"),
        (
            "Software\\Classes\\Applications\\Folio Flow.exe",
            "FriendlyAppName",
            "Folio Flow",
        ),
        (
            "Software\\Classes\\Applications\\Folio Flow.exe\\shell\\open\\command",
            "",
            command.as_str(),
        ),
        (
            "Software\\Classes\\Applications\\Folio Flow.exe\\SupportedTypes",
            ".pdf",
            "",
        ),
        (
            "Software\\Classes\\FolioFlow.PDF\\DefaultIcon",
            "",
            icon.as_str(),
        ),
        (
            "Software\\Classes\\FolioFlow.PDF\\shell\\open\\command",
            "",
            command.as_str(),
        ),
        (
            "Software\\Folio Flow\\Capabilities",
            "ApplicationName",
            "Folio Flow",
        ),
        (
            "Software\\Folio Flow\\Capabilities",
            "ApplicationDescription",
            "PDF reader and page editor",
        ),
        (
            "Software\\Folio Flow\\Capabilities\\FileAssociations",
            ".pdf",
            "FolioFlow.PDF",
        ),
        (
            "Software\\RegisteredApplications",
            "Folio Flow",
            "Software\\Folio Flow\\Capabilities",
        ),
    ];
    for (key, name, value) in entries {
        let mut cmd = Command::new("reg.exe");
        cmd.creation_flags(0x08000000)
            .args(["add", &format!("HKCU\\{key}")]);
        if name.is_empty() {
            cmd.arg("/ve");
        } else {
            cmd.args(["/v", name]);
        }
        let output = cmd.args(["/t", "REG_SZ", "/d", value, "/f"]).output()?;
        if !output.status.success() {
            return Err(std::io::Error::other(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
    }
    // Leave the protected .pdf UserChoice to Windows and the user.
    launch("ms-settings:defaultapps?registeredAppUser=Folio%20Flow")
}

#[cfg(not(windows))]
pub fn default_pdf() -> std::io::Result<()> {
    Err(std::io::Error::other("此入口仅支持 Windows"))
}

/// An address that is safe to open.
#[derive(Debug, PartialEq)]
pub struct Address(String);

impl Address {
    /// `None` unless the address is a web page or a mail address.
    pub fn parse(uri: &str) -> Option<Self> {
        let uri = uri.trim();
        let (scheme, rest) = uri.split_once(':')?;
        let scheme = scheme.to_ascii_lowercase();

        let named = match scheme.as_str() {
            "http" | "https" => rest.strip_prefix("//").and_then(|rest| rest.chars().next()),
            "mailto" => rest.chars().next(),
            _ => None,
        };

        if !named.is_some_and(|first| !matches!(first, '/' | '?' | '#')) {
            return None;
        }

        let mut address = scheme;

        address.push(':');

        // Whatever could end the address early where it is passed on, or be
        // read there as something else, goes in escaped.
        for byte in rest.bytes() {
            if byte.is_ascii_graphic() && !b"\"<>\\^`{|}".contains(&byte) {
                address.push(byte as char);
            } else {
                address.push_str(&format!("%{byte:02X}"));
            }
        }

        Some(Self(address))
    }

    /// The part of the address that says where it leads: the site, without
    /// any name it is signed in under, or whom the mail is to.
    pub fn place(&self) -> &str {
        match self.0.strip_prefix("mailto:") {
            Some(to) => to.split('?').next().unwrap_or(to),
            None => {
                let site = self.0.split_once("//").map_or("", |(_, rest)| rest);
                let site = site.split(['/', '?', '#']).next().unwrap_or(site);

                site.rsplit('@').next().unwrap_or(site)
            }
        }
    }

    /// Opens the address in the program the system has for it. Finding and
    /// starting that program can take a moment, so it happens on a thread of
    /// its own; if it cannot be done, nothing happens.
    pub fn open(self) {
        let _ = std::thread::Builder::new()
            .name("open-link".into())
            .spawn(move || launch(&self.0));
    }
}

#[cfg(windows)]
fn launch(address: &str) -> std::io::Result<()> {
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;

    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let file: Vec<u16> = address.encode_utf16().chain([0]).collect();

    unsafe {
        // The shell may hand the address to a COM object.
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);

        let result = ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );

        if com.is_ok() {
            CoUninitialize();
        }
        let code = result.0 as isize;
        if code > 32 {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "Windows shell launch failed ({code})"
            )))
        }
    }
}

#[cfg(not(windows))]
fn launch(address: &str) -> std::io::Result<()> {
    use std::process::{Command, Stdio};

    let status = Command::new("xdg-open")
        .arg(address)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("Could not open system handler"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_launch_probe_when_requested() {
        if std::env::var_os("FOLIO_SETTINGS_LAUNCH_PROBE").is_some() {
            launch("ms-settings:defaultapps?registeredAppUser=Folio%20Flow").unwrap();
        }
    }

    fn opened(uri: &str) -> Option<String> {
        Address::parse(uri).map(|address| address.0)
    }

    #[test]
    fn opens_web_pages_and_mail_only() {
        assert_eq!(
            opened(" HTTPS://example.com/a?b=c#d "),
            Some("https://example.com/a?b=c#d".into())
        );
        assert_eq!(
            opened("http://example.com"),
            Some("http://example.com".into())
        );
        assert_eq!(
            opened("mailto:someone@example.com?subject=Hi"),
            Some("mailto:someone@example.com?subject=Hi".into())
        );

        for uri in [
            "file:///C:/Windows/System32/calc.exe",
            "C:\\Windows\\System32\\calc.exe",
            "\\\\server\\share\\run.bat",
            "javascript:alert(1)",
            "ms-settings:display",
            "search-ms:query=x",
            "example.com",
            "https:example.com",
            "https://",
            "https:///etc/passwd",
            "mailto:",
            "",
        ] {
            assert_eq!(opened(uri), None, "{uri:?}");
        }
    }

    #[test]
    fn escapes_what_could_be_read_as_more_than_an_address() {
        assert_eq!(
            opened("https://example.com/a b\" --flag\u{e9}\n"),
            Some("https://example.com/a%20b%22%20--flag%C3%A9".into())
        );
    }

    #[test]
    fn names_the_place_an_address_leads_to() {
        let place = |uri: &str| Address::parse(uri).map(|address| address.place().to_owned());

        assert_eq!(
            place("https://example.com/a/b?c"),
            Some("example.com".into())
        );
        // Not the site it is made to look like.
        assert_eq!(
            place("https://example.com@elsewhere.test/"),
            Some("elsewhere.test".into())
        );
        assert_eq!(
            place("mailto:someone@example.com?subject=Hi"),
            Some("someone@example.com".into())
        );
    }
}

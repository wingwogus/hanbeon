//! Linux focused-application discovery through X11, with a safe Wayland fallback.

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionBackend {
    WaylandUnsupported,
    X11,
    Unavailable,
}

#[cfg(any(target_os = "linux", test))]
fn session_backend(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> SessionBackend {
    if session_type.is_some_and(|value| value.eq_ignore_ascii_case("wayland"))
        || wayland_display.is_some_and(|value| !value.is_empty())
    {
        SessionBackend::WaylandUnsupported
    } else if display.is_some_and(|value| !value.is_empty()) {
        SessionBackend::X11
    } else {
        SessionBackend::Unavailable
    }
}

#[cfg(any(target_os = "linux", test))]
fn parse_wm_class(value: &[u8]) -> Vec<String> {
    let Some(value) = value.strip_suffix(b"\0") else {
        return Vec::new();
    };
    let mut fields = value.split(|byte| *byte == b'\0');
    let Some(instance) = fields.next().filter(|field| !field.is_empty()) else {
        return Vec::new();
    };
    let Some(class) = fields.next().filter(|field| !field.is_empty()) else {
        return Vec::new();
    };
    if fields.next().is_some() {
        return Vec::new();
    }

    let (Ok(instance), Ok(class)) = (
        String::from_utf8(instance.to_vec()),
        String::from_utf8(class.to_vec()),
    ) else {
        return Vec::new();
    };
    vec![instance, class]
}

#[cfg(any(target_os = "linux", test))]
fn parse_text(value: &[u8]) -> Option<String> {
    let value = value
        .iter()
        .rposition(|byte| *byte != b'\0')
        .map_or(&[][..], |last_non_nul| &value[..=last_non_nul]);
    (!value.is_empty())
        .then(|| String::from_utf8(value.to_vec()).ok())
        .flatten()
}

#[cfg(any(target_os = "linux", test))]
fn parse_desktop_id(value: &[u8]) -> Option<String> {
    let desktop_id = parse_text(value)?;
    if desktop_id.ends_with(".desktop") && desktop_id.contains('/') {
        desktop_id
            .rsplit('/')
            .next()
            .filter(|filename| !filename.is_empty())
            .map(ToOwned::to_owned)
    } else {
        Some(desktop_id)
    }
}

#[cfg(any(target_os = "linux", test))]
fn optional_atom(atom: u32) -> Option<u32> {
    (atom != 0).then_some(atom)
}

#[cfg(target_os = "linux")]
use std::env;

#[cfg(target_os = "linux")]
use x11rb::{
    connection::Connection,
    protocol::xproto::{Atom, AtomEnum, ConnectionExt, Window},
    rust_connection::RustConnection,
};

#[cfg(target_os = "linux")]
use super::{FocusedApplication, FocusedApplicationSource};

#[cfg(target_os = "linux")]
const MAX_PROPERTY_LONGS: u32 = 256;

#[cfg(target_os = "linux")]
pub struct LinuxSource {
    backend: Backend,
}

#[cfg(target_os = "linux")]
enum Backend {
    X11(Box<X11Backend>),
    WaylandUnsupported,
    Unavailable,
}

#[cfg(target_os = "linux")]
struct X11Backend {
    connection: RustConnection,
    root: Window,
    atoms: Atoms,
}

#[cfg(target_os = "linux")]
struct Atoms {
    active_window: Option<Atom>,
    wm_pid: Option<Atom>,
    gtk_application_id: Option<Atom>,
    kde_desktop_file: Option<Atom>,
    bamf_desktop_file: Option<Atom>,
    utf8_string: Option<Atom>,
}

#[cfg(target_os = "linux")]
impl Atoms {
    fn resolve(connection: &RustConnection) -> Self {
        Self {
            active_window: intern_atom(connection, b"_NET_ACTIVE_WINDOW"),
            wm_pid: intern_atom(connection, b"_NET_WM_PID"),
            gtk_application_id: intern_atom(connection, b"_GTK_APPLICATION_ID"),
            kde_desktop_file: intern_atom(connection, b"_KDE_NET_WM_DESKTOP_FILE"),
            bamf_desktop_file: intern_atom(connection, b"_BAMF_DESKTOP_FILE"),
            utf8_string: intern_atom(connection, b"UTF8_STRING"),
        }
    }

    fn refresh(&mut self, connection: &RustConnection) {
        self.active_window = self
            .active_window
            .or_else(|| intern_atom(connection, b"_NET_ACTIVE_WINDOW"));
        self.wm_pid = self
            .wm_pid
            .or_else(|| intern_atom(connection, b"_NET_WM_PID"));
        self.gtk_application_id = self
            .gtk_application_id
            .or_else(|| intern_atom(connection, b"_GTK_APPLICATION_ID"));
        self.kde_desktop_file = self
            .kde_desktop_file
            .or_else(|| intern_atom(connection, b"_KDE_NET_WM_DESKTOP_FILE"));
        self.bamf_desktop_file = self
            .bamf_desktop_file
            .or_else(|| intern_atom(connection, b"_BAMF_DESKTOP_FILE"));
        self.utf8_string = self
            .utf8_string
            .or_else(|| intern_atom(connection, b"UTF8_STRING"));
    }
}

#[cfg(target_os = "linux")]
impl LinuxSource {
    pub fn new() -> Self {
        let session_type = env::var("XDG_SESSION_TYPE").ok();
        let wayland_display = env::var("WAYLAND_DISPLAY").ok();
        let display = env::var("DISPLAY").ok();

        let backend = match session_backend(
            session_type.as_deref(),
            wayland_display.as_deref(),
            display.as_deref(),
        ) {
            SessionBackend::WaylandUnsupported => Backend::WaylandUnsupported,
            SessionBackend::X11 => X11Backend::connect(display.as_deref())
                .map(|source| Backend::X11(Box::new(source)))
                .unwrap_or(Backend::Unavailable),
            SessionBackend::Unavailable => Backend::Unavailable,
        };

        Self { backend }
    }
}

#[cfg(target_os = "linux")]
impl FocusedApplicationSource for LinuxSource {
    fn current(&mut self) -> Option<FocusedApplication> {
        match &mut self.backend {
            Backend::X11(source) => source.current(),
            Backend::WaylandUnsupported | Backend::Unavailable => None,
        }
    }
}

#[cfg(target_os = "linux")]
impl X11Backend {
    fn connect(display: Option<&str>) -> Option<Self> {
        let (connection, screen) = RustConnection::connect(display).ok()?;
        let root = connection.setup().roots.get(screen)?.root;
        let atoms = Atoms::resolve(&connection);

        Some(Self {
            connection,
            root,
            atoms,
        })
    }

    fn current(&mut self) -> Option<FocusedApplication> {
        self.atoms.refresh(&self.connection);
        let window = self.active_window()?;
        let pid = self.window_pid(window);
        let wm_classes = self.wm_classes(window);
        let desktop_id = self.desktop_id(window);

        if desktop_id.is_none() && wm_classes.is_empty() {
            return None;
        }

        Some(FocusedApplication::linux(pid, desktop_id, wm_classes))
    }

    fn active_window(&self) -> Option<Window> {
        let reply = self.property(
            self.root,
            self.atoms.active_window?,
            AtomEnum::WINDOW.into(),
        )?;
        (reply.type_ == u32::from(AtomEnum::WINDOW)
            && reply.format == 32
            && reply.bytes_after == 0)
            .then_some(())?;
        let mut windows = reply.value32()?;
        let window = windows.next()?;
        (windows.next().is_none() && window != 0).then_some(window)
    }

    fn window_pid(&self, window: Window) -> Option<i32> {
        let reply = self.property(window, self.atoms.wm_pid?, AtomEnum::CARDINAL.into())?;
        (reply.type_ == u32::from(AtomEnum::CARDINAL)
            && reply.format == 32
            && reply.bytes_after == 0)
            .then_some(())?;
        let mut pids = reply.value32()?;
        let pid = pids.next()?;
        (pids.next().is_none()).then_some(())?;
        i32::try_from(pid).ok().filter(|pid| *pid > 0)
    }

    fn wm_classes(&self, window: Window) -> Vec<String> {
        let Some(reply) = self.property(window, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into())
        else {
            return Vec::new();
        };
        if reply.type_ != u32::from(AtomEnum::STRING) || reply.format != 8 || reply.bytes_after != 0
        {
            return Vec::new();
        }

        reply
            .value8()
            .map(|value| parse_wm_class(&value.collect::<Vec<_>>()))
            .unwrap_or_default()
    }

    fn desktop_id(&self, window: Window) -> Option<String> {
        let gtk_desktop_id = self
            .text_property(window, self.atoms.gtk_application_id)
            .and_then(|value| parse_text(&value));
        let kde_desktop_id = self
            .text_property(window, self.atoms.kde_desktop_file)
            .and_then(|value| parse_text(&value));
        let bamf_desktop_id = self
            .text_property(window, self.atoms.bamf_desktop_file)
            .and_then(|value| parse_desktop_id(&value));

        gtk_desktop_id.or(kde_desktop_id).or(bamf_desktop_id)
    }

    fn text_property(&self, window: Window, property: Option<Atom>) -> Option<Vec<u8>> {
        let property = property?;
        let reply = self.property(window, property, AtomEnum::ANY.into())?;
        ((self.atoms.utf8_string == Some(reply.type_)
            || reply.type_ == u32::from(AtomEnum::STRING))
            && reply.format == 8
            && reply.bytes_after == 0)
            .then_some(())?;
        reply.value8().map(|value| value.collect())
    }

    fn property(
        &self,
        window: Window,
        property: Atom,
        property_type: Atom,
    ) -> Option<x11rb::protocol::xproto::GetPropertyReply> {
        self.connection
            .get_property(
                false,
                window,
                property,
                property_type,
                0,
                MAX_PROPERTY_LONGS,
            )
            .ok()?
            .reply()
            .ok()
    }
}

#[cfg(target_os = "linux")]
fn intern_atom(connection: &RustConnection, name: &[u8]) -> Option<Atom> {
    connection
        .intern_atom(true, name)
        .ok()?
        .reply()
        .ok()
        .and_then(|reply| optional_atom(reply.atom))
}

#[cfg(test)]
mod tests {
    use super::{SessionBackend, optional_atom, parse_desktop_id, parse_wm_class, session_backend};

    #[test]
    fn parses_both_wm_class_strings() {
        assert_eq!(
            parse_wm_class(b"spotify\0Spotify\0"),
            vec!["spotify".to_string(), "Spotify".to_string()]
        );
    }

    #[test]
    fn rejects_a_wm_class_without_a_final_nul_terminator() {
        assert_eq!(parse_wm_class(b"spotify\0Spotify"), Vec::<String>::new());
    }

    #[test]
    fn rejects_a_wm_class_with_more_than_two_fields() {
        assert_eq!(
            parse_wm_class(b"spotify\0Spotify\0Extra\0"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn rejects_a_wm_class_with_an_empty_field() {
        assert_eq!(parse_wm_class(b"spotify\0\0"), Vec::<String>::new());
    }

    #[test]
    fn rejects_a_wm_class_with_invalid_utf8() {
        assert_eq!(parse_wm_class(b"spotify\0\xff\0"), Vec::<String>::new());
    }

    #[test]
    fn wayland_wins_over_an_xwayland_display() {
        assert_eq!(
            session_backend(Some("wayland"), Some("wayland-0"), Some(":0")),
            SessionBackend::WaylandUnsupported
        );
    }

    #[test]
    fn bamf_path_becomes_a_desktop_id() {
        assert_eq!(
            parse_desktop_id(b"/usr/share/applications/spotify.desktop\0"),
            Some("spotify.desktop".to_string())
        );
    }

    #[test]
    fn absent_optional_atom_can_be_refreshed_when_it_later_exists() {
        let cached_atom = optional_atom(0);
        assert_eq!(cached_atom, None);

        let refreshed_atom = cached_atom.or_else(|| optional_atom(321));
        assert_eq!(refreshed_atom, Some(321));
    }
}

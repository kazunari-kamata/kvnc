use aes::Aes128;
use anyhow::{Context, Result, bail};
use arboard::{Clipboard, Error as ClipboardError};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use clap::{Parser, ValueEnum};
#[cfg(target_os = "macos")]
use core_foundation::{
    base::{CFType, TCFType},
    boolean::CFBoolean,
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};
#[cfg(target_os = "macos")]
use core_graphics::{
    event::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField},
    event_source::{CGEventSource, CGEventSourceStateID},
    geometry::CGPoint,
};
#[cfg(target_os = "macos")]
use core_graphics2::display_stream::{
    CGDisplayStream, CGDisplayStreamFrameStatus, CGDisplayStreamProperties,
};
#[cfg(unix)]
use daemonize::Daemonize;
#[cfg(target_os = "macos")]
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use eax::{
    Eax,
    aead::{Aead, KeyInit, Payload, generic_array::GenericArray},
};
#[cfg(not(target_os = "macos"))]
use enigo::Coordinate;
use enigo::{Axis, Button, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use image::{Rgba, RgbaImage, imageops::FilterType};
#[cfg(target_os = "macos")]
use objc2_io_surface::{IOSurfaceLockOptions, IOSurfaceRef};
use rand::{RngCore, rngs::OsRng};
use rsa::{Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey, traits::PublicKeyParts};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::fs::{self, File, OpenOptions, read_to_string};
use std::io::{Read, Write};
use std::net::SocketAddr;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt as UnixMetadataExt;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt as WindowsMetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(target_os = "macos")]
use std::{collections::BTreeMap, ptr};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};
use tokio::time::{MissedTickBehavior, interval};
use xcap::Monitor;

const MAX_CLIPBOARD_BYTES: usize = 1024 * 1024;
const MAX_EXTENSION_BYTES: usize = 1024 * 1024;
const RFB_VERSION: &[u8; 12] = b"RFB 003.008\n";
const RFB_ENCODING_RAW: i32 = 0;
const RFB_ENCODING_HEXTILE: i32 = 5;
const RFB_ENCODING_EXTENDED_CLIPBOARD: i32 = -1_063_131_698;
const EXTENDED_CLIPBOARD_TEXT: u32 = 1;
const EXTENDED_CLIPBOARD_CAPS: u32 = 1 << 24;
const EXTENDED_CLIPBOARD_REQUEST: u32 = 1 << 25;
const EXTENDED_CLIPBOARD_NOTIFY: u32 = 1 << 27;
const EXTENDED_CLIPBOARD_PROVIDE: u32 = 1 << 28;
const RFB_SECURITY_NONE: u8 = 1;
const RFB_SECURITY_RA2NE: u8 = 6;
const RA2_RSA_BITS: usize = 2048;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const DISPLAY_WAKE_DURATION: Duration = Duration::from_secs(5);
#[cfg(target_os = "macos")]
const MACOS_DISPLAY_WAKE_ARGUMENTS: [&str; 3] = ["-u", "-t", "5"];
#[cfg(target_os = "macos")]
const MACOS_DISPLAY_ASSERTION_ARGUMENTS: [&str; 1] = ["-d"];
const CONFIG_TEMPLATE: &str = include_str!("../config.toml.example");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PixelFormat {
    bits_per_pixel: u8,
    depth: u8,
    big_endian: bool,
    true_color: bool,
    red_max: u16,
    green_max: u16,
    blue_max: u16,
    red_shift: u8,
    green_shift: u8,
    blue_shift: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FramebufferEncoding {
    Raw,
    Hextile,
}

impl FramebufferEncoding {
    fn rfb_value(self) -> i32 {
        match self {
            Self::Raw => RFB_ENCODING_RAW,
            Self::Hextile => RFB_ENCODING_HEXTILE,
        }
    }
}

const SERVER_PIXEL_FORMAT: PixelFormat = PixelFormat {
    bits_per_pixel: 32,
    depth: 24,
    big_endian: false,
    true_color: true,
    red_max: 255,
    green_max: 255,
    blue_max: 255,
    red_shift: 16,
    green_shift: 8,
    blue_shift: 0,
};

#[derive(Debug, Clone, Copy, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum Source {
    /// Capture the primary desktop using native macOS, Windows, or Linux APIs.
    Desktop,
    /// Serve an animated framebuffer without requiring screen-capture permission.
    Demo,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxDisplaySession {
    X11,
    Wayland,
    Unsupported,
}

#[cfg(target_os = "linux")]
fn linux_display_session(
    xdg_session_type: Option<&str>,
    display_available: bool,
    wayland_display_available: bool,
) -> LinuxDisplaySession {
    match xdg_session_type.map(str::to_ascii_lowercase).as_deref() {
        Some("x11") => LinuxDisplaySession::X11,
        Some("wayland") => LinuxDisplaySession::Wayland,
        _ if wayland_display_available => LinuxDisplaySession::Wayland,
        _ if display_available => LinuxDisplaySession::X11,
        _ => LinuxDisplaySession::Unsupported,
    }
}

#[derive(Debug, Clone, Copy, Deserialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum KeyboardLayout {
    /// Interpret printable keysyms using the standard physical-key layout.
    Us,
    /// Interpret JIS keys and Japanese Kana keysyms using macOS JIS key codes.
    Jis,
    /// Use the standard layout, while accepting unambiguous Japanese keysyms.
    Auto,
}

#[derive(Debug, Clone, Copy, Deserialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum LogRotation {
    /// Keep writing one file indefinitely.
    None,
    /// Start a new log file every hour.
    Hourly,
    /// Start a new log file every day.
    Daily,
    /// Start a new log file every seven days.
    Weekly,
    /// Start a new log file every thirty days.
    Monthly,
}

impl LogRotation {
    fn interval(self) -> Option<Duration> {
        match self {
            Self::None => None,
            Self::Hourly => Some(Duration::from_secs(60 * 60)),
            Self::Daily => Some(Duration::from_secs(24 * 60 * 60)),
            Self::Weekly => Some(Duration::from_secs(7 * 24 * 60 * 60)),
            Self::Monthly => Some(Duration::from_secs(30 * 24 * 60 * 60)),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Hourly => "hourly",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "kvnc")]
#[command(version)]
#[command(about = "Cross-platform CLI VNC server / クロスプラットフォーム CLI VNC サーバー")]
struct Args {
    /// Read startup settings from this TOML file, or write it with --init.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Create a commented per-user configuration file, then exit.
    #[arg(long)]
    init: bool,
    /// Replace an existing configuration file when used with --init.
    #[arg(long, requires = "init")]
    force: bool,
    /// Add missing safe defaults to the per-user configuration and create a backup.
    #[arg(long, conflicts_with_all = ["init", "update"])]
    update_config: bool,
    /// Update a clean Git or JJ checkout and rebuild its standalone binary (alias: --upgrade).
    #[arg(long, visible_alias = "upgrade", conflicts_with_all = ["init", "update_config"])]
    update: bool,
    /// Listener address. The loopback default keeps unencrypted RFB traffic local.
    #[arg(long, default_value = "127.0.0.1:5900")]
    listen: SocketAddr,
    /// Framebuffer source.
    #[arg(long, value_enum, default_value_t = Source::Desktop)]
    source: Source,
    /// Capture/update rate for the framebuffer.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=10))]
    fps: u8,
    /// Scale the captured framebuffer from 0.1 to 1.0 to reduce bandwidth.
    #[arg(long, default_value_t = 1.0)]
    scale: f32,
    /// Include RFB handshake and client-message diagnostics in the log.
    #[arg(long)]
    debug: bool,
    /// Append diagnostic output to this file as well as standard error.
    #[arg(long, value_name = "PATH")]
    log_file: Option<PathBuf>,
    /// Rotate the log file periodically: none, hourly, daily, weekly, or monthly.
    #[arg(long, value_enum, default_value_t = LogRotation::None)]
    log_rotation: LogRotation,
    /// Rotate when the current log reaches this many bytes; zero disables the size limit.
    #[arg(long, default_value_t = 0, value_name = "BYTES")]
    log_max_size_bytes: u64,
    /// Number of timestamped rotated log files to retain (zero removes all archives).
    #[arg(long, default_value_t = 14)]
    log_retention: usize,
    /// Detach into the background (macOS and Linux only; requires --log-file).
    #[arg(long, requires = "log_file")]
    daemon: bool,
    /// Write the background process ID to this file; requires --daemon.
    #[arg(long, value_name = "PATH", requires = "daemon")]
    pid_file: Option<PathBuf>,
    /// Allow a trusted client to control the local keyboard and pointer.
    #[arg(long)]
    allow_input: bool,
    /// Request a five-second platform-native display wake after a desktop client connects.
    #[arg(long)]
    wake_display_on_connect: bool,
    /// Keyboard layout for macOS physical-key injection.
    #[arg(long, value_enum, default_value_t = KeyboardLayout::Auto)]
    keyboard_layout: KeyboardLayout,
    /// Capture the zero-based monitor index shown by --list-monitors.
    #[arg(long, value_name = "INDEX")]
    monitor: Option<usize>,
    /// Compose every available monitor into one virtual-desktop framebuffer.
    #[arg(long)]
    all_monitors: bool,
    /// List monitors and exit.
    #[arg(long)]
    list_monitors: bool,
    /// Print effective startup settings without exposing the password verifier, then exit.
    #[arg(long)]
    print_config: bool,
    /// Generate an Argon2id password verifier for the [auth] configuration section and exit.
    #[arg(long)]
    generate_password_hash: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    listen: Option<SocketAddr>,
    source: Option<Source>,
    fps: Option<u8>,
    scale: Option<f32>,
    debug: Option<bool>,
    log_file: Option<PathBuf>,
    log_rotation: Option<LogRotation>,
    log_max_size_bytes: Option<u64>,
    log_retention: Option<usize>,
    daemon: Option<bool>,
    pid_file: Option<PathBuf>,
    allow_input: Option<bool>,
    wake_display_on_connect: Option<bool>,
    keyboard_layout: Option<KeyboardLayout>,
    monitor: Option<usize>,
    all_monitors: Option<bool>,
    auth: Option<AuthFileConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthFileConfig {
    #[serde(default)]
    username: Option<String>,
    password_hash: String,
}

#[derive(Clone)]
struct AuthVerifier {
    username: String,
    password_hash: String,
    private_key: Arc<RsaPrivateKey>,
    public_key_fingerprint: String,
}

impl FileConfig {
    fn apply(self, args: &mut Args) {
        if let Some(value) = self.listen {
            args.listen = value;
        }
        if let Some(value) = self.source {
            args.source = value;
        }
        if let Some(value) = self.fps {
            args.fps = value;
        }
        if let Some(value) = self.scale {
            args.scale = value;
        }
        if let Some(value) = self.debug {
            args.debug = value;
        }
        if let Some(value) = self.log_file {
            args.log_file = Some(value);
        }
        if let Some(value) = self.log_rotation {
            args.log_rotation = value;
        }
        if let Some(value) = self.log_max_size_bytes {
            args.log_max_size_bytes = value;
        }
        if let Some(value) = self.log_retention {
            args.log_retention = value;
        }
        if let Some(value) = self.daemon {
            args.daemon = value;
        }
        if let Some(value) = self.pid_file {
            args.pid_file = Some(value);
        }
        if let Some(value) = self.allow_input {
            args.allow_input = value;
        }
        if let Some(value) = self.wake_display_on_connect {
            args.wake_display_on_connect = value;
        }
        if let Some(value) = self.keyboard_layout {
            args.keyboard_layout = value;
        }
        if let Some(value) = self.monitor {
            args.monitor = Some(value);
        }
        if let Some(value) = self.all_monitors {
            args.all_monitors = value;
        }
    }
}

#[derive(Clone)]
struct Logger {
    debug_enabled: bool,
    file: Option<Arc<StdMutex<RotatingLogFile>>>,
}

struct RotatingLogFile {
    path: PathBuf,
    rotation: LogRotation,
    max_size_bytes: u64,
    retention: usize,
    opened_at: Instant,
    file: Option<File>,
}

impl RotatingLogFile {
    fn open(
        path: &Path,
        rotation: LogRotation,
        max_size_bytes: u64,
        retention: usize,
    ) -> std::io::Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            rotation,
            max_size_bytes,
            retention,
            opened_at: Instant::now(),
            file: Some(Self::open_append(path)?),
        })
    }

    fn rotate_if_due(&mut self) -> std::io::Result<bool> {
        // A user or an external log rotation utility can rename or remove the
        // active path while kVNC still owns the old file descriptor. Reopen
        // before reading the path size so subsequent entries return to the
        // configured active file instead of writing forever to an unlinked
        // inode.
        let reopened = self.reopen_if_replaced()?;
        let interval_due = self
            .rotation
            .interval()
            .is_some_and(|interval| self.opened_at.elapsed() >= interval);
        let size_due = self.max_size_bytes != 0
            && fs::metadata(&self.path)
                .map(|metadata| metadata.len() >= self.max_size_bytes)
                .unwrap_or(false);
        if !interval_due && !size_due {
            return Ok(reopened);
        }

        // Closing our own handle before rename is required on Windows.
        self.file.take();
        let backup = self.next_backup_path();
        if let Err(error) = fs::rename(&self.path, &backup) {
            self.file = Some(Self::open_append(&self.path)?);
            return Err(error);
        }
        self.file = Some(Self::open_append(&self.path)?);
        self.opened_at = Instant::now();
        self.remove_expired_backups()?;
        Ok(true)
    }

    fn open_append(path: &Path) -> std::io::Result<File> {
        OpenOptions::new().create(true).append(true).open(path)
    }

    fn reopen_if_replaced(&mut self) -> std::io::Result<bool> {
        let file_metadata = self.file.as_ref().map(File::metadata).transpose()?;
        let path_metadata = match fs::metadata(&self.path) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let replaced = match (file_metadata.as_ref(), path_metadata.as_ref()) {
            (Some(file), Some(path)) => log_file_identity_differs(file, path),
            _ => true,
        };
        if replaced {
            self.file = Some(Self::open_append(&self.path)?);
            self.opened_at = Instant::now();
        }
        Ok(replaced)
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        if let Some(file) = self.file.as_mut() {
            writeln!(file, "{line}")?;
            file.flush()?;
        }
        Ok(())
    }

    fn next_backup_path(&self) -> PathBuf {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let filename = self
            .path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("kvnc.log"))
            .to_string_lossy();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let stem = format!("{filename}.{}.{:03}", now.as_secs(), now.subsec_millis());
        let candidate = parent.join(&stem);
        if !candidate.exists() {
            return candidate;
        }
        for sequence in 1_u32.. {
            let candidate = parent.join(format!("{stem}.{sequence}"));
            if !candidate.exists() {
                return candidate;
            }
        }
        unreachable!("unbounded sequence always returns a log archive path")
    }

    fn remove_expired_backups(&self) -> std::io::Result<()> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let prefix = format!(
            "{}.",
            self.path
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("kvnc.log"))
                .to_string_lossy()
        );
        let mut backups = fs::read_dir(parent)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        backups.sort();
        let expired = backups.len().saturating_sub(self.retention);
        for path in backups.into_iter().take(expired) {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

fn log_file_identity_differs(file: &fs::Metadata, path: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        file.dev() != path.dev() || file.ino() != path.ino()
    }
    #[cfg(windows)]
    {
        // `volume_serial_number` / `file_index` are still unstable in the
        // standard library. Creation time is stable and changes when an
        // external rotator replaces the configured path; last-write time
        // covers a replacement that preserves the creation timestamp.
        file.creation_time() != path.creation_time()
            || file.last_write_time() != path.last_write_time()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (file, path);
        false
    }
}

impl Logger {
    fn new(
        debug_enabled: bool,
        log_file: Option<&Path>,
        log_rotation: LogRotation,
        log_max_size_bytes: u64,
        log_retention: usize,
    ) -> Result<Self> {
        let file = log_file
            .map(|path| {
                RotatingLogFile::open(path, log_rotation, log_max_size_bytes, log_retention)
            })
            .transpose()?
            .map(|file| Arc::new(StdMutex::new(file)));
        Ok(Self {
            debug_enabled,
            file,
        })
    }

    fn info(&self, message: impl AsRef<str>) {
        self.write("INFO", message.as_ref());
    }

    fn error(&self, message: impl AsRef<str>) {
        self.write("ERROR", message.as_ref());
    }

    fn debug(&self, message: impl AsRef<str>) {
        if self.debug_enabled {
            self.write("DEBUG", message.as_ref());
        }
    }

    fn rotate_if_due(&self) {
        if let Some(file) = &self.file
            && let Ok(mut file) = file.lock()
            && let Err(error) = file.rotate_if_due()
        {
            eprintln!(
                "kVNC log rotation failed for {}: {error}",
                file.path.display()
            );
        }
    }

    fn write(&self, level: &str, message: &str) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let line = format!(
            "[{}.{:03}] {level} {message}",
            timestamp.as_secs(),
            timestamp.subsec_millis()
        );
        eprintln!("{line}");
        if let Some(file) = &self.file
            && let Ok(mut file) = file.lock()
        {
            if let Err(error) = file.rotate_if_due() {
                eprintln!(
                    "kVNC log rotation failed for {}: {error}",
                    file.path.display()
                );
            }
            if let Err(error) = file.write_line(&line) {
                eprintln!("kVNC log write failed for {}: {error}", file.path.display());
                return;
            }
            // Check once more after writing so a size limit is enforced even
            // when this is the final log entry before the next client event.
            if let Err(error) = file.rotate_if_due() {
                eprintln!(
                    "kVNC log rotation failed for {}: {error}",
                    file.path.display()
                );
            }
        }
    }
}

#[derive(Clone)]
struct Frame {
    width: u16,
    height: u16,
    /// Ratio of captured framebuffer pixels to macOS logical input points.
    scale_factor: f32,
    x: i32,
    y: i32,
    /// 32-bit little-endian true-color pixels: blue, green, red, padding.
    bgrx: Vec<u8>,
    /// The physical display associated with each part of a virtual desktop.
    /// A normal, single-monitor frame has exactly one region.
    input_regions: Vec<InputRegion>,
}

#[derive(Clone, Debug)]
struct InputRegion {
    monitor_index: usize,
    /// Rectangle in the RFB virtual-desktop framebuffer.
    rfb_x: u16,
    rfb_y: u16,
    width: u16,
    height: u16,
    /// Native desktop origin and pixel-to-input-point scale for this monitor.
    origin_x: i32,
    origin_y: i32,
    scale_factor: f32,
}

impl Frame {
    fn input_region(&self, x: u16, y: u16) -> Option<&InputRegion> {
        self.input_regions.iter().find(|region| {
            let right = u32::from(region.rfb_x) + u32::from(region.width);
            let bottom = u32::from(region.rfb_y) + u32::from(region.height);
            u32::from(x) >= u32::from(region.rfb_x)
                && u32::from(x) < right
                && u32::from(y) >= u32::from(region.rfb_y)
                && u32::from(y) < bottom
        })
    }
}

struct InputController {
    enigo: Enigo,
    keyboard_layout: KeyboardLayout,
}

enum InputInitialization<T> {
    Disabled,
    Ready(T),
    Unavailable(anyhow::Error),
}

fn initialize_input<T>(
    allow_input: bool,
    initialize: impl FnOnce() -> Result<T>,
) -> InputInitialization<T> {
    if !allow_input {
        return InputInitialization::Disabled;
    }
    match initialize() {
        Ok(input) => InputInitialization::Ready(input),
        Err(error) => InputInitialization::Unavailable(error),
    }
}

impl InputController {
    fn new(keyboard_layout: KeyboardLayout) -> Result<Self> {
        Ok(Self {
            enigo: Enigo::new(&Settings::default())
                .context("could not initialize OS input injection")?,
            keyboard_layout,
        })
    }

    #[cfg(target_os = "macos")]
    /// Post every macOS pointer event through CoreGraphics.  In particular,
    /// do not obtain the button position from `NSEvent::mouseLocation()`: it
    /// is expressed in a different coordinate space on Retina displays.
    fn macos_mouse_event(
        &self,
        event_type: CGEventType,
        mouse_button: CGMouseButton,
        x: i32,
        y: i32,
    ) -> Result<()> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .map_err(|_| anyhow::anyhow!("could not initialize macOS mouse event source"))?;
        let event = CGEvent::new_mouse_event(
            source,
            event_type,
            CGPoint::new(f64::from(x), f64::from(y)),
            mouse_button,
        )
        .map_err(|_| anyhow::anyhow!("could not create macOS pointer event"))?;
        // Match the event characteristics macOS normally receives from an
        // input device.  This keeps synthetic movement and button events in
        // the same stream and avoids dropped button presses.
        let mut flags = CGEventFlags::CGEventFlagNonCoalesced;
        flags.set(CGEventFlags::from_bits_retain(0x2000_0000), true);
        event.set_flags(flags);
        event.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, 0);
        if matches!(
            event_type,
            CGEventType::LeftMouseDown
                | CGEventType::LeftMouseUp
                | CGEventType::RightMouseDown
                | CGEventType::RightMouseUp
                | CGEventType::OtherMouseDown
                | CGEventType::OtherMouseUp
        ) {
            event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, 1);
        }
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn macos_motion(&self, previous_mask: u8, x: i32, y: i32) -> Result<()> {
        let (event_type, mouse_button) = if previous_mask & 1 != 0 {
            (CGEventType::LeftMouseDragged, CGMouseButton::Left)
        } else if previous_mask & 4 != 0 {
            (CGEventType::RightMouseDragged, CGMouseButton::Right)
        } else if previous_mask & 2 != 0 {
            (CGEventType::OtherMouseDragged, CGMouseButton::Center)
        } else {
            (CGEventType::MouseMoved, CGMouseButton::Left)
        };
        self.macos_mouse_event(event_type, mouse_button, x, y)
    }

    #[cfg(target_os = "macos")]
    fn macos_button(&self, button: Button, down: bool, x: i32, y: i32) -> Result<()> {
        let (event_type, mouse_button) = match (button, down) {
            (Button::Left, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left),
            (Button::Left, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left),
            (Button::Right, true) => (CGEventType::RightMouseDown, CGMouseButton::Right),
            (Button::Right, false) => (CGEventType::RightMouseUp, CGMouseButton::Right),
            (Button::Middle, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center),
            (Button::Middle, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center),
            _ => return Ok(()),
        };
        self.macos_mouse_event(event_type, mouse_button, x, y)
    }

    fn key(&mut self, down: bool, keysym: u32) -> Result<()> {
        let key = rfb_key(keysym, self.keyboard_layout).context("unsupported RFB keysym")?;
        self.enigo
            .key(
                key,
                if down {
                    Direction::Press
                } else {
                    Direction::Release
                },
            )
            .context("could not inject keyboard event")
    }

    fn pointer(
        &mut self,
        mask: u8,
        previous_mask: u8,
        coordinates: (u16, u16),
        origin: (i32, i32),
        scale_factor: f32,
    ) -> Result<(i32, i32)> {
        let (input_x, input_y) = input_coordinates(coordinates, origin, scale_factor);
        #[cfg(target_os = "macos")]
        self.macos_motion(previous_mask, input_x, input_y)
            .context("could not inject macOS pointer motion")?;
        #[cfg(not(target_os = "macos"))]
        self.enigo
            .move_mouse(input_x, input_y, Coordinate::Abs)
            .context("could not inject pointer motion")?;
        for (bit, button) in [(1, Button::Left), (2, Button::Middle), (4, Button::Right)] {
            let was_down = previous_mask & bit != 0;
            let is_down = mask & bit != 0;
            if was_down != is_down {
                #[cfg(target_os = "macos")]
                self.macos_button(button, is_down, input_x, input_y)
                    .context("could not inject macOS pointer button")?;
                #[cfg(not(target_os = "macos"))]
                self.enigo
                    .button(
                        button,
                        if is_down {
                            Direction::Press
                        } else {
                            Direction::Release
                        },
                    )
                    .context("could not inject pointer button")?;
            }
        }
        if mask & 8 != 0 {
            self.enigo
                .scroll(1, Axis::Vertical)
                .context("could not inject pointer scroll")?;
        }
        if mask & 16 != 0 {
            self.enigo
                .scroll(-1, Axis::Vertical)
                .context("could not inject pointer scroll")?;
        }
        if mask & 32 != 0 {
            self.enigo
                .scroll(-1, Axis::Horizontal)
                .context("could not inject pointer scroll")?;
        }
        if mask & 64 != 0 {
            self.enigo
                .scroll(1, Axis::Horizontal)
                .context("could not inject pointer scroll")?;
        }
        Ok((input_x, input_y))
    }
}

/// Converts a point in the scaled RFB framebuffer to an absolute OS input point.
///
/// RFB positions address pixels in the captured image. On Retina displays the
/// macOS event system uses logical display points. The caller must supply the
/// latest captured frame metadata: a display wake or reconfiguration can move
/// a monitor after a client has already completed its RFB handshake.
fn input_coordinates(coordinates: (u16, u16), origin: (i32, i32), scale_factor: f32) -> (i32, i32) {
    let scale_factor = scale_factor.max(f32::EPSILON);
    (
        origin.0 + (f32::from(coordinates.0) / scale_factor).round() as i32,
        origin.1 + (f32::from(coordinates.1) / scale_factor).round() as i32,
    )
}

fn rfb_key(keysym: u32, keyboard_layout: KeyboardLayout) -> Option<Key> {
    let key = match keysym {
        // Use the physical space key rather than layout-dependent Unicode
        // conversion.  On macOS the latter can be interpreted by a Japanese
        // IME as an input-source toggle.
        0x20 => Key::Space,
        0xff08 => Key::Backspace,
        0xff09 => Key::Tab,
        0xff0d => Key::Return,
        0xff1b => Key::Escape,
        0xff50 => Key::Home,
        0xff51 => Key::LeftArrow,
        0xff52 => Key::UpArrow,
        0xff53 => Key::RightArrow,
        0xff54 => Key::DownArrow,
        0xff55 => Key::PageUp,
        0xff56 => Key::PageDown,
        0xff57 => Key::End,
        0xffff => Key::Delete,
        0xffe1 | 0xffe2 => Key::Shift,
        0xffe3 | 0xffe4 => Key::Control,
        0xffe9 | 0xffea => Key::Alt,
        0xffeb | 0xffec => Key::Meta,
        0xffbe => Key::F1,
        0xffbf => Key::F2,
        0xffc0 => Key::F3,
        0xffc1 => Key::F4,
        0xffc2 => Key::F5,
        0xffc3 => Key::F6,
        0xffc4 => Key::F7,
        0xffc5 => Key::F8,
        0xffc6 => Key::F9,
        0xffc7 => Key::F10,
        0xffc8 => Key::F11,
        0xffc9 => Key::F12,
        // An RFB KeyEvent carries an X11 keysym, not text encoded as UTF-8.
        // On macOS, injecting that keysym as Unicode asks the active input
        // method to find a character again.  That is wrong for a JIS client:
        // for example the physical key reported as `i` must remain the `i`
        // key even when the local input source is Japanese Kana.  Vine Server
        // uses a fixed key table in its default configuration, so do the same
        // here.  `Key::Other` is a platform key code on macOS.
        #[cfg(target_os = "macos")]
        value => {
            if keyboard_layout != KeyboardLayout::Us
                && let Some(keycode) = macos_jis_keysym_keycode(value)
            {
                return Some(Key::Other(keycode));
            }
            return macos_us_keysym_keycode(value).map(Key::Other);
        }
        #[cfg(not(target_os = "macos"))]
        value => {
            let _ = keyboard_layout;
            return char::from_u32(value).map(Key::Unicode);
        }
    };
    Some(key)
}

/// Maps the Japanese keysyms emitted by a JIS browser client to physical macOS
/// JIS keys. The aliases cover clients that report the character printed on a
/// Kana key (`け`, `む`, `ろ`) as well as clients that report its ASCII/JIS
/// symbol (`:`, `]`, `¥`).
#[cfg(target_os = "macos")]
fn macos_jis_keysym_keycode(keysym: u32) -> Option<u32> {
    let keycode = match keysym {
        0x3041 | 0x3042 => 20,        // ぁ / あ
        0x3044 => 14,                 // い
        0x3046 => 21,                 // う
        0x3048 => 23,                 // え
        0x304a => 22,                 // お
        0x304b => 17,                 // か
        0x304d => 5,                  // き
        0x304f => 4,                  // く
        0x3051 | 0x3a => 39,          // け / :
        0x3053 => 11,                 // こ
        0x3055 => 7,                  // さ
        0x3057 => 2,                  // し
        0x3059 => 15,                 // す
        0x305b => 35,                 // せ
        0x305d => 8,                  // そ
        0x305f => 12,                 // た
        0x3061 => 0,                  // ち
        0x3064 => 6,                  // つ
        0x3066 => 13,                 // て
        0x3068 => 1,                  // と
        0x306a => 32,                 // な
        0x306b => 34,                 // に
        0x306c => 18,                 // ぬ
        0x306d => 43,                 // ね
        0x306e => 40,                 // の
        0x306f => 3,                  // は
        0x3072 => 9,                  // ひ
        0x3075 => 19,                 // ふ
        0x3078 | 0x5e => 24,          // へ / ^
        0x307b => 27,                 // ほ
        0x307e => 45,                 // み
        0x3080 | 0x5d => 42,          // む / ]
        0x3081 => 44,                 // め
        0x3082 => 46,                 // も
        0x3084 => 26,                 // や
        0x3086 => 28,                 // ゆ
        0x3088 => 25,                 // よ
        0x3089 => 31,                 // ら
        0x308a => 37,                 // り
        0x308b => 47,                 // る
        0x308c | 0x3b => 41,          // れ / ;
        0x308f => 29,                 // わ
        0x3092 => 44,                 // を
        0x3093 => 16,                 // ん
        0x309b | 0x3099 | 0x40 => 33, // ゛ / combining voiced sound mark / @
        0x309c | 0x309a | 0x5b => 30, // ゜ / combining semi-voiced sound mark / [
        0x5c | 0x308d | 0x5f => 94,   // backslash / ろ / underscore (JIS Ro key)
        0xa5 | 0x30fc => 93,          // yen / prolonged sound mark (JIS Yen key)
        _ => return None,
    };
    Some(keycode)
}

/// Maps printable US/X11 keysyms to the corresponding physical macOS key.
///
/// The mapping intentionally represents keyboard positions, rather than text.
/// Modifier events are delivered separately by RFB, as they are by normal
/// keyboards.  This keeps Japanese Kana/Romaji input source handling in macOS
/// and avoids reinterpreting an RFB keysym through the current IME.
#[cfg(target_os = "macos")]
fn macos_us_keysym_keycode(keysym: u32) -> Option<u32> {
    let keysym = u8::try_from(keysym).ok()?;
    let keycode = match keysym {
        b'a' | b'A' => 0,
        b's' | b'S' => 1,
        b'd' | b'D' => 2,
        b'f' | b'F' => 3,
        b'h' | b'H' => 4,
        b'g' | b'G' => 5,
        b'z' | b'Z' => 6,
        b'x' | b'X' => 7,
        b'c' | b'C' => 8,
        b'v' | b'V' => 9,
        b'b' | b'B' => 11,
        b'q' | b'Q' => 12,
        b'w' | b'W' => 13,
        b'e' | b'E' => 14,
        b'r' | b'R' => 15,
        b'y' | b'Y' => 16,
        b't' | b'T' => 17,
        b'1' | b'!' => 18,
        b'2' | b'@' => 19,
        b'3' | b'#' => 20,
        b'4' | b'$' => 21,
        b'6' | b'^' => 22,
        b'5' | b'%' => 23,
        b'=' | b'+' => 24,
        b'9' | b'(' => 25,
        b'7' | b'&' => 26,
        b'-' | b'_' => 27,
        b'8' | b'*' => 28,
        b'0' | b')' => 29,
        b']' | b'}' => 30,
        b'o' | b'O' => 31,
        b'u' | b'U' => 32,
        b'[' | b'{' => 33,
        b'i' | b'I' => 34,
        b'p' | b'P' => 35,
        b'l' | b'L' => 37,
        b'j' | b'J' => 38,
        b'\'' | b'"' => 39,
        b'k' | b'K' => 40,
        b';' | b':' => 41,
        b'\\' | b'|' => 42,
        b',' | b'<' => 43,
        b'/' | b'?' => 44,
        b'n' | b'N' => 45,
        b'm' | b'M' => 46,
        b'.' | b'>' => 47,
        b'`' | b'~' => 50,
        _ => return None,
    };
    Some(keycode)
}

#[tokio::main]
async fn main() -> Result<()> {
    let invoked_without_options = std::env::args_os().len() == 1;
    let mut args = Args::parse();
    if args.init {
        initialize_user_config(args.config.as_deref(), args.force, args.allow_input)?;
        return Ok(());
    }
    if args.update_config {
        update_user_config(args.config.as_deref())?;
        return Ok(());
    }
    if args.update {
        update_git_checkout()?;
        return Ok(());
    }
    let config_path = args.config.clone().or_else(|| {
        (invoked_without_options || args.print_config)
            .then(default_config_path)
            .flatten()
            .filter(|path| path.exists())
    });
    let auth = if let Some(path) = config_path.as_deref() {
        let config = load_file_config(path)?;
        let auth = build_auth_verifier(config.auth.as_ref())?;
        config.apply(&mut args);
        auth
    } else {
        None
    };
    if args.generate_password_hash {
        print_password_hash()?;
        return Ok(());
    }
    if args.print_config {
        print_effective_config(&args, config_path.as_deref(), auth.as_ref());
        return Ok(());
    }
    if !(1..=10).contains(&args.fps) {
        bail!("--fps must be between 1 and 10");
    }
    if !(0.1..=1.0).contains(&args.scale) {
        bail!("--scale must be between 0.1 and 1.0");
    }
    if args.daemon && args.log_file.is_none() {
        bail!("--daemon requires log_file in the command line or config file");
    }
    if args.pid_file.is_some() && !args.daemon {
        bail!("pid_file requires daemon in the command line or config file");
    }
    if (args.log_rotation != LogRotation::None || args.log_max_size_bytes != 0)
        && args.log_file.is_none()
    {
        bail!("log_rotation and log_max_size_bytes require log_file");
    }
    if args.list_monitors {
        list_monitors()?;
        return Ok(());
    }
    detach_if_requested(&args)?;
    let logger = Logger::new(
        args.debug,
        args.log_file.as_deref(),
        args.log_rotation,
        args.log_max_size_bytes,
        args.log_retention,
    )?;
    if let Some(rotation_interval) = args.log_rotation.interval() {
        let rotation_logger = logger.clone();
        tokio::spawn(async move {
            let mut ticker = interval(rotation_interval);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                rotation_logger.rotate_if_due();
            }
        });
    }
    if args.log_rotation != LogRotation::None || args.log_max_size_bytes != 0 {
        logger.info(format!(
            "log rotation enabled: schedule={}, max_size_bytes={}, retention={}",
            args.log_rotation.name(),
            args.log_max_size_bytes,
            args.log_retention
        ));
    }
    if let Some(auth) = &auth {
        logger.info(format!(
            "RA2ne server key generated: fingerprint_sha256={}",
            auth.public_key_fingerprint
        ));
    }
    let wake_display_on_connect =
        args.wake_display_on_connect && matches!(args.source, Source::Desktop);
    if args.wake_display_on_connect {
        if wake_display_on_connect {
            #[cfg(target_os = "macos")]
            logger.info(
                "platform display wake on successful desktop-client connection enabled: macos_assertion=client-lifetime",
            );
            #[cfg(not(target_os = "macos"))]
            logger.info(
                "platform display wake on successful desktop-client connection enabled: duration_seconds=5",
            );
        }
        if !wake_display_on_connect {
            logger.info("display wake on connect is ignored because source is not desktop");
        }
    }
    let input = match initialize_input(args.allow_input, || {
        InputController::new(args.keyboard_layout)
    }) {
        InputInitialization::Disabled => {
            logger.info("local keyboard and pointer input injection disabled");
            None
        }
        InputInitialization::Ready(controller) => {
            logger.info("local keyboard and pointer input injection enabled");
            Some(Arc::new(StdMutex::new(controller)))
        }
        InputInitialization::Unavailable(error) => {
            logger.error(format!(
                "local keyboard and pointer input injection is unavailable; continuing in view-only mode: {error}"
            ));
            None
        }
    };
    let desktop_monitors = match args.source {
        Source::Desktop => Some(select_monitors(args.monitor, args.all_monitors)?),
        Source::Demo => None,
    };
    if let Some(monitors) = &desktop_monitors {
        if args.all_monitors {
            logger.info(format!(
                "desktop virtual-desktop selected: all_monitors=true, count={}",
                monitors.len()
            ));
        }
        for (index, monitor) in monitors {
            logger.info(format!(
                "desktop monitor selected: configured_index={:?}, index={index}, {}",
                args.monitor,
                monitor_description(monitor)
            ));
        }
        #[cfg(target_os = "macos")]
        logger.info("macOS desktop capture backend: CoreGraphics CGDisplayStream");
    }
    #[cfg(target_os = "macos")]
    let mut macos_desktop_capture =
        MacosDesktopCapture::new(args.fps, args.all_monitors, logger.clone());
    let initial_frame = {
        #[cfg(target_os = "macos")]
        {
            match args.source {
                Source::Desktop => macos_desktop_capture.capture(
                    desktop_monitors
                        .as_deref()
                        .context("no monitor is available")?,
                    args.scale,
                ),
                Source::Demo => first_frame(args.source, desktop_monitors.as_deref(), args.scale),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            first_frame(args.source, desktop_monitors.as_deref(), args.scale)
        }
    }
    .context("could not create the initial framebuffer")?;
    if args.all_monitors && matches!(args.source, Source::Desktop) {
        for region in &initial_frame.input_regions {
            logger.info(format!(
                "virtual desktop input region: monitor_index={}, rfb_x={}, rfb_y={}, width={}, height={}, desktop_origin_x={}, desktop_origin_y={}, input_scale={}",
                region.monitor_index,
                region.rfb_x,
                region.rfb_y,
                region.width,
                region.height,
                region.origin_x,
                region.origin_y,
                region.scale_factor
            ));
        }
    }
    let framebuffer = Arc::new(RwLock::new(initial_frame.clone()));
    let (clipboard_tx, _) = broadcast::channel::<String>(16);
    let (clipboard_from_client_tx, mut clipboard_from_client_rx) = mpsc::unbounded_channel();
    let clipboard_state = Arc::new(RwLock::new(
        system_clipboard().unwrap_or(None).unwrap_or_default(),
    ));

    let listener = TcpListener::bind(args.listen)
        .await
        .with_context(|| format!("could not listen on {}", args.listen))?;
    println!("kVNC RFB server listening on {}", args.listen);
    println!("Connect an RFB 3.8 client to: tcp://{}", args.listen);
    println!(
        "Framebuffer: {}x{} ({:?})",
        initial_frame.width, initial_frame.height, args.source
    );
    println!(
        "Clipboard: legacy and Extended Clipboard text (maximum {} bytes)",
        MAX_CLIPBOARD_BYTES
    );
    println!(
        "Authentication: {}",
        if auth.is_some() {
            "RA2ne username/password"
        } else {
            "none"
        }
    );
    let auth_mode = match auth.as_ref() {
        None => "none",
        Some(auth) if auth.username.is_empty() => "password-only",
        Some(_) => "username-password",
    };
    logger.info(format!(
        "started: listen={}, source={:?}, monitor={:?}, all_monitors={}, framebuffer={}x{} at {},{} input_scale={}, capture_scale={}, fps={}, debug={}, auth={auth_mode}",
        args.listen, args.source, args.monitor, args.all_monitors, initial_frame.width, initial_frame.height, initial_frame.x, initial_frame.y, initial_frame.scale_factor, args.scale, args.fps, args.debug
    ));

    let accept_framebuffer = Arc::clone(&framebuffer);
    let accept_clipboard_tx = clipboard_tx.clone();
    let accept_logger = logger.clone();
    let accept_input = input.clone();
    let accept_clipboard_state = Arc::clone(&clipboard_state);
    let accept_auth = auth.clone();
    let accept_wake_display_on_connect = wake_display_on_connect;
    let client_frame_interval = Duration::from_secs_f64(1.0 / f64::from(args.fps));
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    accept_logger.info(format!("client accepted: peer={peer}"));
                    let frame = Arc::clone(&accept_framebuffer);
                    let clipboard_rx = accept_clipboard_tx.subscribe();
                    let inbound = clipboard_from_client_tx.clone();
                    let client_logger = accept_logger.clone();
                    let client_input = accept_input.clone();
                    let client_clipboard_state = Arc::clone(&accept_clipboard_state);
                    let client_auth = accept_auth.clone();
                    let client_wake_display_on_connect = accept_wake_display_on_connect;
                    tokio::spawn(async move {
                        if let Err(error) = serve_client(
                            stream,
                            peer,
                            frame,
                            clipboard_rx,
                            inbound,
                            client_clipboard_state,
                            client_logger.clone(),
                            client_input,
                            client_auth,
                            client_wake_display_on_connect,
                            client_frame_interval,
                        )
                        .await
                        {
                            let normal_disconnect =
                                error.downcast_ref::<std::io::Error>().is_some_and(|cause| {
                                    matches!(
                                        cause.kind(),
                                        std::io::ErrorKind::UnexpectedEof
                                            | std::io::ErrorKind::ConnectionReset
                                            | std::io::ErrorKind::BrokenPipe
                                    )
                                });
                            if normal_disconnect {
                                client_logger.info(format!(
                                    "client connection ended: peer={peer}; reason={error}"
                                ));
                            } else {
                                client_logger.error(format!(
                                    "client disconnected: peer={peer}; reason={error:#}"
                                ));
                            }
                        }
                    });
                }
                Err(error) => accept_logger.error(format!("could not accept VNC client: {error}")),
            }
        }
    });

    let clipboard_tx_for_client = clipboard_tx.clone();
    let clipboard_logger = logger.clone();
    let clipboard_state_for_client = Arc::clone(&clipboard_state);
    tokio::spawn(async move {
        while let Some(text) = clipboard_from_client_rx.recv().await {
            if text.len() > MAX_CLIPBOARD_BYTES {
                clipboard_logger.error("ignored oversized clipboard received from VNC client");
            } else if let Err(error) = set_system_clipboard(&text) {
                clipboard_logger.error(format!("could not update system clipboard: {error:#}"));
            } else {
                *clipboard_state_for_client.write().await = text.clone();
                clipboard_logger.debug(format!(
                    "system clipboard updated from client: bytes={}",
                    text.len()
                ));
                let _ = clipboard_tx_for_client.send(text);
            }
        }
    });

    let mut last_clipboard = clipboard_state.read().await.clone();
    let mut ticker = interval(Duration::from_secs_f64(1.0 / f64::from(args.fps)));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut demo_frame_number = 0_u32;

    loop {
        ticker.tick().await;
        let next_frame = match args.source {
            Source::Desktop => {
                #[cfg(target_os = "macos")]
                {
                    // Look up configured displays every time: a display wake
                    // or reconfiguration may replace a monitor object. The
                    // persistent CGDisplayStream is retained by display ID.
                    select_monitors(args.monitor, args.all_monitors)
                        .and_then(|monitors| macos_desktop_capture.capture(&monitors, args.scale))
                }
                #[cfg(not(target_os = "macos"))]
                {
                    select_monitors(args.monitor, args.all_monitors)
                        .and_then(|monitors| capture_monitors(&monitors, args.scale))
                }
            }
            Source::Demo => Ok(demo_frame(
                initial_frame.width,
                initial_frame.height,
                demo_frame_number,
            )),
        };
        demo_frame_number = demo_frame_number.wrapping_add(1);
        match next_frame {
            Ok(frame)
                if frame.width == initial_frame.width && frame.height == initial_frame.height =>
            {
                *framebuffer.write().await = frame;
            }
            Ok(frame) => logger.error(format!(
                "display size changed to {}x{}; restart kVNC",
                frame.width, frame.height
            )),
            Err(error) => logger.error(format!("could not capture framebuffer: {error:#}")),
        }
        match system_clipboard() {
            Ok(Some(text)) if text != last_clipboard => {
                last_clipboard = text.clone();
                *clipboard_state.write().await = text.clone();
                if text.len() <= MAX_CLIPBOARD_BYTES {
                    logger.debug(format!("local clipboard changed: bytes={}", text.len()));
                    let _ = clipboard_tx.send(text);
                } else {
                    logger.error("ignored oversized local clipboard");
                }
            }
            // An empty clipboard or one without UTF-8 text is normal. Keep
            // the last synchronized RFB text rather than logging at every
            // framebuffer tick or clearing a client's unrelated clipboard.
            Ok(_) => {}
            Err(error) => logger.error(format!("could not read system clipboard: {error:#}")),
        }
    }
}

fn load_file_config(path: &Path) -> Result<FileConfig> {
    let contents = read_to_string(path)
        .with_context(|| format!("could not read config file {}", path.display()))?;
    toml::from_str(&contents)
        .with_context(|| format!("could not parse config file {}", path.display()))
}

/// Returns the per-user configuration path used for argument-free startup.
/// Keeping this outside the repository prevents listener addresses, logs, and
/// password verifiers from being accidentally committed.
fn default_config_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(|path| PathBuf::from(path).join("AppData/Roaming"))
            })
            .map(|path| path.join("kvnc/config.toml"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".config")))
            .map(|path| path.join("kvnc/config.toml"))
    }
}

/// Create a commented, safe-by-default user configuration outside the
/// repository. The generated absolute log path keeps argument-free starts
/// from writing logs into an arbitrary current directory.
fn initialize_user_config(
    requested_path: Option<&Path>,
    force: bool,
    allow_input: bool,
) -> Result<()> {
    let path = requested_path
        .map(Path::to_path_buf)
        .or_else(default_config_path)
        .context("could not determine a per-user config path; pass `--config PATH --init`")?;
    write_initial_config(&path, force, allow_input)?;
    println!("Created kVNC configuration: {}", path.display());
    if allow_input {
        println!("Local input control is enabled; use only with trusted clients.");
    } else {
        println!(
            "Local input control is disabled; add allow_input = true only for trusted clients."
        );
    }
    println!("Next: kvnc --print-config");
    Ok(())
}

fn write_initial_config(path: &Path, force: bool, allow_input: bool) -> Result<()> {
    let parent = path
        .parent()
        .context("configuration path must have a parent directory")?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "could not create configuration directory {}",
            parent.display()
        )
    })?;
    let log_path = parent.join("kvnc.log");
    let log_path = log_path.to_string_lossy().replace('\\', "\\\\");
    let contents = CONFIG_TEMPLATE
        .replace(
            "log_file = \"kvnc.log\"",
            &format!("log_file = \"{log_path}\""),
        )
        .replace(
            "allow_input = false",
            &format!("allow_input = {allow_input}"),
        );

    if force {
        fs::write(path, contents)
            .with_context(|| format!("could not replace configuration file {}", path.display()))?;
    } else {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| {
                format!(
                    "configuration file {} already exists; use `--init --force` to replace it",
                    path.display()
                )
            })?;
        file.write_all(contents.as_bytes())
            .with_context(|| format!("could not write configuration file {}", path.display()))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("could not protect configuration file {}", path.display()))?;
    }
    Ok(())
}

/// Add defaults only for absent top-level fields. Keeping this as raw TOML
/// lines preserves an operator's comments and optional [auth] section.
fn update_user_config(requested_path: Option<&Path>) -> Result<()> {
    let path = requested_path
        .map(Path::to_path_buf)
        .or_else(default_config_path)
        .context(
            "could not determine a per-user config path; pass `--config PATH --update-config`",
        )?;
    if !path.exists() {
        write_initial_config(&path, false, false)?;
        println!("Created kVNC configuration: {}", path.display());
        return Ok(());
    }
    let added = add_missing_config_defaults(&path)?;
    if added.is_empty() {
        println!("kVNC configuration is already current: {}", path.display());
    } else {
        println!("Updated kVNC configuration: {}", path.display());
        println!("Added defaults: {}", added.join(", "));
    }
    Ok(())
}

fn add_missing_config_defaults(path: &Path) -> Result<Vec<&'static str>> {
    let contents = read_to_string(path)
        .with_context(|| format!("could not read configuration file {}", path.display()))?;
    load_file_config(path)?;
    let table = contents
        .parse::<toml::Table>()
        .with_context(|| format!("could not parse configuration file {}", path.display()))?;
    let defaults = [
        ("listen", "listen = \"127.0.0.1:5900\""),
        ("source", "source = \"desktop\""),
        ("monitor", "monitor = 0"),
        ("all_monitors", "all_monitors = false"),
        ("allow_input", "allow_input = false"),
        ("wake_display_on_connect", "wake_display_on_connect = false"),
        ("keyboard_layout", "keyboard_layout = \"auto\""),
        ("scale", "scale = 1.0"),
        ("fps", "fps = 2"),
        ("debug", "debug = true"),
        ("log_file", "log_file = \"kvnc.log\""),
        ("log_rotation", "log_rotation = \"daily\""),
        ("log_max_size_bytes", "log_max_size_bytes = 104857600"),
        ("log_retention", "log_retention = 14"),
    ];
    let added: Vec<_> = defaults
        .iter()
        .filter_map(|(key, line)| (!table.contains_key(*key)).then_some((*key, *line)))
        .collect();
    if added.is_empty() {
        return Ok(Vec::new());
    }

    let backup = next_backup_path(path)?;
    fs::copy(path, &backup).with_context(|| {
        format!(
            "could not back up configuration {} to {}",
            path.display(),
            backup.display()
        )
    })?;
    let insertion = format!(
        "# Added by `kvnc --update-config`; review before exposing kVNC beyond localhost.\n{}\n\n",
        added
            .iter()
            .map(|(_, line)| *line)
            .collect::<Vec<_>>()
            .join("\n")
    );
    let table_start = contents
        .lines()
        .scan(0_usize, |offset, line| {
            let start = *offset;
            *offset += line.len() + 1;
            Some((start, line))
        })
        .find_map(|(offset, line)| line.trim_start().starts_with('[').then_some(offset));
    let updated = match table_start {
        Some(offset) => format!(
            "{}{}{}",
            &contents[..offset],
            insertion,
            &contents[offset..]
        ),
        None => format!("{contents}\n{insertion}"),
    };
    fs::write(path, updated)
        .with_context(|| format!("could not update configuration file {}", path.display()))?;
    println!("Backup: {}", backup.display());
    Ok(added.into_iter().map(|(key, _)| key).collect())
}

fn next_backup_path(path: &Path) -> Result<PathBuf> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_secs();
    let base = PathBuf::from(format!("{}.backup.{timestamp}", path.display()));
    if !base.exists() {
        return Ok(base);
    }
    for suffix in 1_u32.. {
        let candidate = PathBuf::from(format!("{}.backup.{timestamp}.{suffix}", path.display()));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    unreachable!("u32 backup suffix space is exhausted")
}

fn update_git_checkout() -> Result<()> {
    let root = checkout_root()?;
    match checkout_kind(&root) {
        CheckoutKind::Jujutsu => update_jj_checkout(&root)?,
        CheckoutKind::Git => update_standard_git_checkout(&root)?,
    }
    build_checkout_binary(&root)?;
    #[cfg(windows)]
    {
        println!("Checkout updated: {}", root.display());
        println!("Windows cannot replace a running kvnc.exe. After this process exits, run:");
        println!("  Copy-Item -Force .\\target\\release\\kvnc.exe .\\bin\\kvnc.exe");
        Ok(())
    }
    #[cfg(not(windows))]
    {
        replace_checkout_binary(&root)?;
        println!(
            "kVNC checkout and standalone binary updated: {}",
            root.display()
        );
        Ok(())
    }
}

fn build_checkout_binary(root: &Path) -> Result<()> {
    run_checkout_command(root, "cargo", ["build", "--release", "--locked"])?;
    Ok(())
}

#[cfg(not(windows))]
fn checkout_built_binary(root: &Path) -> PathBuf {
    root.join("target")
        .join("release")
        .join(format!("kvnc{}", std::env::consts::EXE_SUFFIX))
}

#[cfg(not(windows))]
fn replace_checkout_binary(root: &Path) -> Result<()> {
    let built_binary = checkout_built_binary(root);
    let destination_directory = root.join("bin");
    let destination = destination_directory.join("kvnc");
    let replacement = destination_directory.join("kvnc.next");
    fs::create_dir_all(&destination_directory).with_context(|| {
        format!(
            "could not create standalone-binary directory {}",
            destination_directory.display()
        )
    })?;
    fs::copy(&built_binary, &replacement).with_context(|| {
        format!(
            "could not stage rebuilt standalone binary from {} to {}",
            built_binary.display(),
            replacement.display()
        )
    })?;
    fs::rename(&replacement, &destination).with_context(|| {
        format!(
            "could not replace standalone binary {} with {}",
            destination.display(),
            replacement.display()
        )
    })?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckoutKind {
    Git,
    Jujutsu,
}

fn checkout_kind(root: &Path) -> CheckoutKind {
    if root.join(".jj").is_dir() {
        CheckoutKind::Jujutsu
    } else {
        CheckoutKind::Git
    }
}

fn update_standard_git_checkout(root: &Path) -> Result<()> {
    let status = checkout_command_output(root, "git", ["status", "--porcelain"])?;
    if !status.trim().is_empty() {
        bail!(
            "refusing to update a dirty checkout {}; commit, stash, or discard local changes first",
            root.display()
        );
    }
    let branch = checkout_command_output(root, "git", ["branch", "--show-current"])?;
    if branch.trim().is_empty() {
        bail!(
            "--update requires a checked-out Git branch with an upstream; this checkout has a detached HEAD"
        );
    }
    run_checkout_command(root, "git", ["pull", "--ff-only"])?;
    Ok(())
}

fn update_jj_checkout(root: &Path) -> Result<()> {
    let diff = checkout_command_output(root, "jj", ["diff", "--summary", "-r", "@"])?;
    if !diff.trim().is_empty() {
        bail!(
            "refusing to update a JJ checkout with changes in the working-copy change {}; describe and push it, then create an empty change before retrying",
            root.display()
        );
    }
    run_checkout_command(root, "jj", ["git", "fetch"])?;
    run_checkout_command(root, "jj", ["new", "main@origin"])?;
    Ok(())
}

fn checkout_root() -> Result<PathBuf> {
    let executable =
        std::env::current_exe().context("could not determine the running executable")?;
    let candidate = executable
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .context("could not determine the checkout directory")?;
    let has_version_control = candidate.join(".git").exists() || candidate.join(".jj").is_dir();
    if has_version_control && candidate.join("Cargo.toml").exists() {
        Ok(candidate)
    } else {
        bail!(
            "--update requires a kVNC Git or JJ checkout with Cargo.toml; update a downloaded release manually"
        )
    }
}

fn run_checkout_command<const N: usize>(
    root: &Path,
    program: &str,
    arguments: [&str; N],
) -> Result<String> {
    let stdout = checkout_command_output(root, program, arguments)?;
    if !stdout.trim().is_empty() {
        print!("{stdout}");
    }
    Ok(stdout)
}

fn checkout_command_output<const N: usize>(
    root: &Path,
    program: &str,
    arguments: [&str; N],
) -> Result<String> {
    let output = Command::new(program)
        .args(arguments)
        .current_dir(root)
        .output()
        .with_context(|| format!("could not run {program}; install it and try again"))?;
    if !output.status.success() {
        bail!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(stdout)
}

fn print_effective_config(args: &Args, config_path: Option<&Path>, auth: Option<&AuthVerifier>) {
    println!(
        "config_path = {}",
        match config_path {
            Some(path) => format!("{:?}", path.display().to_string()),
            None => "none".to_owned(),
        }
    );
    println!("listen = {:?}", args.listen.to_string());
    println!(
        "source = {:?}",
        match args.source {
            Source::Desktop => "desktop",
            Source::Demo => "demo",
        }
    );
    println!(
        "monitor = {}",
        args.monitor
            .map(|value| value.to_string())
            .unwrap_or_else(|| "primary".to_owned())
    );
    println!("all_monitors = {}", args.all_monitors);
    println!("allow_input = {}", args.allow_input);
    println!("wake_display_on_connect = {}", args.wake_display_on_connect);
    println!(
        "keyboard_layout = {:?}",
        match args.keyboard_layout {
            KeyboardLayout::Auto => "auto",
            KeyboardLayout::Us => "us",
            KeyboardLayout::Jis => "jis",
        }
    );
    println!("scale = {}", args.scale);
    println!("fps = {}", args.fps);
    println!("debug = {}", args.debug);
    println!(
        "log_file = {}",
        args.log_file
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "none".to_owned())
    );
    println!("log_rotation = {:?}", args.log_rotation.name());
    println!("log_max_size_bytes = {}", args.log_max_size_bytes);
    println!("log_retention = {}", args.log_retention);
    println!("daemon = {}", args.daemon);
    println!(
        "pid_file = {}",
        args.pid_file
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "none".to_owned())
    );
    match auth {
        None => println!("auth = \"none\""),
        Some(auth) if auth.username.is_empty() => {
            println!("auth = \"password-only\"");
            println!("password_hash = \"[redacted]\"");
        }
        Some(_) => {
            println!("auth = \"username-password\"");
            println!("username = \"[configured]\"");
            println!("password_hash = \"[redacted]\"");
        }
    }
}

fn build_auth_verifier(config: Option<&AuthFileConfig>) -> Result<Option<AuthVerifier>> {
    let Some(config) = config else {
        return Ok(None);
    };
    let username = config.username.clone().unwrap_or_default();
    if username.len() > 255 {
        bail!("auth.username must be at most 255 UTF-8 bytes");
    }
    PasswordHash::new(&config.password_hash).map_err(|error| {
        anyhow::anyhow!("auth.password_hash is not a valid PHC password hash: {error}")
    })?;
    let private_key = RsaPrivateKey::new(&mut OsRng, RA2_RSA_BITS)
        .context("could not generate the RA2ne server key")?;
    let public_key_fingerprint = ra2_public_key_fingerprint(&private_key)?;
    Ok(Some(AuthVerifier {
        username,
        password_hash: config.password_hash.clone(),
        private_key: Arc::new(private_key),
        public_key_fingerprint,
    }))
}

fn ra2_public_key_fingerprint(private_key: &RsaPrivateKey) -> Result<String> {
    let public_key = RsaPublicKey::from(private_key);
    let bytes = RA2_RSA_BITS / 8;
    let mut wire = Vec::with_capacity(4 + bytes * 2);
    wire.extend_from_slice(&(RA2_RSA_BITS as u32).to_be_bytes());
    wire.extend_from_slice(&unsigned_fixed(public_key.n(), bytes)?);
    wire.extend_from_slice(&unsigned_fixed(public_key.e(), bytes)?);
    Ok(sha256_fingerprint(&wire))
}

fn sha256_fingerprint(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn print_password_hash() -> Result<()> {
    let password = rpassword::prompt_password("Password: ").context("could not read password")?;
    let confirmation = rpassword::prompt_password("Confirm password: ")
        .context("could not read password confirmation")?;
    if password != confirmation {
        bail!("password confirmation does not match");
    }
    if password.is_empty() {
        bail!("password must not be empty");
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| anyhow::anyhow!("could not generate Argon2id password hash: {error}"))?
        .to_string();
    println!("password_hash = {hash:?}");
    Ok(())
}

fn detach_if_requested(args: &Args) -> Result<()> {
    if !args.daemon {
        return Ok(());
    }

    #[cfg(unix)]
    {
        const DAEMON_REEXEC_ENV: &str = "KVNC_INTERNAL_DAEMON_REEXEC";
        // `#[tokio::main]` has already created its runtime when this function
        // runs. A process that forks after a multi-threaded runtime exists
        // must not continue using that inherited runtime. The daemon child
        // therefore execs the same binary once, retaining its PID while
        // starting with a fresh runtime.
        if std::env::var_os(DAEMON_REEXEC_ENV).as_deref() == Some("1".as_ref()) {
            return Ok(());
        }

        let working_directory =
            std::env::current_dir().context("could not determine working directory")?;
        let executable =
            std::env::current_exe().context("could not determine the kVNC executable")?;
        let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
        let mut daemon = Daemonize::new()
            .working_directory(working_directory)
            .umask(0o027);
        if let Some(pid_file) = &args.pid_file {
            daemon = daemon.pid_file(pid_file);
        }
        daemon
            .start()
            .context("could not detach kVNC into the background")?;
        let error = Command::new(executable)
            .args(arguments)
            .env(DAEMON_REEXEC_ENV, "1")
            .exec();
        Err(error).context("could not re-exec the detached kVNC process")
    }

    #[cfg(not(unix))]
    {
        bail!(
            "--daemon is currently supported on macOS and Linux; use the Windows service manager instead"
        )
    }
}

/// Request a short, platform-native display wake. This is deliberately
/// invoked after a complete RFB handshake so unauthenticated TCP probes cannot
/// wake the local display.
#[cfg(not(target_os = "macos"))]
fn request_display_wake(logger: &Logger, peer: SocketAddr) {
    #[cfg(target_os = "windows")]
    request_windows_display_wake(logger, peer);
    #[cfg(target_os = "linux")]
    request_linux_display_wake(logger, peer);
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    logger.error(format!(
        "display wake on connect is not implemented for this platform: peer={peer}"
    ));
}

#[cfg(target_os = "macos")]
struct MacosDisplayAssertion {
    child: std::process::Child,
    logger: Logger,
    peer: SocketAddr,
}

#[cfg(target_os = "macos")]
impl Drop for MacosDisplayAssertion {
    fn drop(&mut self) {
        match self.child.try_wait() {
            Ok(Some(status)) if !status.success() => self.logger.error(format!(
                "macOS active-client display assertion ended unsuccessfully: peer={}; status={status}",
                self.peer
            )),
            Ok(Some(_)) => {}
            Ok(None) => {
                if let Err(error) = self.child.kill() {
                    self.logger.error(format!(
                        "could not release macOS active-client display assertion: peer={}; {error}",
                        self.peer
                    ));
                }
                if let Err(error) = self.child.wait() {
                    self.logger.error(format!(
                        "could not wait for macOS active-client display assertion: peer={}; {error}",
                        self.peer
                    ));
                }
            }
            Err(error) => self.logger.error(format!(
                "could not inspect macOS active-client display assertion: peer={}; {error}",
                self.peer
            )),
        }
        self.logger.info(format!(
            "released macOS active-client display assertion: peer={}",
            self.peer
        ));
    }
}

/// Reproduce the documented manual macOS recovery sequence: finish the short
/// user-activity wake before retaining the display assertion or processing
/// framebuffer requests. Starting `-d` concurrently with `-u` can leave the
/// window-server capture surface at its pre-wake contents after display sleep.
#[cfg(target_os = "macos")]
async fn acquire_macos_display_assertion(
    logger: &Logger,
    peer: SocketAddr,
) -> Option<MacosDisplayAssertion> {
    logger.info(format!(
        "requested macOS display wake: peer={peer}; duration_seconds=5; waiting_before_framebuffer=true"
    ));
    let wake_result = tokio::task::spawn_blocking(|| {
        Command::new("/usr/bin/caffeinate")
            .args(MACOS_DISPLAY_WAKE_ARGUMENTS)
            .status()
    })
    .await;
    match wake_result {
        Ok(Ok(status)) if status.success() => logger.info(format!(
            "macOS display wake completed before framebuffer processing: peer={peer}"
        )),
        Ok(Ok(status)) => {
            logger.error(format!(
                "macOS display wake command ended unsuccessfully: peer={peer}; status={status}"
            ));
            return None;
        }
        Ok(Err(error)) => {
            logger.error(format!(
                "could not run macOS display wake command: peer={peer}; {error}"
            ));
            return None;
        }
        Err(error) => {
            logger.error(format!(
                "macOS display wake task did not complete: peer={peer}; {error}"
            ));
            return None;
        }
    }

    match Command::new("/usr/bin/caffeinate")
        .args(MACOS_DISPLAY_ASSERTION_ARGUMENTS)
        .spawn()
    {
        Ok(child) => {
            logger.info(format!(
                "acquired macOS active-client display assertion: peer={peer}; release=client-disconnect"
            ));
            Some(MacosDisplayAssertion {
                child,
                logger: logger.clone(),
                peer,
            })
        }
        Err(error) => {
            logger.error(format!(
                "could not acquire macOS active-client display assertion: peer={peer}; {error}"
            ));
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn request_windows_display_wake(logger: &Logger, peer: SocketAddr) {
    use windows_sys::Win32::System::Power::{
        ES_CONTINUOUS, ES_DISPLAY_REQUIRED, SetThreadExecutionState,
    };

    let completion_logger = logger.clone();
    std::thread::spawn(move || {
        // Execution state belongs to this dedicated thread. It is released
        // explicitly after five seconds and also if this helper thread exits.
        // SAFETY: SetThreadExecutionState accepts only the documented bitflags
        // below and does not dereference a caller-provided pointer.
        let enabled = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED) };
        if enabled == 0 {
            completion_logger.error(format!(
                "could not request Windows display wake: peer={peer}; SetThreadExecutionState failed"
            ));
            return;
        }
        completion_logger.info(format!(
            "requested Windows display wake: peer={peer}; duration_seconds=5"
        ));
        std::thread::sleep(DISPLAY_WAKE_DURATION);
        // SAFETY: as above, this clears the execution request on the same
        // dedicated thread with the documented ES_CONTINUOUS flag.
        let cleared = unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        if cleared == 0 {
            completion_logger.error(format!(
                "could not clear Windows display wake request: peer={peer}; SetThreadExecutionState failed"
            ));
        }
    });
}

#[cfg(target_os = "linux")]
fn request_linux_display_wake(logger: &Logger, peer: SocketAddr) {
    let xdg_session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let session = linux_display_session(
        xdg_session_type.as_deref(),
        std::env::var_os("DISPLAY").is_some(),
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
    );
    match session {
        LinuxDisplaySession::X11 => request_linux_x11_display_wake(logger, peer),
        LinuxDisplaySession::Wayland => request_linux_wayland_idle_inhibition(logger, peer),
        LinuxDisplaySession::Unsupported => logger.error(format!(
            "could not request Linux display wake: peer={peer}; no X11 DISPLAY or Wayland session was detected"
        )),
    }
}

#[cfg(target_os = "linux")]
fn request_linux_x11_display_wake(logger: &Logger, peer: SocketAddr) {
    logger.info(format!(
        "requesting Linux X11 display wake: peer={peer}; methods=xset-screen-saver-reset,xset-dpms-on"
    ));
    let completion_logger = logger.clone();
    std::thread::spawn(move || {
        run_linux_xset(
            &completion_logger,
            peer,
            ["s", "reset"],
            "screen-saver reset",
        );
        run_linux_xset(&completion_logger, peer, ["dpms", "force", "on"], "DPMS on");
        match hold_linux_session_idle_inhibition() {
            Ok(()) => completion_logger.debug(format!(
                "Linux X11 session-idle inhibition completed: peer={peer}; duration_seconds=5"
            )),
            Err(error) => completion_logger.debug(format!(
                "Linux X11 session-idle inhibition unavailable: peer={peer}; {error:#}"
            )),
        }
    });
}

#[cfg(target_os = "linux")]
fn run_linux_xset<const N: usize>(
    logger: &Logger,
    peer: SocketAddr,
    arguments: [&str; N],
    action: &str,
) {
    match Command::new("xset").args(arguments).status() {
        Ok(status) if status.success() => logger.info(format!(
            "requested Linux X11 display wake succeeded: peer={peer}; action={action}"
        )),
        Ok(status) => logger.error(format!(
            "requested Linux X11 display wake failed: peer={peer}; action={action}; status={status}"
        )),
        Err(error) => logger.error(format!(
            "could not run xset for Linux X11 display wake: peer={peer}; action={action}; {error}"
        )),
    }
}

#[cfg(target_os = "linux")]
fn request_linux_wayland_idle_inhibition(logger: &Logger, peer: SocketAddr) {
    logger.info(format!(
        "requesting Linux Wayland display wake through session-idle inhibition: peer={peer}; duration_seconds=5"
    ));
    let completion_logger = logger.clone();
    std::thread::spawn(move || match hold_linux_session_idle_inhibition() {
        Ok(()) => completion_logger.info(format!(
            "Linux Wayland session-idle inhibition completed: peer={peer}; duration_seconds=5"
        )),
        Err(error) => completion_logger.error(format!(
            "could not request Linux Wayland display wake: peer={peer}; {error:#}"
        )),
    });
}

#[cfg(target_os = "linux")]
fn hold_linux_session_idle_inhibition() -> Result<()> {
    const DESTINATION: &str = "org.freedesktop.ScreenSaver";
    const PATH: &str = "/org/freedesktop/ScreenSaver";
    const INTERFACE: &str = "org.freedesktop.ScreenSaver";

    let connection = zbus::blocking::Connection::session()
        .context("could not connect to the graphical-session D-Bus")?;
    let reply = connection
        .call_method(
            Some(DESTINATION),
            PATH,
            Some(INTERFACE),
            "Inhibit",
            &("kVNC", "A trusted VNC client connected"),
        )
        .context("the graphical session does not provide ScreenSaver.Inhibit")?;
    let cookie: u32 = reply
        .body()
        .deserialize()
        .context("ScreenSaver.Inhibit returned an invalid cookie")?;
    std::thread::sleep(DISPLAY_WAKE_DURATION);
    connection
        .call_method(
            Some(DESTINATION),
            PATH,
            Some(INTERFACE),
            "UnInhibit",
            &cookie,
        )
        .context("could not release the Wayland session-idle inhibition")?;
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Per-client state is assembled at accept time.
async fn serve_client(
    mut stream: TcpStream,
    peer: SocketAddr,
    framebuffer: Arc<RwLock<Frame>>,
    mut clipboard_rx: broadcast::Receiver<String>,
    clipboard_from_client_tx: mpsc::UnboundedSender<String>,
    clipboard_state: Arc<RwLock<String>>,
    logger: Logger,
    input: Option<Arc<StdMutex<InputController>>>,
    auth: Option<AuthVerifier>,
    wake_display_on_connect: bool,
    minimum_frame_interval: Duration,
) -> Result<()> {
    let initial_frame = framebuffer.read().await.clone();
    handshake(&mut stream, &initial_frame, &logger, auth.as_ref(), peer).await?;
    #[cfg(target_os = "macos")]
    let _macos_display_assertion = if wake_display_on_connect {
        acquire_macos_display_assertion(&logger, peer).await
    } else {
        None
    };
    #[cfg(not(target_os = "macos"))]
    if wake_display_on_connect {
        request_display_wake(&logger, peer);
    }
    logger.info(format!("client connected: peer={peer}"));
    let (mut reader, writer) = stream.into_split();
    let writer = Arc::new(Mutex::new(writer));
    let pixel_format = Arc::new(RwLock::new(SERVER_PIXEL_FORMAT));
    let framebuffer_encoding = Arc::new(RwLock::new(FramebufferEncoding::Raw));
    let mut extended_clipboard_offered = false;
    let mut extended_clipboard_active = false;
    // Raw desktop frames can be tens of megabytes.  Send them from a
    // dedicated task so keyboard and pointer messages are not blocked by a
    // slow framebuffer write.  The one-item queue coalesces repeated
    // incremental update requests instead of accumulating a write backlog.
    let (framebuffer_update_tx, mut framebuffer_update_rx) = mpsc::channel::<bool>(1);
    let framebuffer_for_updates = Arc::clone(&framebuffer);
    let writer_for_updates = Arc::clone(&writer);
    let pixel_format_for_updates = Arc::clone(&pixel_format);
    let framebuffer_encoding_for_updates = Arc::clone(&framebuffer_encoding);
    let update_logger = logger.clone();
    tokio::spawn(async move {
        let mut last_update = None::<Instant>;
        let mut last_sent_frame = None::<Frame>;
        while let Some(incremental) = framebuffer_update_rx.recv().await {
            if incremental
                && last_update.is_some_and(|last| last.elapsed() < minimum_frame_interval)
            {
                if let Err(error) = write_empty_framebuffer_update(&writer_for_updates).await {
                    update_logger.error(format!(
                        "empty framebuffer update failed: peer={peer}; {error:#}"
                    ));
                    break;
                }
                continue;
            }
            let frame = framebuffer_for_updates.read().await.clone();
            let pixel_format = *pixel_format_for_updates.read().await;
            let framebuffer_encoding = *framebuffer_encoding_for_updates.read().await;
            let result = if incremental {
                match last_sent_frame.as_ref() {
                    Some(previous) => {
                        write_framebuffer_delta(
                            &writer_for_updates,
                            previous,
                            &frame,
                            pixel_format,
                            framebuffer_encoding,
                            &update_logger,
                        )
                        .await
                    }
                    None => {
                        write_framebuffer_update(
                            &writer_for_updates,
                            &frame,
                            pixel_format,
                            framebuffer_encoding,
                            &update_logger,
                        )
                        .await
                    }
                }
            } else {
                write_framebuffer_update(
                    &writer_for_updates,
                    &frame,
                    pixel_format,
                    framebuffer_encoding,
                    &update_logger,
                )
                .await
            };
            if let Err(error) = result {
                update_logger.error(format!("framebuffer update failed: peer={peer}; {error:#}"));
                break;
            }
            last_sent_frame = Some(frame);
            last_update = Some(Instant::now());
        }
    });
    let mut pointer_mask = 0_u8;
    // Keep the display selected at button-down while dragging across a gap
    // between monitors. This lets the matching button-up reach the same OS
    // desktop even when the RFB pointer briefly traverses unused canvas.
    let mut active_pointer_region = None::<InputRegion>;
    loop {
        tokio::select! {
            message = read_client_message(&mut reader, &logger) => match message? {
                ClientMessage::SetPixelFormat(format) => {
                    *pixel_format.write().await = format;
                }
                ClientMessage::SetEncodings(encodings) => {
                    let encoding = if encodings.contains(&RFB_ENCODING_HEXTILE) {
                        FramebufferEncoding::Hextile
                    } else {
                        FramebufferEncoding::Raw
                    };
                    *framebuffer_encoding.write().await = encoding;
                    logger.debug(format!("framebuffer encoding selected: peer={peer} encoding={encoding:?}"));
                    if encodings.contains(&RFB_ENCODING_EXTENDED_CLIPBOARD) && !extended_clipboard_offered {
                        write_extended_clipboard_caps(&writer).await?;
                        extended_clipboard_offered = true;
                        logger.debug(format!("extended clipboard capabilities sent: peer={peer}"));
                    }
                }
                ClientMessage::FramebufferUpdateRequest { incremental } => {
                    if framebuffer_update_tx.try_send(incremental).is_err() {
                        logger.debug(format!("framebuffer update coalesced: peer={peer}"));
                    }
                }
                ClientMessage::CutText(text) => {
                    logger.debug(format!("clipboard received: peer={peer}; bytes={}", text.len()));
                    let _ = clipboard_from_client_tx.send(text);
                }
                ClientMessage::ExtendedClipboard(message) => match message {
                    ExtendedClipboardMessage::Capabilities { actions, formats } => {
                        extended_clipboard_active = formats & EXTENDED_CLIPBOARD_TEXT != 0
                            && actions & EXTENDED_CLIPBOARD_NOTIFY != 0
                            && actions & EXTENDED_CLIPBOARD_REQUEST != 0
                            && actions & EXTENDED_CLIPBOARD_PROVIDE != 0;
                        logger.debug(format!("extended clipboard capabilities received: peer={peer} active={extended_clipboard_active} actions=0x{actions:08x} formats=0x{formats:04x}"));
                        if extended_clipboard_active {
                            let text = clipboard_state.read().await.clone();
                            write_extended_clipboard_notify(&writer, !text.is_empty()).await?;
                        }
                    }
                    ExtendedClipboardMessage::Notify { formats } => {
                        if extended_clipboard_active && formats & EXTENDED_CLIPBOARD_TEXT != 0 {
                            write_extended_clipboard_request(&writer).await?;
                        }
                    }
                    ExtendedClipboardMessage::Request { formats } => {
                        if extended_clipboard_active && formats & EXTENDED_CLIPBOARD_TEXT != 0 {
                            let text = clipboard_state.read().await.clone();
                            write_extended_clipboard_provide(&writer, &text).await?;
                        }
                    }
                    ExtendedClipboardMessage::Provide(text) => {
                        logger.debug(format!("extended clipboard received: peer={peer}; bytes={}", text.len()));
                        let _ = clipboard_from_client_tx.send(text);
                    }
                },
                ClientMessage::KeyEvent { down, keysym } => {
                    if let Some(input) = &input {
                        match input.lock() {
                            Ok(mut input) => match input.key(down, keysym) {
                                Ok(()) => logger.debug(format!("key event injected: peer={peer} keysym=0x{keysym:08x} down={down}")),
                                Err(error) => logger.error(format!("could not inject key event: {error:#}")),
                            },
                            Err(_) => logger.error("input controller lock poisoned"),
                        }
                    }
                }
                ClientMessage::PointerEvent { mask, x, y } => {
                    if let Some(input) = &input {
                        // The display wake request runs after handshake. Use
                        // the frame captured most recently instead of the
                        // handshake frame so display reconfiguration cannot
                        // leave pointer injection on stale monitor geometry.
                        let current_frame = framebuffer.read().await;
                        let region = current_frame
                            .input_region(x, y)
                            .cloned()
                            .or_else(|| {
                                (pointer_mask != 0)
                                    .then(|| active_pointer_region.clone())
                                    .flatten()
                            });
                        drop(current_frame);
                        if let Some(region) = region {
                            let local_x = x - region.rfb_x;
                            let local_y = y - region.rfb_y;
                            match input.lock() {
                                Ok(mut input) => match input.pointer(mask, pointer_mask, (local_x, local_y), (region.origin_x, region.origin_y), region.scale_factor) {
                                    Ok((input_x, input_y)) => logger.debug(format!("pointer event injected: peer={peer} monitor_index={} previous_mask=0x{pointer_mask:02x} mask=0x{mask:02x} rfb_x={x} rfb_y={y} monitor_rfb_x={local_x} monitor_rfb_y={local_y} input_x={input_x} input_y={input_y} frame_origin_x={} frame_origin_y={} input_scale={}", region.monitor_index, region.origin_x, region.origin_y, region.scale_factor)),
                                    Err(error) => logger.error(format!("could not inject pointer event: {error:#}")),
                                },
                                Err(_) => logger.error("input controller lock poisoned"),
                            }
                            if mask & 7 != 0 {
                                active_pointer_region = Some(region);
                            }
                        } else {
                            logger.debug(format!("pointer event ignored outside virtual desktop: peer={peer} mask=0x{mask:02x} rfb_x={x} rfb_y={y}"));
                        }
                    }
                    pointer_mask = mask & 7;
                    if pointer_mask == 0 {
                        active_pointer_region = None;
                    }
                }
                ClientMessage::Other => {}
            },
            clipboard = clipboard_rx.recv() => match clipboard {
                Ok(text) => {
                    logger.debug(format!("clipboard sent: peer={peer}; bytes={}", text.len()));
                    if extended_clipboard_active {
                        write_extended_clipboard_notify(&writer, !text.is_empty()).await?
                    } else {
                        write_server_cut_text(&writer, &text).await?
                    }
                },
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
        }
    }
}

async fn handshake(
    stream: &mut TcpStream,
    frame: &Frame,
    logger: &Logger,
    auth: Option<&AuthVerifier>,
    peer: SocketAddr,
) -> Result<()> {
    stream.write_all(RFB_VERSION).await?;
    let mut version = [0; 12];
    stream.read_exact(&mut version).await?;
    logger.debug(format!(
        "handshake: client_version={}",
        String::from_utf8_lossy(&version).trim()
    ));
    if &version[..4] != b"RFB " {
        bail!("client did not send an RFB version");
    }
    // RFB 3.3 sends a u32 security type and has no client selection. RFB
    // 3.7+ sends a list and receives the client's selected type.
    let is_rfb_33 = version == *b"RFB 003.003\n";
    let selected_security = if let Some(auth) = auth {
        if is_rfb_33 {
            stream.write_u32(u32::from(RFB_SECURITY_RA2NE)).await?;
        } else {
            stream.write_all(&[1, RFB_SECURITY_RA2NE]).await?;
            if stream.read_u8().await? != RFB_SECURITY_RA2NE {
                bail!("client did not choose RA2ne security");
            }
        }
        logger.debug("handshake: security_type=RA2ne");
        if let Err(error) = negotiate_ra2ne(stream, auth, logger, peer).await {
            logger.debug(format!("handshake: RA2ne authentication failed: {error}"));
            stream.write_u32(1).await?;
            if !is_rfb_33 {
                let reason = b"authentication failed";
                stream.write_u32(u32::try_from(reason.len())?).await?;
                stream.write_all(reason).await?;
            }
            return Err(error);
        }
        RFB_SECURITY_RA2NE
    } else {
        if is_rfb_33 {
            stream.write_u32(u32::from(RFB_SECURITY_NONE)).await?;
        } else {
            stream.write_all(&[1, RFB_SECURITY_NONE]).await?;
            if stream.read_u8().await? != RFB_SECURITY_NONE {
                bail!("client did not choose SecurityType None");
            }
        }
        logger.debug("handshake: security_type=None");
        RFB_SECURITY_NONE
    };
    // RFB 3.3 omits SecurityResult only for SecurityType None. RA2ne has an
    // explicit result for every advertised protocol version.
    if !is_rfb_33 || selected_security != RFB_SECURITY_NONE {
        stream.write_u32(0).await?;
    }
    let shared = stream.read_u8().await?;
    logger.debug(format!("handshake: client_init_shared={shared}"));
    stream.write_u16(frame.width).await?;
    stream.write_u16(frame.height).await?;
    write_pixel_format(stream, SERVER_PIXEL_FORMAT).await?;
    let name = b"kVNC";
    stream.write_u32(u32::try_from(name.len())?).await?;
    stream.write_all(name).await?;
    logger.debug(format!(
        "handshake: server_init={}x{} name=kVNC",
        frame.width, frame.height
    ));
    Ok(())
}

fn unsigned_fixed(value: &rsa::BigUint, bytes: usize) -> Result<Vec<u8>> {
    let encoded = value.to_bytes_be();
    if encoded.len() > bytes {
        bail!("RSA public key component exceeds advertised key length");
    }
    let mut fixed = vec![0; bytes];
    fixed[bytes - encoded.len()..].copy_from_slice(&encoded);
    Ok(fixed)
}

fn increment_counter(counter: &mut [u8; 16]) {
    for byte in counter {
        let (next, overflow) = byte.overflowing_add(1);
        *byte = next;
        if !overflow {
            break;
        }
    }
}

struct Ra2Cipher {
    cipher: Eax<Aes128>,
    counter: [u8; 16],
}

impl Ra2Cipher {
    fn new(key: &[u8]) -> Result<Self> {
        if key.len() != 16 {
            bail!("RA2 AES session key has invalid length");
        }
        Ok(Self {
            cipher: Eax::<Aes128>::new_from_slice(key).context("could not initialize RA2 AES")?,
            // RA2 specifies a shared, zero-valued initial counter; the peer
            // derives the same counter and no nonce is transmitted on the wire.
            counter: Default::default(),
        })
    }

    fn encrypt(&mut self, message: &[u8]) -> Result<Vec<u8>> {
        let length = u16::try_from(message.len()).context("RA2 message is too long")?;
        let aad = length.to_be_bytes();
        let nonce = GenericArray::from_slice(&self.counter);
        let encrypted = self
            .cipher
            .encrypt(
                nonce,
                Payload {
                    msg: message,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow::anyhow!("could not encrypt RA2 message"))?;
        increment_counter(&mut self.counter);
        let mut packet = Vec::with_capacity(2 + encrypted.len());
        packet.extend_from_slice(&aad);
        packet.extend_from_slice(&encrypted);
        Ok(packet)
    }

    fn decrypt(&mut self, length: u16, encrypted: &[u8]) -> Result<Vec<u8>> {
        if encrypted.len() != usize::from(length) + 16 {
            bail!("RA2 encrypted message has invalid length");
        }
        let aad = length.to_be_bytes();
        let nonce = GenericArray::from_slice(&self.counter);
        let plain = self
            .cipher
            .decrypt(
                nonce,
                Payload {
                    msg: encrypted,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow::anyhow!("could not authenticate RA2 message"))?;
        increment_counter(&mut self.counter);
        Ok(plain)
    }
}

async fn read_ra2_message(stream: &mut TcpStream, cipher: &mut Ra2Cipher) -> Result<Vec<u8>> {
    let length = stream.read_u16().await?;
    let mut encrypted = vec![0; usize::from(length) + 16];
    stream.read_exact(&mut encrypted).await?;
    cipher.decrypt(length, &encrypted)
}

async fn write_ra2_message(
    stream: &mut TcpStream,
    cipher: &mut Ra2Cipher,
    message: &[u8],
) -> Result<()> {
    stream.write_all(&cipher.encrypt(message)?).await?;
    Ok(())
}

async fn negotiate_ra2ne(
    stream: &mut TcpStream,
    auth: &AuthVerifier,
    logger: &Logger,
    peer: SocketAddr,
) -> Result<()> {
    logger.debug(format!(
        "RA2ne: peer={peer} phase=server-public-key-send fingerprint_sha256={}",
        auth.public_key_fingerprint,
    ));
    let server_public = RsaPublicKey::from(auth.private_key.as_ref());
    let server_bytes = RA2_RSA_BITS / 8;
    let server_n = unsigned_fixed(server_public.n(), server_bytes)?;
    let server_e = unsigned_fixed(server_public.e(), server_bytes)?;
    let mut server_public_wire = Vec::with_capacity(4 + server_bytes * 2);
    server_public_wire.extend_from_slice(&(RA2_RSA_BITS as u32).to_be_bytes());
    server_public_wire.extend_from_slice(&server_n);
    server_public_wire.extend_from_slice(&server_e);
    stream.write_all(&server_public_wire).await?;

    let client_bits =
        usize::try_from(stream.read_u32().await?).context("invalid RA2 client key length")?;
    if !(1024..=8192).contains(&client_bits) || client_bits % 8 != 0 {
        bail!("RA2 client public key has unsupported length");
    }
    let client_bytes = client_bits / 8;
    let mut client_n = vec![0; client_bytes];
    let mut client_e = vec![0; client_bytes];
    stream.read_exact(&mut client_n).await?;
    stream.read_exact(&mut client_e).await?;
    logger.debug(format!(
        "RA2ne: received client public key bits={client_bits} modulus_bytes={} exponent_bytes={}",
        client_n.len(),
        client_e.len()
    ));
    let client_public = RsaPublicKey::new(
        rsa::BigUint::from_bytes_be(&client_n),
        rsa::BigUint::from_bytes_be(&client_e),
    )
    .context("RA2 client public key is invalid")?;
    let mut client_public_wire = Vec::with_capacity(4 + client_bytes * 2);
    client_public_wire.extend_from_slice(&(client_bits as u32).to_be_bytes());
    client_public_wire.extend_from_slice(&client_n);
    client_public_wire.extend_from_slice(&client_e);
    logger.debug(format!(
        "RA2ne: peer={peer} direction=client-to-server packet=client-public-key bytes={} sha256={}",
        client_public_wire.len(),
        sha256_fingerprint(&client_public_wire)
    ));

    let encrypted_client_random_len = usize::from(stream.read_u16().await?);
    if encrypted_client_random_len != server_bytes {
        bail!("RA2 encrypted client random has invalid length");
    }
    let mut encrypted_client_random = vec![0; encrypted_client_random_len];
    stream.read_exact(&mut encrypted_client_random).await?;
    logger.debug(format!(
        "RA2ne: received encrypted client random bytes={encrypted_client_random_len}"
    ));
    let client_random = auth
        .private_key
        .decrypt(Pkcs1v15Encrypt, &encrypted_client_random)
        .context("could not decrypt RA2 client random")?;
    if client_random.len() != 16 {
        bail!("RA2 client random has invalid length");
    }
    logger.debug(format!(
        "RA2ne: peer={peer} phase=client-random-decrypted fingerprint_sha256={}",
        auth.public_key_fingerprint,
    ));
    let mut server_random = [0; 16];
    OsRng.fill_bytes(&mut server_random);
    let encrypted_server_random = client_public
        .encrypt(&mut OsRng, Pkcs1v15Encrypt, &server_random)
        .context("could not encrypt RA2 server random")?;
    if encrypted_server_random.len() != client_bytes {
        bail!("RA2 encrypted server random has invalid length");
    }
    stream.write_u16(u16::try_from(client_bytes)?).await?;
    stream.write_all(&encrypted_server_random).await?;
    logger.debug(format!(
        "RA2ne: peer={peer} direction=server-to-client packet=server-encrypted-random bytes={} sha256={}",
        encrypted_server_random.len(),
        sha256_fingerprint(&encrypted_server_random)
    ));

    let mut client_material = Vec::with_capacity(32);
    client_material.extend_from_slice(&server_random);
    client_material.extend_from_slice(&client_random);
    let mut server_material = Vec::with_capacity(32);
    server_material.extend_from_slice(&client_random);
    server_material.extend_from_slice(&server_random);
    let client_key = Sha1::digest(&client_material);
    let server_key = Sha1::digest(&server_material);
    let mut client_cipher = Ra2Cipher::new(&client_key[..16])?;
    let mut server_cipher = Ra2Cipher::new(&server_key[..16])?;

    let mut client_hash_material = client_public_wire.clone();
    client_hash_material.extend_from_slice(&server_public_wire);
    let expected_client_hash = Sha1::digest(&client_hash_material);
    let received_client_hash = read_ra2_message(stream, &mut client_cipher).await?;
    if received_client_hash.as_slice() != expected_client_hash.as_slice() {
        bail!("RA2 client public-key verification failed");
    }
    let mut server_hash_material = server_public_wire;
    server_hash_material.extend_from_slice(&client_public_wire);
    let server_hash = Sha1::digest(&server_hash_material);
    write_ra2_message(stream, &mut server_cipher, &server_hash).await?;
    // RA2ne subtype 1 requests username and password; subtype 2 requests a
    // password only.  This lets a password-only configuration present the
    // same credential UI as password-only VNC servers.
    let credential_subtype = if auth.username.is_empty() { 2 } else { 1 };
    write_ra2_message(stream, &mut server_cipher, &[credential_subtype]).await?;
    let credentials = read_ra2_message(stream, &mut client_cipher).await?;
    if credentials.len() < 2 {
        bail!("RA2 credentials are malformed");
    }
    let username_len = usize::from(credentials[0]);
    if credentials.len() < username_len + 2 {
        bail!("RA2 username is truncated");
    }
    let password_len = usize::from(credentials[username_len + 1]);
    if credentials.len() != username_len + password_len + 2 {
        bail!("RA2 password is truncated");
    }
    let username = std::str::from_utf8(&credentials[1..1 + username_len])
        .context("RA2 username is not UTF-8")?;
    let password = &credentials[username_len + 2..];
    let parsed_hash = PasswordHash::new(&auth.password_hash)
        .map_err(|error| anyhow::anyhow!("configured password hash is invalid: {error}"))?;
    if username != auth.username
        || Argon2::default()
            .verify_password(password, &parsed_hash)
            .is_err()
    {
        bail!("RA2 authentication failed");
    }
    logger.debug(format!(
        "RA2ne: peer={peer} phase=authentication-complete fingerprint_sha256={}",
        auth.public_key_fingerprint,
    ));
    Ok(())
}

async fn write_pixel_format(
    writer: &mut (impl AsyncWrite + Unpin),
    format: PixelFormat,
) -> Result<()> {
    writer
        .write_all(&[
            format.bits_per_pixel,
            format.depth,
            u8::from(format.big_endian),
            u8::from(format.true_color),
        ])
        .await?;
    writer.write_u16(format.red_max).await?;
    writer.write_u16(format.green_max).await?;
    writer.write_u16(format.blue_max).await?;
    writer
        .write_all(&[
            format.red_shift,
            format.green_shift,
            format.blue_shift,
            0,
            0,
            0,
        ])
        .await?;
    Ok(())
}

fn validate_pixel_format(format: PixelFormat) -> Result<()> {
    if !format.true_color {
        bail!("indexed client pixel formats are not supported");
    }
    if !matches!(format.bits_per_pixel, 8 | 16 | 32) {
        bail!(
            "unsupported client bits-per-pixel: {}",
            format.bits_per_pixel
        );
    }
    let bits = u16::from(format.bits_per_pixel);
    for (name, maximum, shift) in [
        ("red", format.red_max, format.red_shift),
        ("green", format.green_max, format.green_shift),
        ("blue", format.blue_max, format.blue_shift),
    ] {
        if maximum == 0 || u16::from(shift) >= bits {
            bail!("invalid {name} channel in client pixel format");
        }
    }
    Ok(())
}

enum ClientMessage {
    SetPixelFormat(PixelFormat),
    SetEncodings(Vec<i32>),
    FramebufferUpdateRequest { incremental: bool },
    CutText(String),
    ExtendedClipboard(ExtendedClipboardMessage),
    KeyEvent { down: bool, keysym: u32 },
    PointerEvent { mask: u8, x: u16, y: u16 },
    Other,
}

enum ExtendedClipboardMessage {
    Capabilities { actions: u32, formats: u32 },
    Request { formats: u32 },
    Notify { formats: u32 },
    Provide(String),
}

async fn read_client_message(
    reader: &mut (impl AsyncRead + Unpin),
    logger: &Logger,
) -> Result<ClientMessage> {
    match reader.read_u8().await? {
        0 => {
            let mut rest = [0; 19];
            reader.read_exact(&mut rest).await?;
            let format = PixelFormat {
                bits_per_pixel: rest[3],
                depth: rest[4],
                big_endian: rest[5] != 0,
                true_color: rest[6] != 0,
                red_max: u16::from_be_bytes([rest[7], rest[8]]),
                green_max: u16::from_be_bytes([rest[9], rest[10]]),
                blue_max: u16::from_be_bytes([rest[11], rest[12]]),
                red_shift: rest[13],
                green_shift: rest[14],
                blue_shift: rest[15],
            };
            validate_pixel_format(format)?;
            logger.debug(format!("client message: SetPixelFormat {format:?}"));
            Ok(ClientMessage::SetPixelFormat(format))
        }
        2 => {
            reader.read_u8().await?;
            let count = usize::from(reader.read_u16().await?);
            let mut encodings = vec![0; count.checked_mul(4).context("encoding list too large")?];
            reader.read_exact(&mut encodings).await?;
            let (encoding_chunks, remainder) = encodings.as_chunks::<4>();
            debug_assert!(remainder.is_empty());
            let encodings: Vec<i32> = encoding_chunks
                .iter()
                .map(|encoding| i32::from_be_bytes(*encoding))
                .collect();
            logger.debug(format!(
                "client message: SetEncodings count={count} values={encodings:?}"
            ));
            Ok(ClientMessage::SetEncodings(encodings))
        }
        3 => {
            let mut rest = [0; 9];
            reader.read_exact(&mut rest).await?;
            logger.debug(format!("client message: FramebufferUpdateRequest incremental={} x={} y={} width={} height={}", rest[0], u16::from_be_bytes([rest[1], rest[2]]), u16::from_be_bytes([rest[3], rest[4]]), u16::from_be_bytes([rest[5], rest[6]]), u16::from_be_bytes([rest[7], rest[8]])));
            Ok(ClientMessage::FramebufferUpdateRequest {
                incremental: rest[0] != 0,
            })
        }
        4 => {
            let mut rest = [0; 7];
            reader.read_exact(&mut rest).await?;
            let down = rest[0] != 0;
            let keysym = u32::from_be_bytes(rest[3..7].try_into().expect("four-byte keysym"));
            logger.debug(format!(
                "client message: KeyEvent down={down} keysym=0x{keysym:08x}"
            ));
            Ok(ClientMessage::KeyEvent { down, keysym })
        }
        5 => {
            let mut rest = [0; 5];
            reader.read_exact(&mut rest).await?;
            let mask = rest[0];
            let x = u16::from_be_bytes([rest[1], rest[2]]);
            let y = u16::from_be_bytes([rest[3], rest[4]]);
            logger.debug(format!(
                "client message: PointerEvent mask=0x{mask:02x} x={x} y={y}"
            ));
            Ok(ClientMessage::PointerEvent { mask, x, y })
        }
        6 => {
            let mut padding = [0; 3];
            reader.read_exact(&mut padding).await?;
            let length = reader.read_i32().await?;
            if length >= 0 {
                let length = usize::try_from(length).context("clipboard length too large")?;
                if length > MAX_CLIPBOARD_BYTES {
                    bail!("clipboard exceeds {MAX_CLIPBOARD_BYTES} bytes");
                }
                let mut bytes = vec![0; length];
                reader.read_exact(&mut bytes).await?;
                return Ok(ClientMessage::CutText(
                    String::from_utf8(bytes).context("clipboard is not UTF-8")?,
                ));
            }
            let length = usize::try_from(length.unsigned_abs())
                .context("extended clipboard length too large")?;
            if length > MAX_CLIPBOARD_BYTES + 1024 {
                bail!("extended clipboard exceeds allowed size");
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).await?;
            Ok(ClientMessage::ExtendedClipboard(parse_extended_clipboard(
                &bytes,
            )?))
        }
        150 => {
            let mut rest = [0; 9];
            reader.read_exact(&mut rest).await?;
            logger.debug(format!(
                "client message: EnableContinuousUpdates enabled={} x={} y={} width={} height={}",
                rest[0],
                u16::from_be_bytes([rest[1], rest[2]]),
                u16::from_be_bytes([rest[3], rest[4]]),
                u16::from_be_bytes([rest[5], rest[6]]),
                u16::from_be_bytes([rest[7], rest[8]])
            ));
            Ok(ClientMessage::FramebufferUpdateRequest { incremental: true })
        }
        248 => {
            let mut padding = [0; 3];
            reader.read_exact(&mut padding).await?;
            let length =
                usize::try_from(reader.read_u32().await?).context("fence length too large")?;
            if length > MAX_EXTENSION_BYTES {
                bail!("fence exceeds {MAX_EXTENSION_BYTES} bytes");
            }
            let mut payload = vec![0; length];
            reader.read_exact(&mut payload).await?;
            logger.debug(format!(
                "client message: ClientFence bytes={length} (ignored)"
            ));
            Ok(ClientMessage::Other)
        }
        251 => {
            let mut header = [0; 7];
            reader.read_exact(&mut header).await?;
            let screens = usize::from(header[5]);
            let length = screens
                .checked_mul(16)
                .context("desktop-size screen list too large")?;
            if length > MAX_EXTENSION_BYTES {
                bail!("desktop-size screen list exceeds {MAX_EXTENSION_BYTES} bytes");
            }
            let mut screens = vec![0; length];
            reader.read_exact(&mut screens).await?;
            logger.debug(format!(
                "client message: SetDesktopSize {}x{} screens={} (ignored)",
                u16::from_be_bytes([header[1], header[2]]),
                u16::from_be_bytes([header[3], header[4]]),
                header[5]
            ));
            Ok(ClientMessage::Other)
        }
        message_type => {
            logger.error(format!(
                "unsupported RFB client message type {message_type}; closing connection"
            ));
            bail!("unsupported RFB client message type {message_type}")
        }
    }
}

async fn write_framebuffer_update(
    writer: &Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    frame: &Frame,
    pixel_format: PixelFormat,
    encoding: FramebufferEncoding,
    logger: &Logger,
) -> Result<()> {
    write_framebuffer_update_at(writer, frame, pixel_format, encoding, 0, 0, logger).await
}

async fn write_framebuffer_delta(
    writer: &Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    previous: &Frame,
    frame: &Frame,
    pixel_format: PixelFormat,
    encoding: FramebufferEncoding,
    logger: &Logger,
) -> Result<()> {
    let Some((x, y, width, height)) = changed_region(previous, frame) else {
        logger.debug("framebuffer delta sent: unchanged");
        return write_empty_framebuffer_update(writer).await;
    };
    let region = frame_region(frame, x, y, width, height);
    write_framebuffer_update_at(writer, &region, pixel_format, encoding, x, y, logger).await
}

async fn write_framebuffer_update_at(
    writer: &Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    frame: &Frame,
    pixel_format: PixelFormat,
    encoding: FramebufferEncoding,
    x: u16,
    y: u16,
    logger: &Logger,
) -> Result<()> {
    let pixels = match encoding {
        FramebufferEncoding::Raw => encode_pixels(frame, pixel_format)?,
        FramebufferEncoding::Hextile => encode_hextile(frame, pixel_format)?,
    };
    let mut writer = writer.lock().await;
    writer.write_all(&[0, 0]).await?;
    writer.write_u16(1).await?;
    writer.write_u16(x).await?;
    writer.write_u16(y).await?;
    writer.write_u16(frame.width).await?;
    writer.write_u16(frame.height).await?;
    writer.write_i32(encoding.rfb_value()).await?;
    writer.write_all(&pixels).await?;
    logger.debug(format!(
        "framebuffer update sent: x={x} y={y} {}x{} encoding={encoding:?} bytes={} format={:?}",
        frame.width,
        frame.height,
        pixels.len(),
        pixel_format
    ));
    Ok(())
}

/// Return the smallest rectangle that includes all pixel changes. Incremental
/// RFB requests can therefore transmit a cursor, typed character, or menu
/// redraw without retransmitting the complete desktop.
fn changed_region(previous: &Frame, current: &Frame) -> Option<(u16, u16, u16, u16)> {
    if previous.width != current.width || previous.height != current.height {
        return Some((0, 0, current.width, current.height));
    }
    let width = usize::from(current.width);
    let height = usize::from(current.height);
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut changed = false;
    let (previous_pixels, previous_remainder) = previous.bgrx.as_chunks::<4>();
    let (current_pixels, current_remainder) = current.bgrx.as_chunks::<4>();
    debug_assert!(previous_remainder.is_empty());
    debug_assert!(current_remainder.is_empty());
    for (index, (before, after)) in previous_pixels.iter().zip(current_pixels).enumerate() {
        if before != after {
            let x = index % width;
            let y = index / width;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            changed = true;
        }
    }
    changed.then(|| {
        (
            min_x as u16,
            min_y as u16,
            (max_x - min_x + 1) as u16,
            (max_y - min_y + 1) as u16,
        )
    })
}

fn frame_region(frame: &Frame, x: u16, y: u16, width: u16, height: u16) -> Frame {
    let frame_width = usize::from(frame.width);
    let x = usize::from(x);
    let y = usize::from(y);
    let width_usize = usize::from(width);
    let height_usize = usize::from(height);
    let mut bgrx = Vec::with_capacity(width_usize * height_usize * 4);
    for row in y..y + height_usize {
        let start = (row * frame_width + x) * 4;
        bgrx.extend_from_slice(&frame.bgrx[start..start + width_usize * 4]);
    }
    Frame {
        width,
        height,
        scale_factor: frame.scale_factor,
        x: frame.x,
        y: frame.y,
        bgrx,
        input_regions: Vec::new(),
    }
}

async fn write_empty_framebuffer_update(
    writer: &Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
) -> Result<()> {
    let mut writer = writer.lock().await;
    writer.write_all(&[0, 0]).await?;
    writer.write_u16(0).await?;
    Ok(())
}

fn encode_pixels(frame: &Frame, format: PixelFormat) -> Result<Vec<u8>> {
    validate_pixel_format(format)?;
    if format == SERVER_PIXEL_FORMAT {
        return Ok(frame.bgrx.clone());
    }
    let bytes_per_pixel = usize::from(format.bits_per_pixel / 8);
    let mut encoded = Vec::with_capacity(frame.bgrx.len() / 4 * bytes_per_pixel);
    let (pixels, remainder) = frame.bgrx.as_chunks::<4>();
    debug_assert!(remainder.is_empty());
    for pixel in pixels {
        let red = u32::from(pixel[2]) * u32::from(format.red_max) / 255;
        let green = u32::from(pixel[1]) * u32::from(format.green_max) / 255;
        let blue = u32::from(pixel[0]) * u32::from(format.blue_max) / 255;
        let value =
            (red << format.red_shift) | (green << format.green_shift) | (blue << format.blue_shift);
        let bytes = if format.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        };
        if format.big_endian {
            encoded.extend_from_slice(&bytes[4 - bytes_per_pixel..]);
        } else {
            encoded.extend_from_slice(&bytes[..bytes_per_pixel]);
        }
    }
    Ok(encoded)
}

/// Encode an RFB Hextile rectangle using independently coloured subrectangles.
///
/// Hextile is part of the base RFB protocol and is advertised by the deployed
/// clients.  Each tile falls back to Raw when its compact representation would
/// be larger, so this preserves every source pixel without lowering quality.
fn encode_hextile(frame: &Frame, format: PixelFormat) -> Result<Vec<u8>> {
    let pixels = encode_pixels(frame, format)?;
    let bytes_per_pixel = usize::from(format.bits_per_pixel / 8);
    let width = usize::from(frame.width);
    let height = usize::from(frame.height);
    let mut output = Vec::with_capacity(pixels.len() / 2);

    for tile_y in (0..height).step_by(16) {
        for tile_x in (0..width).step_by(16) {
            let tile_width = (width - tile_x).min(16);
            let tile_height = (height - tile_y).min(16);
            let mut tile = Vec::with_capacity(tile_width * tile_height * bytes_per_pixel);
            for y in 0..tile_height {
                let start = ((tile_y + y) * width + tile_x) * bytes_per_pixel;
                tile.extend_from_slice(&pixels[start..start + tile_width * bytes_per_pixel]);
            }
            let raw_size = 1 + tile.len();
            let compact = encode_hextile_tile(&tile, tile_width, tile_height, bytes_per_pixel);
            if compact.len() < raw_size {
                output.extend_from_slice(&compact);
            } else {
                output.push(1); // Raw tile.
                output.extend_from_slice(&tile);
            }
        }
    }
    Ok(output)
}

fn encode_hextile_tile(
    tile: &[u8],
    width: usize,
    height: usize,
    bytes_per_pixel: usize,
) -> Vec<u8> {
    let pixel = |x: usize, y: usize| {
        let start = (y * width + x) * bytes_per_pixel;
        &tile[start..start + bytes_per_pixel]
    };
    let mut colours: Vec<(Vec<u8>, usize)> = Vec::new();
    for value in tile.chunks_exact(bytes_per_pixel) {
        if let Some((_, count)) = colours.iter_mut().find(|(colour, _)| colour == value) {
            *count += 1;
        } else {
            colours.push((value.to_vec(), 1));
        }
    }
    let background = &colours
        .iter()
        .max_by_key(|(_, count)| *count)
        .expect("hextile tile has at least one pixel")
        .0;
    let mut used = vec![false; width * height];
    let mut rectangles: Vec<(Vec<u8>, usize, usize, usize, usize)> = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let offset = y * width + x;
            if used[offset] || pixel(x, y) == background.as_slice() {
                continue;
            }
            let colour = pixel(x, y).to_vec();
            let mut rect_width = 1;
            while x + rect_width < width
                && !used[y * width + x + rect_width]
                && pixel(x + rect_width, y) == colour.as_slice()
            {
                rect_width += 1;
            }
            let mut rect_height = 1;
            'extend_height: while y + rect_height < height {
                for dx in 0..rect_width {
                    if used[(y + rect_height) * width + x + dx]
                        || pixel(x + dx, y + rect_height) != colour.as_slice()
                    {
                        break 'extend_height;
                    }
                }
                rect_height += 1;
            }
            for dy in 0..rect_height {
                for dx in 0..rect_width {
                    used[(y + dy) * width + x + dx] = true;
                }
            }
            rectangles.push((colour, x, y, rect_width, rect_height));
            if rectangles.len() > 255 {
                return vec![1; tile.len() + 1]; // Force the caller to choose Raw.
            }
        }
    }

    // BackgroundSpecified | AnySubrects | SubrectsColoured.
    let mut encoded = Vec::with_capacity(2 + tile.len() / 2);
    encoded.push(2 | 8 | 16);
    encoded.extend_from_slice(background);
    encoded.push(rectangles.len() as u8);
    for (colour, x, y, rect_width, rect_height) in rectangles {
        encoded.extend_from_slice(&colour);
        encoded.push(((x as u8) << 4) | y as u8);
        encoded.push((((rect_width - 1) as u8) << 4) | (rect_height - 1) as u8);
    }
    encoded
}

async fn write_server_cut_text(
    writer: &Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    text: &str,
) -> Result<()> {
    let bytes = text.as_bytes();
    if bytes.len() > MAX_CLIPBOARD_BYTES {
        bail!("clipboard exceeds {MAX_CLIPBOARD_BYTES} bytes");
    }
    let mut writer = writer.lock().await;
    writer.write_all(&[3, 0, 0, 0]).await?;
    writer.write_u32(u32::try_from(bytes.len())?).await?;
    writer.write_all(bytes).await?;
    Ok(())
}

fn parse_extended_clipboard(bytes: &[u8]) -> Result<ExtendedClipboardMessage> {
    if bytes.len() < 4 {
        bail!("extended clipboard message is missing flags");
    }
    let flags = u32::from_be_bytes(bytes[..4].try_into().expect("four-byte flags"));
    let formats = flags & 0xffff;
    let actions = flags & 0xff00_0000;
    if actions & EXTENDED_CLIPBOARD_CAPS != 0 {
        return Ok(ExtendedClipboardMessage::Capabilities { actions, formats });
    }
    if actions == EXTENDED_CLIPBOARD_REQUEST {
        return Ok(ExtendedClipboardMessage::Request { formats });
    }
    if actions == EXTENDED_CLIPBOARD_NOTIFY {
        return Ok(ExtendedClipboardMessage::Notify { formats });
    }
    if actions != EXTENDED_CLIPBOARD_PROVIDE || formats & EXTENDED_CLIPBOARD_TEXT == 0 {
        bail!("unsupported extended clipboard action or format");
    }
    let mut decoder = ZlibDecoder::new(&bytes[4..]);
    let mut length = [0_u8; 4];
    decoder
        .read_exact(&mut length)
        .context("extended clipboard text length is invalid")?;
    let length = usize::try_from(u32::from_be_bytes(length))?;
    if length == 0 || length > MAX_CLIPBOARD_BYTES + 1 {
        bail!("extended clipboard text exceeds allowed size");
    }
    let mut text = vec![0; length];
    decoder
        .read_exact(&mut text)
        .context("extended clipboard text is truncated")?;
    if text.last() == Some(&0) {
        text.pop();
    }
    let text = String::from_utf8(text)
        .context("extended clipboard text is not UTF-8")?
        .replace("\r\n", "\n");
    Ok(ExtendedClipboardMessage::Provide(text))
}

async fn write_extended_clipboard_caps(
    writer: &Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
) -> Result<()> {
    let flags = EXTENDED_CLIPBOARD_CAPS
        | EXTENDED_CLIPBOARD_REQUEST
        | EXTENDED_CLIPBOARD_NOTIFY
        | EXTENDED_CLIPBOARD_PROVIDE
        | EXTENDED_CLIPBOARD_TEXT;
    let mut payload = Vec::with_capacity(8);
    payload.extend_from_slice(&flags.to_be_bytes());
    payload.extend_from_slice(&(MAX_CLIPBOARD_BYTES as u32).to_be_bytes());
    write_server_extended_clipboard(writer, &payload).await
}

async fn write_extended_clipboard_notify(
    writer: &Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    has_text: bool,
) -> Result<()> {
    let formats = if has_text { EXTENDED_CLIPBOARD_TEXT } else { 0 };
    write_server_extended_clipboard(writer, &(EXTENDED_CLIPBOARD_NOTIFY | formats).to_be_bytes())
        .await
}

async fn write_extended_clipboard_request(
    writer: &Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
) -> Result<()> {
    write_server_extended_clipboard(
        writer,
        &(EXTENDED_CLIPBOARD_REQUEST | EXTENDED_CLIPBOARD_TEXT).to_be_bytes(),
    )
    .await
}

async fn write_extended_clipboard_provide(
    writer: &Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    text: &str,
) -> Result<()> {
    let text = text.replace('\n', "\r\n");
    let mut uncompressed = Vec::with_capacity(text.len() + 5);
    uncompressed.extend_from_slice(&u32::try_from(text.len() + 1)?.to_be_bytes());
    uncompressed.extend_from_slice(text.as_bytes());
    uncompressed.push(0);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&uncompressed)?;
    let compressed = encoder.finish()?;
    let mut payload = Vec::with_capacity(4 + compressed.len());
    payload
        .extend_from_slice(&(EXTENDED_CLIPBOARD_PROVIDE | EXTENDED_CLIPBOARD_TEXT).to_be_bytes());
    payload.extend_from_slice(&compressed);
    write_server_extended_clipboard(writer, &payload).await
}

async fn write_server_extended_clipboard(
    writer: &Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    payload: &[u8],
) -> Result<()> {
    let length = i32::try_from(payload.len()).context("extended clipboard is too large")?;
    let mut writer = writer.lock().await;
    writer.write_all(&[3, 0, 0, 0]).await?;
    writer.write_i32(-length).await?;
    writer.write_all(payload).await?;
    Ok(())
}

fn first_frame(
    source: Source,
    monitors: Option<&[(usize, Monitor)]>,
    capture_scale: f32,
) -> Result<Frame> {
    match source {
        Source::Desktop => {
            capture_monitors(monitors.context("no monitor is available")?, capture_scale)
                .context("desktop capture failed; grant capture permission or use --source demo")
        }
        Source::Demo => Ok(demo_frame(
            (1024.0 * capture_scale).round() as u16,
            (768.0 * capture_scale).round() as u16,
            0,
        )),
    }
}

fn select_monitors(index: Option<usize>, all_monitors: bool) -> Result<Vec<(usize, Monitor)>> {
    let monitors = Monitor::all().context("could not enumerate monitors")?;
    if all_monitors {
        if monitors.is_empty() {
            bail!("no monitor is available");
        }
        return Ok(monitors.into_iter().enumerate().collect());
    }
    match index {
        Some(index) => monitors
            .get(index)
            .cloned()
            .map(|monitor| vec![(index, monitor)])
            .with_context(|| format!("monitor index {index} is not available")),
        None => monitors
            .iter()
            .enumerate()
            .find(|(_, monitor)| monitor.is_primary().unwrap_or(false))
            .or_else(|| monitors.iter().enumerate().next())
            .map(|(index, monitor)| vec![(index, monitor.clone())])
            .context("no monitor is available"),
    }
}

fn monitor_description(monitor: &Monitor) -> String {
    format!(
        "name={:?}, primary={}, x={}, y={}, width={}, height={}, scale_factor={}",
        monitor.name().unwrap_or_else(|_| "unknown".to_owned()),
        monitor.is_primary().unwrap_or(false),
        monitor.x().unwrap_or_default(),
        monitor.y().unwrap_or_default(),
        monitor.width().unwrap_or_default(),
        monitor.height().unwrap_or_default(),
        monitor.scale_factor().unwrap_or(1.0),
    )
}

fn capture_monitor(monitor: &Monitor, capture_scale: f32) -> Result<Frame> {
    let image = monitor
        .capture_image()
        .context("could not capture monitor")?;
    capture_monitor_from_image(monitor, image, capture_scale)
}

fn capture_monitor_from_image(
    monitor: &Monitor,
    image: RgbaImage,
    capture_scale: f32,
) -> Result<Frame> {
    let x = monitor.x().context("could not read monitor x coordinate")?;
    let y = monitor.y().context("could not read monitor y coordinate")?;
    let scale_factor = monitor
        .scale_factor()
        .context("could not read monitor scale factor")?;
    frame_from_rgba(
        scale_image(image, capture_scale),
        x,
        y,
        scale_factor * capture_scale,
    )
}

struct CapturedMonitor {
    monitor_index: usize,
    /// Position in the normalized virtual RFB pixel grid.
    virtual_x: i32,
    virtual_y: i32,
    /// Origin in the platform desktop's logical coordinate system.
    origin_x: i32,
    origin_y: i32,
    /// RFB pixels per platform input point after the configured capture scale.
    input_scale: f32,
    image: RgbaImage,
}

/// macOS display capture backed by CoreGraphics' live display stream.
///
/// xcap's macOS backend uses a window-list image snapshot.  On some macOS
/// releases that snapshot can remain unchanged after a display wake even
/// though the logged-in desktop is visibly changing.  CGDisplayStream instead
/// delivers compositor frames for the selected physical display, which keeps
/// menu popovers and application windows current for VNC clients.
#[cfg(target_os = "macos")]
struct MacosDesktopCapture {
    frame_interval: Duration,
    virtual_desktop: bool,
    live_backend_available: bool,
    logger: Logger,
    streams: BTreeMap<u32, MacosDisplayStream>,
}

#[cfg(target_os = "macos")]
impl MacosDesktopCapture {
    fn new(fps: u8, virtual_desktop: bool, logger: Logger) -> Self {
        Self {
            frame_interval: Duration::from_secs_f64(1.0 / f64::from(fps)),
            virtual_desktop,
            live_backend_available: true,
            logger,
            streams: BTreeMap::new(),
        }
    }

    fn capture(&mut self, monitors: &[(usize, Monitor)], capture_scale: f32) -> Result<Frame> {
        if !self.live_backend_available {
            return capture_monitors(monitors, capture_scale)
                .context("macOS snapshot fallback could not capture the desktop");
        }
        match self.capture_live(monitors, capture_scale) {
            Ok(frame) => Ok(frame),
            Err(error) => {
                self.streams.clear();
                self.live_backend_available = false;
                self.logger.error(format!(
                    "macOS desktop capture backend fallback: CoreGraphics CGDisplayStream is unavailable ({error:#}); using xcap snapshot capture. Grant Screen Recording permission to the standalone kVNC binary, then restart kVNC to enable live desktop updates."
                ));
                capture_monitors(monitors, capture_scale)
                    .context("macOS snapshot fallback could not capture the desktop")
            }
        }
    }

    fn capture_live(&mut self, monitors: &[(usize, Monitor)], capture_scale: f32) -> Result<Frame> {
        if monitors.is_empty() {
            bail!("no monitor is available");
        }
        let display_ids = monitors
            .iter()
            .map(|(_, monitor)| monitor.id().context("could not read monitor display id"))
            .collect::<Result<Vec<_>>>()?;
        self.streams
            .retain(|display_id, _| display_ids.contains(display_id));

        let mut images = Vec::with_capacity(monitors.len());
        for (monitor_index, monitor) in monitors {
            let display_id = monitor.id().context("could not read monitor display id")?;
            let (output_width, output_height) =
                macos_stream_dimensions(monitor, capture_scale, self.virtual_desktop)?;
            let replace_stream = self.streams.get(&display_id).is_none_or(|stream| {
                stream.output_width != output_width
                    || stream.output_height != output_height
                    || stream.is_stopped()
            });
            if replace_stream {
                self.streams.insert(
                    display_id,
                    MacosDisplayStream::new(
                        display_id,
                        output_width,
                        output_height,
                        self.frame_interval,
                    )?,
                );
            }
            let image = self
                .streams
                .get(&display_id)
                .context("macOS display stream was not retained")?
                .image()?;
            images.push((*monitor_index, monitor, image));
        }

        if images.len() == 1 && !self.virtual_desktop {
            let (_, monitor, image) = images.pop().expect("single image is present");
            return capture_monitor_from_image(monitor, image, capture_scale);
        }
        images
            .into_iter()
            .map(|(monitor_index, monitor, image)| {
                capture_monitor_image_from_image(monitor_index, monitor, image, capture_scale)
            })
            .collect::<Result<Vec<_>>>()
            .and_then(compose_virtual_desktop)
    }
}

#[cfg(target_os = "macos")]
struct MacosDisplayStream {
    output_width: usize,
    output_height: usize,
    state: Arc<StdMutex<MacosDisplayStreamState>>,
    stream: CGDisplayStream,
    // The callback queue must outlive CGDisplayStream.
    _queue: DispatchRetained<DispatchQueue>,
}

#[cfg(target_os = "macos")]
struct MacosDisplayStreamState {
    image: Option<RgbaImage>,
    stopped: bool,
    failure: Option<String>,
}

#[cfg(target_os = "macos")]
impl MacosDisplayStream {
    fn new(
        display_id: u32,
        output_width: usize,
        output_height: usize,
        frame_interval: Duration,
    ) -> Result<Self> {
        const BGRA_32: i32 = 0x4247_5241; // kCVPixelFormatType_32BGRA
        let state = Arc::new(StdMutex::new(MacosDisplayStreamState {
            image: None,
            stopped: false,
            failure: None,
        }));
        let callback_state = Arc::clone(&state);
        let queue = DispatchQueue::new(
            &format!("dev.kvnc.display-stream.{display_id}"),
            DispatchQueueAttr::SERIAL,
        );
        let min_frame_time = CFNumber::from(frame_interval.as_secs_f64());
        let min_frame_time_key: CFString = CGDisplayStreamProperties::MinimumFrameTime.into();
        let queue_depth = CFNumber::from(3_i64);
        let queue_depth_key: CFString = CGDisplayStreamProperties::QueueDepth.into();
        let show_cursor_key: CFString = CGDisplayStreamProperties::ShowCursor.into();
        let show_cursor = CFBoolean::true_value();
        let properties: CFDictionary<CFString, CFType> = CFDictionary::from_CFType_pairs(&[
            (min_frame_time_key, min_frame_time.as_CFType()),
            (queue_depth_key, queue_depth.as_CFType()),
            (show_cursor_key, show_cursor.as_CFType()),
        ]);
        let stream = CGDisplayStream::new_with_dispatch_queue(
            display_id,
            output_width,
            output_height,
            BGRA_32,
            Some(&properties),
            &queue,
            move |status, _timestamp, surface, _update| {
                if status == CGDisplayStreamFrameStatus::Stopped {
                    if let Ok(mut state) = callback_state.lock() {
                        state.stopped = true;
                        state.failure = Some("CGDisplayStream stopped".to_owned());
                    }
                    return;
                }
                if status != CGDisplayStreamFrameStatus::FrameComplete {
                    return;
                }
                let Some(surface) = surface else {
                    if let Ok(mut state) = callback_state.lock() {
                        state.failure = Some("CGDisplayStream delivered no IOSurface".to_owned());
                    }
                    return;
                };
                match macos_image_from_surface(&surface) {
                    Ok(image) => {
                        if let Ok(mut state) = callback_state.lock() {
                            state.image = Some(image);
                            state.failure = None;
                        }
                    }
                    Err(error) => {
                        if let Ok(mut state) = callback_state.lock() {
                            state.failure =
                                Some(format!("could not copy CGDisplayStream surface: {error:#}"));
                        }
                    }
                }
            },
        )
        .map_err(|_| {
            anyhow::anyhow!("could not create CGDisplayStream for display {display_id}")
        })?;
        let start_result = stream.start();
        if !matches!(start_result, core_graphics2::error::CGError::Success) {
            bail!("could not start CGDisplayStream for display {display_id}: {start_result:?}");
        }
        let capture = Self {
            output_width,
            output_height,
            state,
            stream,
            _queue: queue,
        };
        capture.wait_for_initial_frame(display_id)?;
        Ok(capture)
    }

    fn wait_for_initial_frame(&self, display_id: u32) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.image().is_ok() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.image().with_context(|| {
            format!("CGDisplayStream did not deliver an initial frame for display {display_id}")
        })?;
        Ok(())
    }

    fn image(&self) -> Result<RgbaImage> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("macOS display stream state is unavailable"))?;
        if let Some(image) = &state.image {
            return Ok(image.clone());
        }
        if let Some(failure) = &state.failure {
            bail!("{failure}");
        }
        bail!("CGDisplayStream has not delivered a complete frame")
    }

    fn is_stopped(&self) -> bool {
        self.state.lock().map(|state| state.stopped).unwrap_or(true)
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacosDisplayStream {
    fn drop(&mut self) {
        let _ = self.stream.stop();
    }
}

#[cfg(target_os = "macos")]
fn macos_stream_dimensions(
    monitor: &Monitor,
    capture_scale: f32,
    virtual_desktop: bool,
) -> Result<(usize, usize)> {
    let logical_width = monitor.width().context("could not read monitor width")?;
    let logical_height = monitor.height().context("could not read monitor height")?;
    let scale_factor = monitor
        .scale_factor()
        .context("could not read monitor scale factor")?;
    // A single-monitor framebuffer preserves native Retina density before the
    // configured scale is applied. A virtual desktop is normalized to macOS
    // logical coordinates before composition, matching the existing RFB/input
    // geometry and avoiding needless full-resolution copies of every screen.
    let native_scale = if virtual_desktop { 1.0 } else { scale_factor };
    let stream_scale = if virtual_desktop { capture_scale } else { 1.0 };
    let output_width = (logical_width as f32 * native_scale * stream_scale).round() as usize;
    let output_height = (logical_height as f32 * native_scale * stream_scale).round() as usize;
    if output_width == 0 || output_height == 0 {
        bail!("macOS display stream output dimensions are invalid");
    }
    Ok((output_width, output_height))
}

#[cfg(target_os = "macos")]
fn macos_image_from_surface(surface: &IOSurfaceRef) -> Result<RgbaImage> {
    const BGRA_32: u32 = 0x4247_5241; // kCVPixelFormatType_32BGRA
    let width = surface.width();
    let height = surface.height();
    let bytes_per_row = surface.bytes_per_row();
    if width == 0 || height == 0 || surface.bytes_per_element() != 4 {
        bail!("CGDisplayStream returned an invalid IOSurface layout");
    }
    if surface.pixel_format() != BGRA_32 {
        bail!("CGDisplayStream returned an unexpected pixel format");
    }
    let row_bytes = width
        .checked_mul(4)
        .context("CGDisplayStream row size exceeds usize")?;
    if bytes_per_row < row_bytes {
        bail!("CGDisplayStream row stride is smaller than its image width");
    }
    let source_len = bytes_per_row
        .checked_mul(height)
        .context("CGDisplayStream image size exceeds usize")?;
    let destination_len = row_bytes
        .checked_mul(height)
        .context("CGDisplayStream converted image size exceeds usize")?;
    let lock_result = unsafe { surface.lock(IOSurfaceLockOptions::ReadOnly, ptr::null_mut()) };
    if lock_result != 0 {
        bail!("IOSurfaceLock returned {lock_result}");
    }
    let source = unsafe {
        std::slice::from_raw_parts(surface.base_address().as_ptr().cast::<u8>(), source_len)
    };
    let mut rgba = vec![0_u8; destination_len];
    for row in 0..height {
        let source_row = &source[row * bytes_per_row..row * bytes_per_row + row_bytes];
        let destination_row = &mut rgba[row * row_bytes..(row + 1) * row_bytes];
        destination_row.copy_from_slice(source_row);
    }
    let (pixels, remainder) = rgba.as_chunks_mut::<4>();
    debug_assert!(remainder.is_empty());
    for pixel in pixels {
        pixel.swap(0, 2);
    }
    let unlock_result = unsafe { surface.unlock(IOSurfaceLockOptions::ReadOnly, ptr::null_mut()) };
    if unlock_result != 0 {
        bail!("IOSurfaceUnlock returned {unlock_result}");
    }
    RgbaImage::from_raw(width as u32, height as u32, rgba)
        .context("CGDisplayStream returned an invalid RGBA image")
}

fn capture_monitors(monitors: &[(usize, Monitor)], capture_scale: f32) -> Result<Frame> {
    if monitors.len() == 1 {
        return capture_monitor(&monitors[0].1, capture_scale);
    }
    let captures = monitors
        .iter()
        .map(|(index, monitor)| capture_monitor_image(*index, monitor, capture_scale))
        .collect::<Result<Vec<_>>>()?;
    compose_virtual_desktop(captures)
}

fn capture_monitor_image(
    monitor_index: usize,
    monitor: &Monitor,
    capture_scale: f32,
) -> Result<CapturedMonitor> {
    let image = monitor
        .capture_image()
        .context("could not capture monitor")?;
    capture_monitor_image_from_image(monitor_index, monitor, image, capture_scale)
}

fn capture_monitor_image_from_image(
    monitor_index: usize,
    monitor: &Monitor,
    image: RgbaImage,
    capture_scale: f32,
) -> Result<CapturedMonitor> {
    let origin_x = monitor.x().context("could not read monitor x coordinate")?;
    let origin_y = monitor.y().context("could not read monitor y coordinate")?;
    let logical_width = monitor.width().context("could not read monitor width")?;
    let logical_height = monitor.height().context("could not read monitor height")?;

    // The capture image may be physical Retina pixels while x/y/width/height
    // are desktop points. Normalize every monitor to the same logical desktop
    // grid before placing it in the single RFB framebuffer.
    let width = ((logical_width as f32 * capture_scale).round() as u32).max(1);
    let height = ((logical_height as f32 * capture_scale).round() as u32).max(1);
    let image = if image.width() == width && image.height() == height {
        image
    } else {
        image::imageops::resize(&image, width, height, FilterType::Triangle)
    };
    let virtual_x = (origin_x as f32 * capture_scale).round() as i32;
    let virtual_y = (origin_y as f32 * capture_scale).round() as i32;
    Ok(CapturedMonitor {
        monitor_index,
        virtual_x,
        virtual_y,
        origin_x,
        origin_y,
        input_scale: capture_scale,
        image,
    })
}

fn compose_virtual_desktop(captures: Vec<CapturedMonitor>) -> Result<Frame> {
    let input_scale = captures
        .first()
        .context("no monitor is available")?
        .input_scale;
    let min_x = captures
        .iter()
        .map(|capture| capture.virtual_x)
        .min()
        .context("no monitor is available")?;
    let min_y = captures
        .iter()
        .map(|capture| capture.virtual_y)
        .min()
        .context("no monitor is available")?;
    let max_x = captures
        .iter()
        .map(|capture| {
            let image_width = i32::try_from(capture.image.width())
                .context("virtual desktop width exceeds i32 limit")?;
            capture
                .virtual_x
                .checked_add(image_width)
                .context("virtual desktop width exceeds i32 limit")
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .context("no monitor is available")?;
    let max_y = captures
        .iter()
        .map(|capture| {
            let image_height = i32::try_from(capture.image.height())
                .context("virtual desktop height exceeds i32 limit")?;
            capture
                .virtual_y
                .checked_add(image_height)
                .context("virtual desktop height exceeds i32 limit")
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .context("no monitor is available")?;
    let width = u32::try_from(max_x - min_x).context("virtual desktop width is invalid")?;
    let height = u32::try_from(max_y - min_y).context("virtual desktop height is invalid")?;
    let width_u16 = u16::try_from(width).context(
        "combined desktop width exceeds RFB u16 limit; reduce scale or select a monitor",
    )?;
    let height_u16 = u16::try_from(height).context(
        "combined desktop height exceeds RFB u16 limit; reduce scale or select a monitor",
    )?;
    let mut image = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255]));
    let mut input_regions = Vec::with_capacity(captures.len());
    for capture in captures {
        let x = u32::try_from(capture.virtual_x - min_x)
            .context("virtual desktop monitor x coordinate is invalid")?;
        let y = u32::try_from(capture.virtual_y - min_y)
            .context("virtual desktop monitor y coordinate is invalid")?;
        let monitor_width =
            u16::try_from(capture.image.width()).context("monitor width exceeds RFB u16 limit")?;
        let monitor_height = u16::try_from(capture.image.height())
            .context("monitor height exceeds RFB u16 limit")?;
        let rfb_x = u16::try_from(x).context("virtual desktop monitor x exceeds RFB u16 limit")?;
        let rfb_y = u16::try_from(y).context("virtual desktop monitor y exceeds RFB u16 limit")?;
        let destination_stride = usize::try_from(width).expect("u32 fits usize") * 4;
        let source_stride = usize::try_from(capture.image.width()).expect("u32 fits usize") * 4;
        for row in 0..usize::try_from(capture.image.height()).expect("u32 fits usize") {
            let destination_start = (usize::try_from(y).expect("u32 fits usize") + row)
                * destination_stride
                + usize::try_from(x).expect("u32 fits usize") * 4;
            let source_start = row * source_stride;
            image.as_mut()[destination_start..destination_start + source_stride].copy_from_slice(
                &capture.image.as_raw()[source_start..source_start + source_stride],
            );
        }
        input_regions.push(InputRegion {
            monitor_index: capture.monitor_index,
            rfb_x,
            rfb_y,
            width: monitor_width,
            height: monitor_height,
            origin_x: capture.origin_x,
            origin_y: capture.origin_y,
            scale_factor: capture.input_scale,
        });
    }
    debug_assert_eq!(
        width_u16,
        u16::try_from(image.width()).expect("validated width")
    );
    debug_assert_eq!(
        height_u16,
        u16::try_from(image.height()).expect("validated height")
    );
    frame_from_rgba_with_regions(image, min_x, min_y, input_scale, input_regions)
}

fn scale_image(image: RgbaImage, capture_scale: f32) -> RgbaImage {
    if (capture_scale - 1.0).abs() < f32::EPSILON {
        return image;
    }
    let width = ((image.width() as f32 * capture_scale).round() as u32).max(1);
    let height = ((image.height() as f32 * capture_scale).round() as u32).max(1);
    image::imageops::resize(&image, width, height, FilterType::Triangle)
}

fn frame_from_rgba(image: RgbaImage, x: i32, y: i32, scale_factor: f32) -> Result<Frame> {
    let (width, height) = image.dimensions();
    let width = u16::try_from(width).context("display width exceeds RFB u16 limit")?;
    let height = u16::try_from(height).context("display height exceeds RFB u16 limit")?;
    frame_from_rgba_with_regions(
        image,
        x,
        y,
        scale_factor,
        vec![InputRegion {
            monitor_index: 0,
            rfb_x: 0,
            rfb_y: 0,
            width,
            height,
            origin_x: x,
            origin_y: y,
            scale_factor,
        }],
    )
}

fn frame_from_rgba_with_regions(
    image: RgbaImage,
    x: i32,
    y: i32,
    scale_factor: f32,
    input_regions: Vec<InputRegion>,
) -> Result<Frame> {
    let (width, height) = image.dimensions();
    let width = u16::try_from(width).context("display width exceeds RFB u16 limit")?;
    let height = u16::try_from(height).context("display height exceeds RFB u16 limit")?;
    let mut bgrx = image.into_raw();
    let (pixels, remainder) = bgrx.as_chunks_mut::<4>();
    debug_assert!(remainder.is_empty());
    for pixel in pixels {
        pixel.swap(0, 2);
        pixel[3] = 0;
    }
    Ok(Frame {
        width,
        height,
        scale_factor,
        x,
        y,
        bgrx,
        input_regions,
    })
}

fn demo_frame(width: u16, height: u16, frame: u32) -> Frame {
    let mut bgrx = vec![0; usize::from(width) * usize::from(height) * 4];
    for y in 0..height {
        for x in 0..width {
            let offset = (usize::from(y) * usize::from(width) + usize::from(x)) * 4;
            bgrx[offset] = ((frame / 2) & 0xff) as u8;
            bgrx[offset + 1] = (u32::from(y).wrapping_add(frame) & 0xff) as u8;
            bgrx[offset + 2] = (u32::from(x).wrapping_add(frame) & 0xff) as u8;
        }
    }
    Frame {
        width,
        height,
        scale_factor: 1.0,
        x: 0,
        y: 0,
        bgrx,
        input_regions: vec![InputRegion {
            monitor_index: 0,
            rfb_x: 0,
            rfb_y: 0,
            width,
            height,
            origin_x: 0,
            origin_y: 0,
            scale_factor: 1.0,
        }],
    }
}

fn list_monitors() -> Result<()> {
    let monitors = Monitor::all().context("could not enumerate monitors")?;
    if monitors.is_empty() {
        bail!("no monitor is available");
    }
    for (index, monitor) in monitors.iter().enumerate() {
        println!(
            "{index}: name={:?} primary={} x={} y={} width={} height={}",
            monitor.name().unwrap_or_else(|_| "unknown".to_owned()),
            monitor.is_primary().unwrap_or(false),
            monitor.x().unwrap_or_default(),
            monitor.y().unwrap_or_default(),
            monitor.width().unwrap_or_default(),
            monitor.height().unwrap_or_default()
        );
    }
    Ok(())
}

fn system_clipboard() -> Result<Option<String>> {
    let text = Clipboard::new()
        .context("could not access the system clipboard")?
        .get_text();
    clipboard_text_or_absent(text)
}

fn clipboard_text_or_absent(
    text: std::result::Result<String, ClipboardError>,
) -> Result<Option<String>> {
    match text {
        Ok(text) => Ok(Some(text)),
        // This covers an empty clipboard and non-text clipboard formats. Both
        // are expected states, not server errors.
        Err(ClipboardError::ContentNotAvailable) => Ok(None),
        Err(error) => Err(error).context("could not read UTF-8 text from the system clipboard"),
    }
}

fn set_system_clipboard(text: &str) -> Result<()> {
    Clipboard::new()
        .context("could not access the system clipboard")?
        .set_text(text)
        .context("could not write UTF-8 text to the system clipboard")
}

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    use super::replace_checkout_binary;
    use super::{
        CapturedMonitor, CheckoutKind, EXTENDED_CLIPBOARD_CAPS, EXTENDED_CLIPBOARD_PROVIDE,
        EXTENDED_CLIPBOARD_TEXT, ExtendedClipboardMessage, Frame, InputInitialization, Key,
        KeyboardLayout, LogRotation, PixelFormat, RotatingLogFile, SERVER_PIXEL_FORMAT,
        add_missing_config_defaults, changed_region, checkout_kind, clipboard_text_or_absent,
        compose_virtual_desktop, demo_frame, encode_hextile, encode_pixels, frame_from_rgba,
        frame_region, initialize_input, input_coordinates, parse_extended_clipboard,
        read_to_string, rfb_key, unsigned_fixed, write_initial_config,
    };
    #[cfg(target_os = "linux")]
    use super::{LinuxDisplaySession, linux_display_session};
    #[cfg(target_os = "macos")]
    use super::{MACOS_DISPLAY_ASSERTION_ARGUMENTS, MACOS_DISPLAY_WAKE_ARGUMENTS};
    use arboard::Error as ClipboardError;
    use flate2::{Compression, write::ZlibEncoder};
    use image::RgbaImage;
    use rand::{RngCore, rngs::OsRng};
    use rsa::{Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey, traits::PublicKeyParts};
    use std::fs;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn unavailable_input_initialization_falls_back_to_view_only() {
        let disabled = initialize_input(false, || -> anyhow::Result<()> {
            panic!("disabled input must not initialize")
        });
        assert!(matches!(disabled, InputInitialization::Disabled));

        let ready = initialize_input(true, || Ok("input"));
        assert!(matches!(ready, InputInitialization::Ready("input")));

        let unavailable = initialize_input(true, || -> anyhow::Result<()> {
            Err(anyhow::anyhow!("permission denied"))
        });
        match unavailable {
            InputInitialization::Unavailable(error) => {
                assert!(error.to_string().contains("permission denied"));
            }
            _ => panic!("failed input initialization must use the view-only fallback"),
        }
    }

    #[test]
    fn input_coordinates_follow_the_latest_capture_geometry() {
        let rfb_position = (100, 200);
        assert_eq!(input_coordinates(rfb_position, (0, 0), 1.0), (100, 200));
        assert_eq!(
            input_coordinates(rfb_position, (-3840, -1260), 0.5),
            (-3640, -860)
        );
    }

    #[test]
    fn jj_metadata_takes_precedence_over_colocated_git_metadata() {
        let directory = tempdir().expect("temporary checkout");
        assert_eq!(checkout_kind(directory.path()), CheckoutKind::Git);

        fs::create_dir(directory.path().join(".git")).expect("Git metadata");
        assert_eq!(checkout_kind(directory.path()), CheckoutKind::Git);

        fs::create_dir(directory.path().join(".jj")).expect("JJ metadata");
        assert_eq!(checkout_kind(directory.path()), CheckoutKind::Jujutsu);
    }

    #[test]
    fn absent_text_clipboard_is_a_normal_polling_state() {
        assert_eq!(
            clipboard_text_or_absent(Err(ClipboardError::ContentNotAvailable))
                .expect("empty clipboard is not an error"),
            None
        );
        assert_eq!(
            clipboard_text_or_absent(Ok("shared text".to_owned())).expect("text clipboard"),
            Some("shared text".to_owned())
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn rebuilt_standalone_binary_replaces_the_previous_bin_file() {
        let directory = tempdir().expect("temporary checkout");
        let release_directory = directory.path().join("target/release");
        let binary_directory = directory.path().join("bin");
        fs::create_dir_all(&release_directory).expect("release directory");
        fs::create_dir_all(&binary_directory).expect("binary directory");
        fs::write(release_directory.join("kvnc"), b"new binary").expect("rebuilt binary");
        fs::write(binary_directory.join("kvnc"), b"old binary").expect("previous binary");

        replace_checkout_binary(directory.path()).expect("replace standalone binary");

        assert_eq!(
            fs::read(binary_directory.join("kvnc")).expect("replaced binary"),
            b"new binary"
        );
        assert!(!binary_directory.join("kvnc.next").exists());
    }

    #[test]
    fn size_rotation_retains_only_the_requested_archive_count() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("kvnc.log");
        let mut log =
            RotatingLogFile::open(&path, LogRotation::None, 1, 1).expect("open rotating log");

        for line in ["first", "second", "third"] {
            log.write_line(line).expect("write log line");
            assert!(log.rotate_if_due().expect("rotate oversized log"));
        }

        let archives = std::fs::read_dir(directory.path())
            .expect("read temporary directory")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("kvnc.log."))
            .count();
        assert_eq!(archives, 1);
        assert!(path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn externally_replaced_active_log_is_reopened_before_writing() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("kvnc.log");
        let archived_path = directory.path().join("kvnc.log.external");
        let mut log =
            RotatingLogFile::open(&path, LogRotation::None, 0, 1).expect("open rotating log");
        log.write_line("before replacement")
            .expect("write original log line");

        fs::rename(&path, &archived_path).expect("external rotation");
        fs::write(&path, "external replacement\n").expect("new external active log");

        assert!(log.rotate_if_due().expect("reopen replaced log"));
        log.write_line("after replacement")
            .expect("write reopened log line");

        assert!(
            read_to_string(&archived_path)
                .expect("archived log")
                .contains("before replacement")
        );
        let active = read_to_string(&path).expect("reopened active log");
        assert!(active.contains("external replacement"));
        assert!(active.contains("after replacement"));
    }

    #[test]
    fn rgba_is_converted_to_rfb_bgrx() {
        let image =
            RgbaImage::from_raw(1, 1, vec![0x10, 0x20, 0x30, 0xff]).expect("valid RGBA image");
        let frame = frame_from_rgba(image, 0, 0, 1.0).expect("RFB-sized image");

        assert_eq!((frame.width, frame.height), (1, 1));
        assert_eq!(frame.bgrx, vec![0x30, 0x20, 0x10, 0]);
    }

    #[test]
    fn demo_frame_has_rfb_pixel_size() {
        let frame = demo_frame(3, 2, 0);

        assert_eq!(frame.bgrx.len(), 3 * 2 * 4);
    }

    #[test]
    fn virtual_desktop_composes_monitor_positions_and_routes_input() {
        let red = RgbaImage::from_pixel(2, 1, image::Rgba([255, 0, 0, 255]));
        let blue = RgbaImage::from_pixel(2, 1, image::Rgba([0, 0, 255, 255]));
        let frame = compose_virtual_desktop(vec![
            CapturedMonitor {
                monitor_index: 4,
                virtual_x: -2,
                virtual_y: 0,
                origin_x: -2,
                origin_y: 0,
                input_scale: 1.0,
                image: red,
            },
            CapturedMonitor {
                monitor_index: 9,
                virtual_x: 0,
                virtual_y: 0,
                origin_x: 0,
                origin_y: 0,
                input_scale: 1.0,
                image: blue,
            },
        ])
        .expect("composed virtual desktop");

        assert_eq!((frame.width, frame.height), (4, 1));
        assert_eq!(&frame.bgrx[0..4], &[0, 0, 255, 0]);
        assert_eq!(&frame.bgrx[8..12], &[255, 0, 0, 0]);
        assert_eq!(
            frame.input_region(1, 0).map(|region| region.monitor_index),
            Some(4)
        );
        assert_eq!(
            frame.input_region(2, 0).map(|region| region.monitor_index),
            Some(9)
        );
    }

    #[test]
    fn ra2ne_fixed_width_client_key_encrypts_a_sixteen_byte_server_random() {
        // noVNC sends both n and e as 256-byte unsigned big-endian values.
        let client_private = RsaPrivateKey::new(&mut OsRng, 2048).expect("client key");
        let client_public = RsaPublicKey::from(&client_private);
        let modulus = unsigned_fixed(client_public.n(), 256).expect("fixed modulus");
        let exponent = unsigned_fixed(client_public.e(), 256).expect("fixed exponent");
        let wire_public = RsaPublicKey::new(
            rsa::BigUint::from_bytes_be(&modulus),
            rsa::BigUint::from_bytes_be(&exponent),
        )
        .expect("noVNC-format public key");
        let mut server_random = [0_u8; 16];
        OsRng.fill_bytes(&mut server_random);
        let ciphertext = wire_public
            .encrypt(&mut OsRng, Pkcs1v15Encrypt, &server_random)
            .expect("RSAES-PKCS1-v1_5 encryption");

        assert_eq!(ciphertext.len(), 256);
        assert_eq!(
            client_private
                .decrypt(Pkcs1v15Encrypt, &ciphertext)
                .expect("client decrypts server random"),
            server_random
        );
    }

    #[test]
    fn rgb565_client_format_is_encoded_little_endian() {
        let image = RgbaImage::from_raw(1, 1, vec![255, 0, 0, 255]).expect("valid RGBA image");
        let frame = frame_from_rgba(image, 0, 0, 1.0).expect("RFB-sized image");
        let rgb565 = PixelFormat {
            bits_per_pixel: 16,
            depth: 16,
            big_endian: false,
            true_color: true,
            red_max: 31,
            green_max: 63,
            blue_max: 31,
            red_shift: 11,
            green_shift: 5,
            blue_shift: 0,
        };

        assert_eq!(
            encode_pixels(&frame, rgb565).expect("valid RGB565"),
            [0, 0xf8]
        );
    }

    #[test]
    fn space_keysym_uses_the_physical_space_key() {
        assert_eq!(rfb_key(0x20, KeyboardLayout::Auto), Some(Key::Space));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_display_wake_matches_the_short_user_activity_command() {
        assert_eq!(MACOS_DISPLAY_WAKE_ARGUMENTS, ["-u", "-t", "5"]);
        assert_eq!(MACOS_DISPLAY_ASSERTION_ARGUMENTS, ["-d"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_display_wake_uses_the_active_graphical_session_type() {
        assert_eq!(
            linux_display_session(Some("wayland"), true, true),
            LinuxDisplaySession::Wayland
        );
        assert_eq!(
            linux_display_session(Some("x11"), true, true),
            LinuxDisplaySession::X11
        );
        assert_eq!(
            linux_display_session(None, true, false),
            LinuxDisplaySession::X11
        );
        assert_eq!(
            linux_display_session(None, false, true),
            LinuxDisplaySession::Wayland
        );
        assert_eq!(
            linux_display_session(None, false, false),
            LinuxDisplaySession::Unsupported
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn jis_kana_aliases_use_jis_physical_keys() {
        assert_eq!(
            rfb_key(0xa5, KeyboardLayout::Jis),
            Some(Key::Other(93)),
            "yen must be the JIS yen key"
        );
        assert_eq!(
            rfb_key(u32::from('ろ'), KeyboardLayout::Jis),
            Some(Key::Other(94)),
            "ro must be the distinct JIS Ro key"
        );
        assert_eq!(
            rfb_key(u32::from('@'), KeyboardLayout::Jis),
            Some(Key::Other(33))
        );
        assert_eq!(
            rfb_key(u32::from('^'), KeyboardLayout::Jis),
            Some(Key::Other(24))
        );
        assert_eq!(
            rfb_key(u32::from('け'), KeyboardLayout::Jis),
            Some(Key::Other(39))
        );
        assert_eq!(
            rfb_key(u32::from('む'), KeyboardLayout::Jis),
            Some(Key::Other(42))
        );
    }

    #[test]
    fn solid_frame_uses_compact_hextile_background() {
        let image = RgbaImage::from_raw(2, 2, [0x10, 0x20, 0x30, 0xff].repeat(4))
            .expect("valid RGBA image");
        let frame = frame_from_rgba(image, 0, 0, 1.0).expect("RFB-sized image");

        assert_eq!(
            encode_hextile(&frame, SERVER_PIXEL_FORMAT).expect("valid Hextile"),
            [2 | 8 | 16, 0x30, 0x20, 0x10, 0, 0]
        );
    }

    #[test]
    fn changed_region_and_crop_only_include_changed_pixels() {
        let previous = Frame {
            width: 3,
            height: 2,
            scale_factor: 1.0,
            x: 0,
            y: 0,
            bgrx: vec![0; 3 * 2 * 4],
            input_regions: Vec::new(),
        };
        let mut current = previous.clone();
        current.bgrx[(usize::from(1_u16) * 3 + 2) * 4] = 0xff;

        assert_eq!(changed_region(&previous, &current), Some((2, 1, 1, 1)));
        assert_eq!(frame_region(&current, 2, 1, 1, 1).bgrx, [0xff, 0, 0, 0]);
        assert_eq!(changed_region(&current, &current), None);
    }

    #[test]
    fn extended_clipboard_parses_caps_and_utf8_text() {
        let caps = (EXTENDED_CLIPBOARD_CAPS | EXTENDED_CLIPBOARD_TEXT).to_be_bytes();
        assert!(matches!(
            parse_extended_clipboard(&caps).expect("valid capabilities"),
            ExtendedClipboardMessage::Capabilities { formats, .. } if formats == EXTENDED_CLIPBOARD_TEXT
        ));

        let text = "コピー\n貼り付け";
        let mut uncompressed = Vec::new();
        let normalized = text.replace('\n', "\r\n");
        uncompressed.extend_from_slice(&u32::try_from(normalized.len() + 1).unwrap().to_be_bytes());
        uncompressed.extend_from_slice(normalized.as_bytes());
        uncompressed.push(0);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&uncompressed).unwrap();
        let mut payload = (EXTENDED_CLIPBOARD_PROVIDE | EXTENDED_CLIPBOARD_TEXT)
            .to_be_bytes()
            .to_vec();
        payload.extend_from_slice(&encoder.finish().unwrap());
        assert!(matches!(
            parse_extended_clipboard(&payload).expect("valid provided text"),
            ExtendedClipboardMessage::Provide(value) if value == text
        ));
    }

    #[test]
    fn initial_config_is_safe_and_update_adds_only_missing_defaults() {
        let directory = tempfile::tempdir().expect("temporary configuration directory");
        let path = directory.path().join("config.toml");
        write_initial_config(&path, false, false).expect("initial configuration");
        let contents = read_to_string(&path).expect("initial configuration contents");
        assert!(contents.contains("listen = \"127.0.0.1:5900\""));
        assert!(contents.contains("allow_input = false"));
        assert!(contents.contains("all_monitors = false"));
        assert!(contents.contains("wake_display_on_connect = false"));
        let expected_log_path = directory
            .path()
            .join("kvnc.log")
            .to_string_lossy()
            .replace('\\', "\\\\");
        assert!(contents.contains(&format!("log_file = \"{}\"", expected_log_path)));
        assert!(write_initial_config(&path, false, false).is_err());

        let input_path = directory.path().join("input-enabled.toml");
        write_initial_config(&input_path, false, true).expect("input-enabled configuration");
        assert!(
            read_to_string(&input_path)
                .expect("input-enabled configuration contents")
                .contains("allow_input = true")
        );

        fs::write(&path, "listen = \"127.0.0.1:5901\"\n[auth]\npassword_hash = \"$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$hash\"\n")
            .expect("minimal existing configuration");
        let added = add_missing_config_defaults(&path).expect("configuration update");
        let updated = read_to_string(&path).expect("updated configuration contents");
        assert!(added.contains(&"allow_input"));
        assert!(added.contains(&"all_monitors"));
        assert!(added.contains(&"wake_display_on_connect"));
        assert!(updated.contains("listen = \"127.0.0.1:5901\""));
        assert!(updated.contains("allow_input = false"));
        assert!(updated.contains("all_monitors = false"));
        assert!(updated.contains("wake_display_on_connect = false"));
        assert!(updated.contains("[auth]\npassword_hash"));
        assert!(
            directory
                .path()
                .read_dir()
                .expect("configuration directory")
                .flatten()
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("config.toml.backup."))
        );
    }
}

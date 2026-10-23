//! Private, real-engine verification. Game probes stay Lua because they use engine APIs.
mod characters;
mod hats;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const OPERATION_TIMEOUT: Duration = Duration::from_secs(600);
const IO_SLICE: Duration = Duration::from_millis(200);
const MAX_PACKET: usize = 16 * 1024 * 1024;
const MEASUREMENT_SCOPE: &str = "Loopback RCON imageSpaces first request per variant and repeated metadata-cache queries; identical directive paths may already be cached. Includes IPC/scheduling. Helper caches shared; not OS-cold decoding, client texture upload, frame time, FPS, or equipment network packets.";
const NATIVE_VARIANTS: [&str; 8] = [
    "conservative/auto",
    "compact/auto",
    "conservative/compressed",
    "conservative/passes",
    "compact/compressed",
    "auto/text",
    "auto/compressed",
    "auto/passes",
];
// Keep each RCON invocation below the unchanged default vanilla Lua limit.
const VARIANTS_PER_BATCH: usize = 4;
const MEASURE_VARIANTS: [&str; 8] = [
    "original/off",
    "conservative/passes",
    "conservative/auto",
    "conservative/compressed",
    "compact/compressed",
    "auto/text",
    "auto/compressed",
    "auto/passes",
];
const HELP: &str = "Usage: outfit native-verify --mode vanilla|opensb --server PATH --pak PATH --fixture PATH --run-dir NEW_PRIVATE_DIR [--opensb-assets PATH] [--measure-images]\nRuns real Linux game probes in new private storage, on two allowed CPUs, with loopback-only RCON. OpenStarbound requires --opensb-assets. Image timings include IPC and shared metadata caches, not rendering or OS-cold decoding.";

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Vanilla,
    Opensb,
}
impl Mode {
    fn key(self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Opensb => "opensb",
        }
    }
}

#[derive(Debug)]
struct Options {
    mode: Mode,
    server: PathBuf,
    pak: PathBuf,
    fixture: PathBuf,
    run_dir: PathBuf,
    opensb_assets: Option<PathBuf>,
    measure_images: bool,
}

impl Options {
    fn parse(args: &[String]) -> Result<Option<Self>, Box<dyn Error>> {
        let mut values = BTreeMap::new();
        let mut measure_images = false;
        let mut index = 0;
        while index < args.len() {
            let argument = &args[index];
            if argument == "--help" || argument == "-h" {
                return Ok(None);
            }
            if argument == "--measure-images" {
                measure_images = true;
                index += 1;
                continue;
            }
            let (name, inline) = argument
                .split_once('=')
                .map_or((argument.as_str(), None), |(a, b)| (a, Some(b)));
            if ![
                "--mode",
                "--server",
                "--pak",
                "--fixture",
                "--run-dir",
                "--opensb-assets",
            ]
            .contains(&name)
            {
                return Err(invalid(format!(
                    "Unknown native verification option: {argument}\n{HELP}"
                ))
                .into());
            }
            let value = if let Some(value) = inline {
                value.to_owned()
            } else {
                index += 1;
                args.get(index)
                    .filter(|s| !s.starts_with("--"))
                    .ok_or_else(|| invalid(format!("Missing value for {name}")))?
                    .clone()
            };
            if value.is_empty() {
                return Err(invalid(format!("Empty value for {name}")).into());
            }
            values.insert(name.to_owned(), value);
            index += 1;
        }
        let mut required = |name: &str| {
            values
                .remove(name)
                .ok_or_else(|| invalid(format!("Required option {name} is missing\n{HELP}")))
        };
        let mode = match required("--mode")?.as_str() {
            "vanilla" => Mode::Vanilla,
            "opensb" => Mode::Opensb,
            other => {
                return Err(invalid(format!("Invalid native verification mode: {other}")).into());
            }
        };
        let server = PathBuf::from(required("--server")?);
        let pak = PathBuf::from(required("--pak")?);
        let fixture = PathBuf::from(required("--fixture")?);
        let run_dir = PathBuf::from(required("--run-dir")?);
        let opensb_assets = values.remove("--opensb-assets").map(PathBuf::from);
        if mode == Mode::Opensb && opensb_assets.is_none() {
            return Err(invalid("--opensb-assets is required for OpenStarbound").into());
        }
        Ok(Some(Self {
            mode,
            server,
            pak,
            fixture,
            run_dir,
            opensb_assets,
            measure_images,
        }))
    }
}

#[derive(Debug)]
struct Case {
    name: String,
    poses: u64,
    variants: u64,
    colors: u64,
}

#[derive(Debug)]
struct Manifest {
    cases: Vec<Case>,
    hats: Vec<hats::HatCase>,
    characters: Vec<characters::CharacterCase>,
}
impl Manifest {
    fn read(path: &Path) -> Result<Self, Box<dyn Error>> {
        Self::parse(&serde_json::from_reader(File::open(path)?)?)
    }

    fn parse(value: &Value) -> Result<Self, Box<dyn Error>> {
        if value["format"] != 1 || value["frameSize"] != 43 {
            return Err(
                invalid("Unsupported native manifest (expected format 1, frameSize 43)").into(),
            );
        }
        let cases = value["cases"]
            .as_array()
            .ok_or_else(|| invalid("Native manifest has no cases"))?;
        let mut names = HashSet::new();
        let mut result = Vec::with_capacity(cases.len());
        for case in cases {
            let name = case["name"]
                .as_str()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| invalid("Native fixture has no name"))?;
            if !names.insert(name) {
                return Err(invalid(format!("Duplicate native fixture: {name}")).into());
            }
            let layers = case["layers"]
                .as_array()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| invalid(format!("No layers in fixture {name}")))?;
            let mut poses = 0u64;
            for layer in layers {
                let layer_poses = layer["poses"]
                    .as_array()
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| invalid(format!("No poses in fixture {name}")))?;
                poses = poses
                    .checked_add(layer_poses.len() as u64)
                    .ok_or_else(|| invalid("Manifest pose count overflow"))?;
            }
            let variants = case["variants"]
                .as_array()
                .ok_or_else(|| invalid(format!("No variants in fixture {name}")))?;
            let mut variant_names = HashSet::new();
            for variant in variants {
                let variant_name = variant["name"]
                    .as_str()
                    .ok_or_else(|| invalid("Missing native variant name"))?;
                if !variant_names.insert(variant_name) || !variant["directives"].is_string() {
                    return Err(invalid(format!(
                        "Invalid or duplicate native variant in {name}: {variant_name}"
                    ))
                    .into());
                }
            }
            if NATIVE_VARIANTS.iter().any(|v| !variant_names.contains(v)) {
                return Err(invalid(format!(
                    "Missing a required native variant in fixture {name}"
                ))
                .into());
            }
            let baseline = case["baseline"]
                .as_str()
                .ok_or_else(|| invalid("Missing native baseline"))?;
            let prefix = case["originalPrefix"]
                .as_str()
                .ok_or_else(|| invalid("Missing native original prefix"))?;
            let tail = baseline.strip_prefix(prefix).ok_or_else(|| {
                invalid(format!(
                    "Baseline does not preserve original prefix: {name}"
                ))
            })?;
            let mut colors = HashSet::new();
            for stage in tail.split('?').filter(|s| s.starts_with("replace;")) {
                for pair in stage[8..].split(';') {
                    let (_, color) = pair
                        .split_once('=')
                        .ok_or_else(|| invalid("Malformed baseline replacement"))?;
                    if !color.bytes().all(|c| c.is_ascii_hexdigit()) {
                        return Err(invalid("Malformed baseline color").into());
                    }
                    let alpha = match color.len() {
                        3 | 6 => 255,
                        4 => u8::from_str_radix(&color[3..], 16)? * 17,
                        8 => u8::from_str_radix(&color[6..], 16)?,
                        _ => return Err(invalid("Malformed baseline color length").into()),
                    };
                    if alpha > 0 {
                        colors.insert(color);
                    }
                }
            }
            result.push(Case {
                name: name.to_owned(),
                poses,
                variants: variants.len() as u64,
                colors: colors.len() as u64,
            });
        }
        let hats = hats::parse(&value["hats"], &mut names)?;
        let characters = characters::parse(
            &value["characters"],
            &value["characterVariants"],
            &mut names,
        )?;
        if result.is_empty() && hats.is_empty() && characters.is_empty() {
            return Err(invalid("Native manifest has no outfit, hat or character cases").into());
        }
        Ok(Self {
            cases: result,
            hats,
            characters,
        })
    }
}

static INTERRUPTED: AtomicI32 = AtomicI32::new(0);
fn check_interrupted() -> io::Result<()> {
    let signal = INTERRUPTED.load(Ordering::Relaxed);
    if signal == 0 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            format!("Native verification interrupted by signal {signal}"),
        ))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    use std::os::unix::process::CommandExt;

    extern "C" fn interrupted(signal: libc::c_int) {
        INTERRUPTED.store(signal, Ordering::Relaxed);
    }

    pub struct Signals {
        previous: Vec<(i32, libc::sigaction)>,
    }
    impl Signals {
        pub fn install() -> io::Result<Self> {
            INTERRUPTED.store(0, Ordering::Relaxed);
            let mut result = Self {
                previous: Vec::new(),
            };
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
                // The handler only stores a lock-free atomic; all I/O and cleanup stay outside it.
                let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
                action.sa_sigaction = interrupted as *const () as usize;
                unsafe {
                    libc::sigemptyset(&mut action.sa_mask);
                }
                let mut previous = unsafe { std::mem::zeroed() };
                if unsafe { libc::sigaction(signal, &action, &mut previous) } != 0 {
                    return Err(io::Error::last_os_error());
                }
                result.previous.push((signal, previous));
            }
            Ok(result)
        }
    }
    impl Drop for Signals {
        fn drop(&mut self) {
            for (signal, previous) in self.previous.iter().rev() {
                unsafe {
                    libc::sigaction(*signal, previous, std::ptr::null_mut());
                }
            }
        }
    }

    pub fn directory(path: &Path) -> io::Result<()> {
        fs::DirBuilder::new().mode(0o700).create(path)
    }
    pub fn private_file(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }
    pub fn symlink(source: &Path, target: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(source, target)
    }

    pub fn constrain(command: &mut Command) -> io::Result<()> {
        let mut allowed: libc::cpu_set_t = unsafe { std::mem::zeroed() };
        if unsafe { libc::sched_getaffinity(0, std::mem::size_of_val(&allowed), &mut allowed) } != 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut chosen: libc::cpu_set_t = unsafe { std::mem::zeroed() };
        let mut count = 0;
        unsafe {
            libc::CPU_ZERO(&mut chosen);
            for cpu in 0..libc::CPU_SETSIZE as usize {
                if libc::CPU_ISSET(cpu, &allowed) {
                    libc::CPU_SET(cpu, &mut chosen);
                    count += 1;
                    if count == 2 {
                        break;
                    }
                }
            }
        }
        if count == 0 {
            return Err(invalid(
                "No allowed CPU is available for native verification",
            ));
        }
        command.process_group(0);
        // Only async-signal-safe system calls run in the forked child.
        unsafe {
            command.pre_exec(move || {
                if libc::sched_setaffinity(0, std::mem::size_of_val(&chosen), &chosen) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(())
    }
    pub fn signal_group(child: &Child, signal: i32) -> io::Result<()> {
        if unsafe { libc::kill(-(child.id() as i32), signal) } == 0 {
            Ok(())
        } else {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::*;
    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "Native verification requires Linux CPU affinity and private process groups",
        )
    }
    pub struct Signals;
    impl Signals {
        pub fn install() -> io::Result<Self> {
            Err(unsupported())
        }
    }
    pub fn directory(_: &Path) -> io::Result<()> {
        Err(unsupported())
    }
    pub fn private_file(_: &Path) -> io::Result<File> {
        Err(unsupported())
    }
    pub fn symlink(_: &Path, _: &Path) -> io::Result<()> {
        Err(unsupported())
    }
    pub fn constrain(_: &mut Command) -> io::Result<()> {
        Err(unsupported())
    }
    pub fn signal_group(_: &Child, _: i32) -> io::Result<()> {
        Err(unsupported())
    }
}

struct Server {
    child: Child,
    reaped: bool,
}
impl Server {
    fn status(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.reaped = true;
        }
        Ok(status)
    }
    fn wait(&mut self, timeout: Duration, interruptible: bool) -> io::Result<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.status()? {
                return Ok(status);
            }
            if interruptible {
                check_interrupted()?;
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Server did not shut down within its deadline",
                ));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn cleanup(&mut self) -> io::Result<()> {
        if self.reaped || self.status()?.is_some() {
            return Ok(());
        }
        platform::signal_group(&self.child, libc::SIGINT)?;
        if self.wait(Duration::from_secs(15), false).is_err() {
            platform::signal_group(&self.child, libc::SIGKILL)?;
            self.child.wait()?;
            self.reaped = true;
        }
        Ok(())
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if !self.reaped && self.cleanup().is_err() {
            // Last resort still reaps the owned process even if group signalling failed.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct Rcon {
    stream: TcpStream,
    sequence: i32,
}
struct Packet {
    id: i32,
    kind: i32,
    body: Vec<u8>,
}
impl Rcon {
    fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            sequence: 0,
        })
    }
    fn io_timeout(&self, deadline: Instant) -> io::Result<()> {
        check_interrupted()?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "RCON operation exceeded its 600 second deadline",
                )
            })?;
        let slice = remaining.min(IO_SLICE);
        self.stream.set_read_timeout(Some(slice))?;
        self.stream.set_write_timeout(Some(slice))
    }
    fn exact(&mut self, mut bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
        while !bytes.is_empty() {
            self.io_timeout(deadline)?;
            match self.stream.read(bytes) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "RCON closed before completing a response",
                    ));
                }
                Ok(n) => {
                    bytes = &mut bytes[n..];
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::Interrupted
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn send(&mut self, kind: i32, text: &str, deadline: Instant) -> io::Result<i32> {
        if text.contains('\0') || text.len() > MAX_PACKET - 10 {
            return Err(invalid("Invalid RCON request body"));
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("RCON request sequence exhausted"))?;
        let mut packet = Vec::with_capacity(text.len() + 14);
        packet.extend_from_slice(&((text.len() + 10) as i32).to_le_bytes());
        packet.extend_from_slice(&self.sequence.to_le_bytes());
        packet.extend_from_slice(&kind.to_le_bytes());
        packet.extend_from_slice(text.as_bytes());
        packet.extend_from_slice(&[0, 0]);
        let mut bytes = packet.as_slice();
        while !bytes.is_empty() {
            self.io_timeout(deadline)?;
            match self.stream.write(bytes) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "RCON closed while sending a request",
                    ));
                }
                Ok(n) => bytes = &bytes[n..],
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::Interrupted
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(self.sequence)
    }
    fn receive(&mut self, deadline: Instant) -> io::Result<Packet> {
        let mut length = [0; 4];
        self.exact(&mut length, deadline)?;
        let size = i32::from_le_bytes(length);
        if size < 10 || size as usize > MAX_PACKET {
            return Err(invalid(format!("Invalid RCON packet size: {size}")));
        }
        let mut header = [0; 8];
        self.exact(&mut header, deadline)?;
        let id = i32::from_le_bytes(
            header[..4]
                .try_into()
                .map_err(|_| invalid("Missing RCON response ID"))?,
        );
        let kind = i32::from_le_bytes(
            header[4..]
                .try_into()
                .map_err(|_| invalid("Missing RCON response kind"))?,
        );
        let mut body = vec![0; size as usize - 10];
        self.exact(&mut body, deadline)?;
        let mut terminator = [0; 2];
        self.exact(&mut terminator, deadline)?;
        if terminator != [0, 0] {
            return Err(invalid("Invalid RCON response terminator"));
        }
        Ok(Packet { id, kind, body })
    }
    fn authenticate(&mut self, password: &str) -> io::Result<()> {
        let deadline = Instant::now() + OPERATION_TIMEOUT;
        let id = self.send(3, password, deadline)?;
        loop {
            let packet = self.receive(deadline)?;
            if packet.id == -1 {
                return Err(invalid("Private RCON authentication failed"));
            }
            if packet.id != id || ![0, 2].contains(&packet.kind) || !packet.body.is_empty() {
                return Err(invalid("Mismatched RCON authentication response"));
            }
            if packet.kind == 2 {
                return Ok(());
            }
        }
    }
    fn response(&mut self, id: i32, deadline: Instant) -> io::Result<Packet> {
        let packet = self.receive(deadline)?;
        if packet.id != id || packet.kind != 0 {
            return Err(invalid(format!(
                "Unexpected RCON command response: {}, {}, {}",
                packet.id,
                packet.kind,
                String::from_utf8_lossy(&packet.body)
            )));
        }
        Ok(packet)
    }
    fn command(&mut self, text: &str) -> io::Result<String> {
        self.timed_command(text).map(|(body, _)| body)
    }
    fn timed_command(&mut self, text: &str) -> io::Result<(String, Duration)> {
        let started = Instant::now();
        let deadline = Instant::now() + OPERATION_TIMEOUT;
        let id = self.send(2, text, deadline)?;
        let mut bytes = self.response(id, deadline)?.body;
        let mut elapsed = started.elapsed();
        // Starbound provides no length/terminator for a multi-packet response. Send
        // an ordered native echo only AFTER its first packet: asynchronous eval
        // promises may otherwise finish after an eagerly pipelined echo response.
        let boundary = "outfit-response-boundary";
        let fence = self.send(2, &format!("echo {boundary}"), deadline)?;
        loop {
            let packet = self.receive(deadline)?;
            if packet.id == fence && packet.kind == 0 && packet.body == boundary.as_bytes() {
                break;
            }
            if packet.id != id || packet.kind != 0 {
                return Err(invalid(
                    "Unexpected RCON response while collecting command chunks",
                ));
            }
            if bytes.len().saturating_add(packet.body.len()) > MAX_PACKET {
                return Err(invalid("RCON response exceeds 16 MiB"));
            }
            bytes.extend_from_slice(&packet.body);
            elapsed = started.elapsed();
        }
        let body = String::from_utf8(bytes)
            .map_err(|e| invalid(format!("Invalid UTF-8 RCON response: {e}")))?;
        Ok((body, elapsed))
    }
    fn stop(&mut self) -> io::Result<()> {
        let deadline = Instant::now() + OPERATION_TIMEOUT;
        let id = self.send(2, "stop", deadline)?;
        let packet = self.response(id, deadline)?;
        let body = String::from_utf8(packet.body)
            .map_err(|e| invalid(format!("Invalid shutdown response: {e}")))?;
        if body.trim() != "OK: shutting down" {
            return Err(invalid(format!("Unexpected shutdown response: {body}")));
        }
        Ok(())
    }
}

fn free_port() -> io::Result<u16> {
    Ok(TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?
        .local_addr()?
        .port())
}

fn write_json(path: &Path, value: &Value) -> Result<(), Box<dyn Error>> {
    let mut file = platform::private_file(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn prepare(options: &Options) -> Result<(PathBuf, PathBuf, u16, String), Box<dyn Error>> {
    let server = options.server.canonicalize()?;
    let pak = options.pak.canonicalize()?;
    let fixture = options.fixture.canonicalize()?;
    let opensb = options
        .opensb_assets
        .as_ref()
        .map(|p| p.canonicalize())
        .transpose()?;
    if !server.is_file()
        || !pak.is_file()
        || !fixture.is_dir()
        || opensb.as_ref().is_some_and(|p| !p.is_dir())
    {
        return Err(invalid(
            "Server/pak must be files and fixture/OpenStarbound assets must be directories",
        )
        .into());
    }
    platform::directory(&options.run_dir)?; // Never reuses storage, follows no existing run-dir symlink.
    let run = options.run_dir.canonicalize()?;
    let game_port = free_port()?;
    let mut rcon_port = free_port()?;
    while rcon_port == game_port {
        rcon_port = free_port()?;
    }
    let mut random = [0u8; 24];
    getrandom::fill(&mut random)
        .map_err(|e| io::Error::other(format!("Cannot generate private RCON password: {e}")))?;
    let mut password = String::with_capacity(48);
    use std::fmt::Write as _;
    for byte in random {
        write!(&mut password, "{byte:02x}")?;
    }
    let sources = run.join("asset-sources");
    platform::directory(&sources)?;
    platform::symlink(&pak, &sources.join("base.pak"))?;
    platform::symlink(&fixture, &sources.join("fixture"))?;
    if let Some(opensb) = opensb {
        platform::symlink(&opensb, &sources.join("opensb"))?;
    }
    let storage = run.join("storage");
    platform::directory(&storage)?;
    write_json(
        &run.join("boot.config"),
        &json!({ "assetDirectories": [sources], "storageDirectory": storage }),
    )?;
    write_json(
        &storage.join("starbound_server.config"),
        &json!({
            "configurationVersion": {"basic": 2, "server": 4},
            "allowAdminCommands": true, "allowAdminCommandsFromAnyone": false,
            "allowAnonymousConnections": false, "anonymousConnectionsAreAdmin": false,
            "allowAssetsMismatch": true, "checkAssetsDigest": false,
            "bannedIPs": [], "bannedUuids": [], "serverUsers": {},
            "clearPlayerFiles": false, "clearUniverseFiles": false,
            "clientIPJoinable": false, "clientP2PJoinable": false,
            "maxPlayers": 8, "maxTeamSize": 4, "playerBackupFileCount": 3,
            "safeScripts": true, "scriptInstructionLimit": 10000000,
            "scriptInstructionMeasureInterval": 10000, "scriptProfilingEnabled": false,
            "scriptRecursionLimit": 100, "serverFidelity": "automatic",
            "serverName": "Private outfit verification", "serverOverrideAssetsDigest": null,
            "interactiveHighlight": true, "monochromeLighting": false, "tutorialMessages": true,
            "crafting": {"filterHaveMaterials": false}, "inventory": {"pickupToActionBar": true},
            "queryServerBind": "127.0.0.1", "queryServerPort": free_port()?,
            "gameServerBind": "127.0.0.1", "gameServerPort": game_port,
            "runQueryServer": false, "runRconServer": true,
            "rconServerBind": "127.0.0.1", "rconServerPort": rcon_port,
            "rconServerPassword": password, "rconServerTimeout": 600000
        }),
    )?;
    Ok((run, server, rcon_port, password))
}

fn connect(server: &mut Server, port: u16) -> Result<Rcon, Box<dyn Error>> {
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    loop {
        check_interrupted()?;
        if let Some(status) = server.status()? {
            return Err(invalid(format!("Server exited during startup: {status}")).into());
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Server startup timed out"))?;
        match TcpStream::connect_timeout(&address, remaining.min(Duration::from_millis(500))) {
            Ok(stream) => return Ok(Rcon::new(stream)?),
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("Server startup timed out: {error}"),
                    )
                    .into());
                }
                thread::sleep(IO_SLICE.min(deadline.saturating_duration_since(Instant::now())));
            }
        }
    }
}

fn parse_batch(
    response: &str,
    case: &Case,
    first_variant: usize,
    variant_count: usize,
) -> Result<Value, Box<dyn Error>> {
    let mut batch: Value = serde_json::from_str(response).map_err(|e| {
        invalid(format!(
            "Native case failed: {}: {e}: {response}",
            case.name
        ))
    })?;
    if let Some(inner) = batch.as_str() {
        batch = serde_json::from_str(inner).map_err(|e| {
            invalid(format!(
                "Native case failed: {}: {e}: {response}",
                case.name
            ))
        })?;
    }
    if !batch.is_object()
        || batch["fixture"] != case.name
        || batch["frames"] != 1
        || batch["firstVariant"] != first_variant
        || batch["variants"] != variant_count
        || batch["colorChecks"].as_u64() != case.colors.checked_mul(variant_count as u64)
    {
        return Err(invalid(format!(
            "Unexpected native batch result for {}: {response}",
            case.name
        ))
        .into());
    }
    batch["transparentProbes"]
        .as_u64()
        .filter(|n| *n <= 3 * variant_count as u64 && *n % variant_count as u64 == 0)
        .ok_or_else(|| {
            invalid(format!(
                "Invalid transparent-probe result for {}: {response}",
                case.name
            ))
        })?;
    Ok(batch)
}

fn output_line(
    output: &mut impl Write,
    prefix: &str,
    value: &impl std::fmt::Display,
) -> io::Result<()> {
    writeln!(output, "{prefix} {value}")?;
    output.flush()
}

fn measure(client: &mut Rcon, manifest: &Manifest) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut measurements = Vec::with_capacity(manifest.cases.len() * MEASURE_VARIANTS.len());
    for (zero_index, case) in manifest.cases.iter().enumerate() {
        let index = zero_index + 1;
        let mut expected = None;
        for offset in 0..MEASURE_VARIANTS.len() {
            let name = MEASURE_VARIANTS[(index + offset) % MEASURE_VARIANTS.len()];
            let mut durations = [0.0; 6];
            for duration in &mut durations {
                // Exclude the response-boundary echo from the actual query timing.
                let (response, elapsed) = client.timed_command(&format!(
                    "eval outfitImageQuery({index},{})",
                    serde_json::to_string(name)?
                ))?;
                *duration = elapsed.as_secs_f64() * 1000.0;
                let spaces = response.trim().parse::<u64>().map_err(|e| {
                    invalid(format!("Native image measurement failed: {response}: {e}"))
                })?;
                if let Some(expected) = expected {
                    if spaces != expected {
                        return Err(invalid(format!(
                            "Native benchmark occupancy differs: {}/{name}",
                            case.name
                        ))
                        .into());
                    }
                } else {
                    expected = Some(spaces);
                }
            }
            let mut repeated = [
                durations[1],
                durations[2],
                durations[3],
                durations[4],
                durations[5],
            ];
            repeated.sort_by(f64::total_cmp);
            measurements.push(json!({ "fixture": case.name, "variant": name,
                "firstRequestMs": durations[0], "repeatMedianMs": repeated[2],
                "repeatSamplesMs": &durations[1..], "spaces": expected }));
        }
    }
    Ok(measurements)
}

fn ensure_no_fail(log: &str) -> io::Result<()> {
    if log.contains("OUTFIT_VERIFY_FAIL") || log.contains("OUTFIT_VANILLA_FAIL") {
        return Err(invalid(
            "Native Lua probe logged FAIL (including unoptimized baseline failures); inspect retained logs",
        ));
    }
    if log.contains("[Error]") {
        return Err(invalid(
            "Native engine logged an Error; inspect retained logs",
        ));
    }
    Ok(())
}

fn opensb_marker(log: &str, manifest: &Manifest) -> Result<String, Box<dyn Error>> {
    let markers: Vec<_> = log
        .lines()
        .filter(|line| line.contains("OUTFIT_VERIFY_PASS "))
        .collect();
    if markers.len() != 1 {
        return Err(invalid("Missing unique native RGBA PASS marker").into());
    }
    let marker = markers[0];
    let payload = marker
        .split_once("OUTFIT_VERIFY_PASS ")
        .ok_or_else(|| invalid("Missing native RGBA PASS payload"))?
        .1;
    let mut counts = BTreeMap::new();
    for field in payload.split_whitespace() {
        let (key, value) = field
            .split_once('=')
            .ok_or_else(|| invalid("Malformed native RGBA PASS field"))?;
        if counts.insert(key, value.parse::<u64>()?).is_some() {
            return Err(invalid("Duplicate native RGBA PASS field").into());
        }
    }
    let frames: u64 = manifest.cases.iter().map(|c| c.poses).sum();
    let variants: u64 = manifest.cases.iter().map(|c| c.poses * c.variants).sum();
    let pixels = variants
        .checked_mul(43 * 43)
        .ok_or_else(|| invalid("Native pixel count overflow"))?;
    let channels = pixels
        .checked_mul(4)
        .ok_or_else(|| invalid("Native channel count overflow"))?;
    let expected = [
        ("fixtures", manifest.cases.len() as u64),
        ("frames", frames),
        ("variants", variants),
        ("pixels", pixels),
        ("channels", channels),
    ];
    if counts.len() != 6
        || expected
            .iter()
            .any(|(key, count)| counts.get(key) != Some(count))
        || counts
            .get("transparent")
            .is_none_or(|n| *n > frames.saturating_mul(43 * 43))
    {
        return Err(invalid(format!(
            "Incomplete or unexpected native RGBA PASS counts: {payload}"
        ))
        .into());
    }
    Ok(marker.to_owned())
}

fn execute(
    options: &Options,
    manifest: &Manifest,
    run: &Path,
    server: &mut Server,
    port: u16,
    password: &str,
    output: &mut impl Write,
) -> Result<(), Box<dyn Error>> {
    let mut client = connect(server, port)?;
    client.authenticate(password)?;
    let mut results = Vec::new();
    let mut hat_results = Vec::new();
    let mut character_results = Vec::new();
    let mut batches = 0u64;
    let mut measurements = Vec::new();
    if options.mode == Mode::Vanilla {
        let loaded = client.command("eval require('/vanilla-verify.lua')")?;
        output_line(output, "VANILLA_PROBE_LOAD", &loaded)?;
        if loaded.contains("OUTFIT_VANILLA_FAIL")
            || loaded.contains("Error executing")
            || loaded.contains("Command promise failed")
        {
            return Err(invalid(format!("Cannot load vanilla probe: {loaded}")).into());
        }
        if options.measure_images {
            measurements = measure(&mut client, manifest)?;
            output_line(
                output,
                "OUTFIT_IMAGE_MEASUREMENTS",
                &serde_json::to_string(&measurements)?,
            )?;
        }
        for (zero_index, case) in manifest.cases.iter().enumerate() {
            let index = zero_index + 1;
            let mut color_checks = 0u64;
            let mut transparent_probes = 0u64;
            for pose in 1..=case.poses {
                for first in (1..=NATIVE_VARIANTS.len()).step_by(VARIANTS_PER_BATCH) {
                    let last = (first + VARIANTS_PER_BATCH - 1).min(NATIVE_VARIANTS.len());
                    let response = client.command(&format!(
                        "eval outfitVanillaVerify({index},{pose},{pose},{first},{last})"
                    ))?;
                    let batch =
                        parse_batch(&response, case, first, last - first + 1).map_err(|e| {
                            invalid(format!(
                                "{} pose {pose}, variants {first}-{last}: {e}",
                                case.name
                            ))
                        })?;
                    color_checks += batch["colorChecks"]
                        .as_u64()
                        .ok_or_else(|| invalid("Missing batch color checks"))?;
                    transparent_probes += batch["transparentProbes"]
                        .as_u64()
                        .ok_or_else(|| invalid("Missing batch transparent probes"))?;
                    batches += 1;
                }
            }
            let result = json!({"fixture": case.name, "frames": case.poses, "colorChecks": color_checks, "transparentProbes": transparent_probes, "variants": NATIVE_VARIANTS, "verifiedFrameVariants":case.poses * NATIVE_VARIANTS.len() as u64});
            output_line(output, "OUTFIT_VANILLA_CASE_PASS", &result)?;
            results.push(result);
        }
        hat_results = hats::run(&mut client, &manifest.hats, output)?;
        character_results = characters::run(&mut client, &manifest.characters, output)?;
    }
    client.stop()?;
    drop(client);
    let status = server.wait(Duration::from_secs(30), true)?;
    let log = fs::read_to_string(run.join("console.log"))?;
    ensure_no_fail(&log)?;
    // Keep both evidence channels; either one reporting a real failure rejects the run.
    if run.join("engine.log").exists() {
        ensure_no_fail(&fs::read_to_string(run.join("engine.log"))?)?;
    }
    if !status.success() {
        return Err(invalid(format!(
            "Native server exited unsuccessfully after stop: {status}"
        ))
        .into());
    }
    let evidence = if options.mode == Mode::Opensb {
        let marker = if manifest.cases.is_empty() {
            None
        } else {
            let marker = opensb_marker(&log, manifest)?;
            writeln!(output, "{marker}")?;
            Some(marker)
        };
        let hat_marker = if manifest.hats.is_empty() {
            None
        } else {
            let marker = hats::opensb_marker(&log, &manifest.hats)?;
            writeln!(output, "{marker}")?;
            Some(marker)
        };
        output.flush()?;
        let character_marker = if manifest.characters.is_empty() {
            None
        } else {
            let marker = characters::opensb_marker(&log, &manifest.characters)?;
            writeln!(output, "{marker}")?;
            Some(marker)
        };
        json!({ "mode": options.mode.key(), "marker": marker, "hatMarker": hat_marker, "characterMarker": character_marker })
    } else {
        let marker_count = log.matches("OUTFIT_VANILLA_BATCH_PASS ").count() as u64;
        let expected_batches: u64 = manifest.cases.iter().map(|c| c.poses).sum::<u64>()
            * NATIVE_VARIANTS.len().div_ceil(VARIANTS_PER_BATCH) as u64;
        if results.len() != manifest.cases.len()
            || batches != expected_batches
            || marker_count != batches
            || hat_results.len() != manifest.hats.len()
            || log.matches("OUTFIT_VANILLA_HAT_BATCH_PASS ").count() as u64
                != hats::expected_batches(&manifest.hats)
            || character_results.len() != manifest.characters.len()
            || log.matches("OUTFIT_VANILLA_CHARACTER_BATCH_PASS ").count() as u64
                != characters::expected_batches(&manifest.characters)
        {
            return Err(invalid(format!("Not every native vanilla case reached its PASS marker: expected {expected_batches}, responses {batches}, log markers {marker_count}")).into());
        }
        let evidence = json!({"mode": options.mode.key(), "batches": batches, "cases": results,
            "fixtures":manifest.cases.len(),"mappedFrames":manifest.cases.iter().map(|c| c.poses).sum::<u64>(),
            "nativeVariants":NATIVE_VARIANTS,"verifiedFrameVariants":manifest.cases.iter().map(|c| c.poses).sum::<u64>() * NATIVE_VARIANTS.len() as u64,
            "evidenceScope":"Original vanilla imageSpaces occupancy and exact visible-color footprints against original/off and source assets, plus corner/center transparent-address RGB probes; not full-RGBA engine readback.",
            "hats": hat_results, "characters": character_results, "imageMeasurements": measurements, "measurementScope": MEASUREMENT_SCOPE});
        output_line(output, "OUTFIT_VANILLA_PASS", &evidence)?;
        evidence
    };
    write_json(&run.join("result.json"), &evidence)?;
    Ok(())
}

/// Run the native-verify CLI subcommand. Never executes a shell or touches an existing storage directory.
pub fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let Some(options) = Options::parse(args)? else {
        writeln!(io::stdout().lock(), "{HELP}")?;
        return Ok(());
    };
    let _signals = platform::Signals::install()?;
    let manifest = Manifest::read(&options.fixture.join("outfit-verification.json"))?;
    let (run, executable, port, password) = prepare(&options)?;
    let console = platform::private_file(&run.join("console.log"))?;
    let mut command = Command::new(executable);
    command
        .arg("-bootconfig")
        .arg(run.join("boot.config"))
        .arg("-logfile")
        .arg(run.join("engine.log"))
        .current_dir(&run)
        .stdin(Stdio::null())
        .stdout(Stdio::from(console.try_clone()?))
        .stderr(Stdio::from(console));
    platform::constrain(&mut command)?;
    let child = command.spawn().map_err(|e| {
        invalid(format!(
            "Cannot launch native server: {e}; private run: {}",
            run.display()
        ))
    })?;
    let mut server = Server {
        child,
        reaped: false,
    };
    let result = execute(
        &options,
        &manifest,
        &run,
        &mut server,
        port,
        &password,
        &mut io::stdout().lock(),
    );
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            let cleanup = server.cleanup();
            let suffix = cleanup
                .err()
                .map(|e| format!("; cleanup also failed: {e}"))
                .unwrap_or_default();
            Err(invalid(format!(
                "{error}{suffix}; retained evidence: {} and {}",
                run.join("console.log").display(),
                run.join("engine.log").display()
            ))
            .into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Value {
        json!({"format":1,"frameSize":43,"cases":[{"name":"sample","originalPrefix":"?prefix","baseline":"?prefix?replace;00010000=f00;01010000=0f08;02010000=ff00",
            "layers":[{"poses":[{"name":"idle","id":1}]}],
            "variants":NATIVE_VARIANTS.map(|name| json!({"name":name,"directives":"?prefix"}))}]})
    }

    #[test]
    fn validates_manifest_and_real_batch_counts() {
        let manifest = Manifest::parse(&manifest()).unwrap();
        let case = &manifest.cases[0];
        assert_eq!(case.colors, 2);
        let batch = r#"{"fixture":"sample","frames":1,"firstVariant":5,"variants":4,"colorChecks":8,"transparentProbes":12}"#;
        assert!(parse_batch(batch, case, 5, 4).is_ok());
        assert!(parse_batch(&serde_json::to_string(batch).unwrap(), case, 5, 4).is_ok());
        assert!(parse_batch(batch, case, 1, 4).is_err());
        assert!(parse_batch(batch, case, 5, 3).is_err());
        assert!(
            parse_batch(
                &batch.replace("\"colorChecks\":8", "\"colorChecks\":0"),
                case,
                5,
                4
            )
            .is_err()
        );
        assert!(
            parse_batch(
                &batch.replace("\"transparentProbes\":12", "\"transparentProbes\":1"),
                case,
                5,
                4
            )
            .is_err()
        );
        let mut incomplete = super::tests::manifest();
        incomplete["cases"][0]["variants"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(Manifest::parse(&incomplete).is_err());
        assert!(Manifest::parse(&json!({"format":1,"frameSize":43,"cases":[]})).is_err());
    }

    #[test]
    fn exit_zero_is_not_a_probe_pass() {
        let manifest = Manifest::parse(&manifest()).unwrap();
        assert!(opensb_marker("normal successful shutdown", &manifest).is_err());
        let marker = "[Info] OUTFIT_VERIFY_PASS fixtures=1 frames=1 variants=8 pixels=14792 channels=59168 transparent=1";
        assert_eq!(opensb_marker(marker, &manifest).unwrap(), marker);
        assert!(opensb_marker(&format!("{marker}\n{marker}"), &manifest).is_err());
        assert!(opensb_marker(&marker.replace("frames=1", "frames=2"), &manifest).is_err());
        assert!(
            ensure_no_fail(&format!(
                "{marker}\nOUTFIT_VERIFY_FAIL original/off desired"
            ))
            .is_err()
        );
        assert!(ensure_no_fail("OUTFIT_VANILLA_FAIL occupancy").is_err());
        assert!(ensure_no_fail("[Error] unrelated native failure").is_err());
    }

    #[test]
    fn options_keep_required_mode_and_private_run_contract() {
        let strings = |values: &[&str]| values.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let vanilla = strings(&[
            "--mode=vanilla",
            "--server",
            "server",
            "--pak",
            "pak",
            "--fixture",
            "fixture",
            "--run-dir",
            "new",
            "--measure-images",
        ]);
        let options = Options::parse(&vanilla).unwrap().unwrap();
        assert_eq!(options.mode, Mode::Vanilla);
        assert!(options.measure_images);
        let mut opensb = vanilla.clone();
        opensb[0] = "--mode=opensb".to_owned();
        assert!(Options::parse(&opensb).is_err());
        assert!(Options::parse(&strings(&["--server"])).is_err());
        assert!(Options::parse(&strings(&["--unknown"])).is_err());
    }

    fn test_packet(stream: &mut TcpStream, id: i32, kind: i32, body: &[u8]) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&((body.len() + 10) as i32).to_le_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(&[0, 0]);
        // Exercise exact reads across both header and body boundaries.
        for byte in bytes {
            stream.write_all(&[byte]).unwrap();
        }
    }

    fn test_request(stream: &mut TcpStream) -> (i32, i32, String) {
        let mut header = [0; 4];
        stream.read_exact(&mut header).unwrap();
        let mut packet = vec![0; i32::from_le_bytes(header) as usize];
        stream.read_exact(&mut packet).unwrap();
        (
            i32::from_le_bytes(packet[..4].try_into().unwrap()),
            i32::from_le_bytes(packet[4..8].try_into().unwrap()),
            String::from_utf8(packet[8..packet.len() - 2].to_vec()).unwrap(),
        )
    }

    #[test]
    fn little_endian_authentication_and_chunked_utf8_responses() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let (id, kind, password) = test_request(&mut stream);
            assert_eq!((kind, password.as_str()), (3, "private"));
            test_packet(&mut stream, id, 0, b"");
            test_packet(&mut stream, id, 2, b"");
            let (id, kind, command) = test_request(&mut stream);
            assert_eq!((kind, command.as_str()), (2, "eval realProbe()"));
            test_packet(
                &mut stream,
                id,
                0,
                &[b'{', b'"', b'x', b'"', b':', b'"', 0xc3],
            );
            let (fence, kind, command) = test_request(&mut stream);
            assert_eq!(
                (kind, command.as_str()),
                (2, "echo outfit-response-boundary")
            );
            test_packet(&mut stream, id, 0, &[0xa9, b'"', b'}']);
            test_packet(&mut stream, fence, 0, b"outfit-response-boundary");
        });
        let mut client = Rcon::new(TcpStream::connect(address).unwrap()).unwrap();
        client.authenticate("private").unwrap();
        assert_eq!(client.command("eval realProbe()").unwrap(), "{\"x\":\"é\"}");
        peer.join().unwrap();
    }

    #[test]
    fn rejects_malformed_lengths_and_expired_deadlines() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(&9i32.to_le_bytes()).unwrap();
        });
        let mut client = Rcon::new(TcpStream::connect(address).unwrap()).unwrap();
        assert!(
            client
                .receive(Instant::now() + Duration::from_secs(5))
                .is_err()
        );
        assert!(client.receive(Instant::now()).is_err());
        peer.join().unwrap();
    }

    #[test]
    fn rejects_authentication_failure_and_wrong_response_ids() {
        for authenticated in [false, true] {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let address = listener.local_addr().unwrap();
            let peer = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let (id, _, _) = test_request(&mut stream);
                if authenticated {
                    test_packet(&mut stream, id + 1, 2, b"");
                } else {
                    test_packet(&mut stream, -1, 2, b"");
                }
            });
            let mut client = Rcon::new(TcpStream::connect(address).unwrap()).unwrap();
            assert!(client.authenticate("private").is_err());
            peer.join().unwrap();
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn private_storage_refuses_reuse_without_modifying_existing_files() {
        use std::os::unix::fs::PermissionsExt;
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let name = format!(
            "outfit-native-private-{}-{}",
            std::process::id(),
            u64::from_le_bytes(random)
        );
        let path = std::env::temp_dir().join(name);
        platform::directory(&path).unwrap();
        struct RemoveDirectory(PathBuf);
        impl Drop for RemoveDirectory {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _remove = RemoveDirectory(path.clone());
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
        let marker = path.join("existing-private-data");
        platform::private_file(&marker)
            .unwrap()
            .write_all(b"unchanged")
            .unwrap();
        assert_eq!(
            fs::metadata(&marker).unwrap().permissions().mode() & 0o077,
            0
        );
        assert!(platform::directory(&path).is_err());
        assert!(platform::private_file(&marker).is_err());
        assert_eq!(fs::read(&marker).unwrap(), b"unchanged");
    }
}

use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{self, Command};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

type Result<T, E = Box<dyn Error>> = std::result::Result<T, E>;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace parent");
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("build") => {
            if args.next().is_some() {
                return Err("build does not take arguments".into());
            }
            build(root)
        }
        Some("serve") => {
            let mut port = 4200;
            let mut rebuild = true;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--port" => port = args.next().ok_or("--port requires a number")?.parse()?,
                    "--no-build" => rebuild = false,
                    _ => return Err(format!("unknown serve option: {arg}").into()),
                }
            }
            if rebuild {
                build(root)?;
            }
            serve(&root.join("dist"), port)
        }
        None | Some("help" | "--help" | "-h") => {
            println!(
                "cargo xtask build\n  Build the Rust/WASM static site into dist/.\ncargo xtask serve [--port 4200] [--no-build]\n  Build and serve locally on 127.0.0.1; PNG files stay in the browser."
            );
            Ok(())
        }
        Some(other) => Err(format!("unknown task: {other}; use cargo xtask --help").into()),
    }
}

fn checked(command: &mut Command) -> Result<()> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{command:?} failed: {status}").into());
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            return Err(io::Error::other(format!(
                "refusing non-regular asset: {}",
                entry.path().display()
            )));
        }
    }
    Ok(())
}

fn build(root: &Path) -> Result<()> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    checked(Command::new(cargo).current_dir(root).args([
        "build",
        "--locked",
        "--release",
        "--target",
        "wasm32-unknown-unknown",
        "--package",
        "outfit-web",
    ]))?;
    let target = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let target = if target.is_absolute() {
        target
    } else {
        root.join(target)
    };
    let next = root.join(".dist-next");
    if next.exists() {
        fs::remove_dir_all(&next)?;
    }
    fs::create_dir_all(next.join("pkg"))?;
    checked(
        Command::new("wasm-bindgen")
            .current_dir(root)
            .arg(target.join("wasm32-unknown-unknown/release/outfit_web.wasm"))
            .args(["--target", "web", "--out-dir"])
            .arg(next.join("pkg")),
    )?;
    fs::copy(root.join("web/index.html"), next.join("index.html"))?;
    fs::copy(root.join("web/style.css"), next.join("style.css"))?;
    fs::copy(root.join("LICENSE"), next.join("LICENSE"))?;
    copy_tree(&root.join("public/assets"), &next.join("assets"))?;
    let dist = root.join("dist");
    let previous = root.join(".dist-previous");
    if previous.exists() {
        fs::remove_dir_all(&previous)?;
    }
    if dist.exists() {
        fs::rename(&dist, &previous)?;
    }
    if let Err(error) = fs::rename(&next, &dist) {
        if previous.exists() {
            fs::rename(&previous, &dist)?;
        }
        return Err(error.into());
    }
    if previous.exists() {
        fs::remove_dir_all(previous)?;
    }
    println!("Built {}", dist.display());
    Ok(())
}

fn serve(root: &Path, port: u16) -> Result<()> {
    let root = root.canonicalize()?;
    if !root.join("index.html").is_file() {
        return Err("dist/index.html is missing; run cargo xtask build".into());
    }
    let server =
        Server::http(("127.0.0.1", port)).map_err(|error| io::Error::other(error.to_string()))?;
    println!("Serving http://127.0.0.1:{port}");
    for request in server.incoming_requests() {
        if let Err(error) = respond(request, &root) {
            eprintln!("HTTP response failed: {error}");
        }
    }
    Ok(())
}

fn respond(request: Request, root: &Path) -> io::Result<()> {
    if request.method() != &Method::Get && request.method() != &Method::Head {
        return request.respond(Response::empty(StatusCode(405)));
    }
    let path = request.url().split('?').next().unwrap_or("/");
    let decoded = match percent_encoding::percent_decode_str(path).decode_utf8() {
        Ok(path) => path,
        Err(_) => return request.respond(Response::empty(StatusCode(400))),
    };
    let relative = Path::new(decoded.strip_prefix('/').unwrap_or(&decoded));
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return request.respond(Response::empty(StatusCode(403)));
    }
    let path = root.join(if relative.as_os_str().is_empty() {
        Path::new("index.html")
    } else {
        relative
    });
    let path = match path.canonicalize() {
        Ok(path) if path.starts_with(root) && path.is_file() => path,
        _ => return request.respond(Response::empty(StatusCode(404))),
    };
    let content_type = match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    };
    let header = Header::from_bytes("Content-Type", content_type).expect("static HTTP header");
    let cache = Header::from_bytes("Cache-Control", "no-cache").expect("static HTTP header");
    request.respond(
        Response::from_file(File::open(path)?)
            .with_header(header)
            .with_header(cache),
    )
}

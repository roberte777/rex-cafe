use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use tokio::sync::mpsc;

use rex_cafe::app::App;
use rex_cafe::engine::{Canvas, Rgb};
use rex_cafe::replay::{WALL, replay};
use rex_cafe::settings::Settings;
use rex_cafe::view::{self, Ui};
use rex_cafe::{demo, rex};

const HELP: &str = "\
rex-cafe: a pixel-art café where every agent on your Rex server is a Clawd

Usage: rex-cafe [--demo] [--quiet] [--spawn-cmd <command>]

  --demo                 Play made-up agents instead of reading Rex
  --quiet                Start with the sound muted (m toggles it, v adjusts it)
  --spawn-cmd <command>  What `n` and the door open beside the café (default: claude)
  --dump <dir>           Write demo frames at --at seconds as .ppm and .txt, then exit
  --at <secs,...>        Times for --dump (default: 0,8,20,40)
  --size <cols>x<rows>   Terminal size for --dump (default: 160x56)
  --sprites <dir>        Write every sprite as an enlarged .ppm, then exit
  --dump-audio <file>    Write a minute of the café's sound, at your volume levels, as a .wav, then exit
  -h, --help             Show this help

Keys: tab/arrows select · enter jump to the agent · n new agent · m mute · v volume · r resync · q quit
Mouse: click a Clawd or its Legend row to jump to it; click the door for a new agent";

struct Args {
    demo: bool,
    spawn_cmd: Vec<String>,
    dump: Option<PathBuf>,
    at: Vec<f64>,
    size: (u16, u16),
    sprites: Option<PathBuf>,
    quiet: bool,
    dump_audio: Option<PathBuf>,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        demo: false,
        spawn_cmd: vec!["claude".into()],
        dump: None,
        at: vec![0.0, 8.0, 20.0, 40.0],
        size: (160, 56),
        sprites: None,
        quiet: false,
        dump_audio: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--demo" => args.demo = true,
            "--quiet" => args.quiet = true,
            "--dump-audio" => {
                args.dump_audio = Some(it.next().context("--dump-audio needs a file")?.into())
            }
            "--spawn-cmd" => {
                let cmd = it.next().context("--spawn-cmd needs a command")?;
                args.spawn_cmd = cmd.split_whitespace().map(str::to_string).collect();
                if args.spawn_cmd.is_empty() {
                    bail!("--spawn-cmd needs a command");
                }
            }
            "--dump" => args.dump = Some(it.next().context("--dump needs a directory")?.into()),
            "--at" => {
                let list = it.next().context("--at needs times")?;
                args.at = list
                    .split(',')
                    .map(|s| s.trim().parse::<f64>())
                    .collect::<Result<_, _>>()?;
                args.at.sort_by(f64::total_cmp);
            }
            "--sprites" => {
                args.sprites = Some(it.next().context("--sprites needs a directory")?.into())
            }
            "--size" => {
                let size = it.next().context("--size needs <cols>x<rows>")?;
                let (c, r) = size.split_once('x').context("--size needs <cols>x<rows>")?;
                args.size = (c.parse()?, r.parse()?);
            }
            "-h" | "--help" => {
                println!("{HELP}");
                std::process::exit(0);
            }
            other => bail!("unknown argument {other}\n\n{HELP}"),
        }
    }
    Ok(args)
}

/// Finds a bare command on the café's PATH. Rex starts spawned panes in a
/// non-interactive shell, which may not have the PATH an interactive one sets.
fn resolve_program(mut cmd: Vec<String>) -> Vec<String> {
    if let Some(program) = cmd.first_mut()
        && !program.contains('/')
        && let Some(path) = std::env::var_os("PATH")
    {
        let found = std::env::split_paths(&path)
            .map(|dir| dir.join(&*program))
            .find(|p| {
                use std::os::unix::fs::PermissionsExt;
                p.metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            });
        if let Some(found) = found {
            *program = found.display().to_string();
        }
    }
    cmd
}

/// Writes each sprite on a dark tile, 12x, with Hat colours filled in.
fn dump_sprites(dir: &PathBuf) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let art = view::art::art();
    let hat = rex_cafe::cafe::HATS[0];
    let overrides = [('H', hat.color), ('h', hat.shade)];
    for (sheet_name, sheet) in [("characters", &art.chars), ("props", &art.props)] {
        for name in sheet.names() {
            let s = sheet.get(name);
            let mut c = Canvas::new(s.w + 2, s.h + 2, Rgb::hex(0x1a1410));
            c.sprite(s, 1, 1, &sheet.palette, &overrides);
            std::fs::write(dir.join(format!("{sheet_name}-{name}.ppm")), c.to_ppm(12))?;
        }
    }
    Ok(())
}

fn dump(dir: &PathBuf, at: &[f64], (cols, rows): (u16, u16)) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut result = Ok(());
    replay(at, |t, model| {
        let canvas = view::paint(&model.cafe, None);
        let frame = view::compose(
            &model.cafe,
            &Ui {
                demo: true,
                ..Ui::default()
            },
            cols,
            rows,
            WALL + t,
        );
        let name = format!("frame-{t:05.1}");
        let write = std::fs::write(dir.join(format!("{name}.ppm")), canvas.to_ppm(6))
            .and_then(|_| std::fs::write(dir.join(format!("{name}.txt")), frame.screen.to_text()))
            .and_then(|_| {
                let screen = frame.screen.to_ppm(4, 8);
                std::fs::write(dir.join(format!("{name}-screen.ppm")), screen)
            });
        if let Err(e) = write {
            result = Err(e);
        }
    });
    Ok(result?)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = parse_args()?;
    let settings_path = rex_cafe::settings::path();
    let settings = settings_path
        .as_deref()
        .map(Settings::load)
        .unwrap_or_default();
    if let Some(path) = &args.dump_audio {
        return rex_cafe::audio::render_wav(path, 60.0, settings.levels);
    }
    if let Some(dir) = &args.sprites {
        return dump_sprites(dir);
    }
    if let Some(dir) = &args.dump {
        return dump(dir, &args.at, args.size);
    }

    let (feed_tx, feed_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    if args.demo {
        tokio::spawn(demo::run(feed_tx, cmd_rx));
    } else {
        let ctx = rex::Context {
            socket: rex::client::socket_path(),
            session: std::env::var("REX_SESSION").ok().filter(|s| !s.is_empty()),
            block: std::env::var("REX_BLOCK").ok().filter(|s| !s.is_empty()),
            cwd: std::env::current_dir()
                .map(|d| d.display().to_string())
                .unwrap_or_default(),
            spawn_cmd: resolve_program(args.spawn_cmd),
        };
        tokio::spawn(rex::run(ctx, feed_tx, cmd_rx));
    }
    let muted = args.quiet || settings.muted;
    let audio = rex_cafe::audio::Audio::start(settings.levels, muted);
    App::new(cmd_tx, args.demo, audio, settings, settings_path, muted)
        .run(feed_rx)
        .await
}

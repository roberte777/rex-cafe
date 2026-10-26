use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use tokio::sync::mpsc;

use rex_cafe::app::App;
use rex_cafe::replay::{WALL, replay};
use rex_cafe::view::{self, Ui};
use rex_cafe::{demo, rex};

const HELP: &str = "\
rex-cafe: a pixel-art café where every agent on your Rex server is a Clawd

Usage: rex-cafe [--demo] [--spawn-cmd <command>]

  --demo                 Play made-up agents instead of reading Rex
  --spawn-cmd <command>  What `n` and the door open beside the café (default: claude)
  --dump <dir>           Write demo frames at --at seconds as .ppm and .txt, then exit
  --at <secs,...>        Times for --dump (default: 0,8,20,40)
  -h, --help             Show this help

Keys: tab/arrows select · enter jump to the agent · n new agent · r resync · q quit
Mouse: click a Clawd or its Legend row to jump to it; click the door for a new agent";

struct Args {
    demo: bool,
    spawn_cmd: Vec<String>,
    dump: Option<PathBuf>,
    at: Vec<f64>,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        demo: false,
        spawn_cmd: vec!["claude".into()],
        dump: None,
        at: vec![0.0, 8.0, 20.0, 40.0],
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--demo" => args.demo = true,
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

fn dump(dir: &PathBuf, at: &[f64]) -> Result<()> {
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
            160,
            56,
            WALL + t,
        );
        let name = format!("frame-{t:05.1}");
        let write = std::fs::write(dir.join(format!("{name}.ppm")), canvas.to_ppm(6))
            .and_then(|_| std::fs::write(dir.join(format!("{name}.txt")), frame.screen.to_text()));
        if let Err(e) = write {
            result = Err(e);
        }
    });
    Ok(result?)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = parse_args()?;
    if let Some(dir) = &args.dump {
        return dump(dir, &args.at);
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
    App::new(cmd_tx, args.demo).run(feed_rx).await
}

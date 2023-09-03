use std::{
    io::{stdout, Write},
    time::Duration,
};

use clap::Parser;
use miette::{IntoDiagnostic, Result};
use zookeeper::ZooKeeper;

pub fn get_styles() -> clap::builder::Styles {
    clap::builder::Styles::styled()
        .usage(
            anstyle::Style::new()
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow)))
                .bold(),
        )
        .header(
            anstyle::Style::new()
                .bold()
                .underline()
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow))),
        )
        .literal(
            anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green))),
        )
}

#[derive(Debug, Parser)]
#[clap(about = "Cli around zookeeper")]
#[command(styles = get_styles())]
struct Options {
    #[clap(long, short, default_value_t = String::from("localhost:2181/"))]
    pub addr: String,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Parser)]
enum Command {
    Ls {
        path: Option<String>,
    },
    Tree {
        path: Option<String>,
    },
    Cat {
        file: String,
    },
    Rm {
        file: String,
        #[clap(long, short, default_value_t = false)]
        recursive: bool,
    },
}

fn main() -> Result<()> {
    let opt = Options::parse();
    let mut log_builder = env_logger::Builder::new();
    log_builder.parse_filters("warn");
    log_builder.init();

    log::info!("Connecting to {}", opt.addr);
    let zk = ZooKeeper::connect(&opt.addr, Duration::from_secs(1), |_| ()).into_diagnostic()?;
    log::info!("Connected");

    match opt.command {
        Command::Ls { path } => {
            let mut path = path.unwrap_or(String::from("/"));
            sanitize_path(&mut path);
            let mut children = zk.get_children(&path, false).into_diagnostic()?;
            children.sort();
            for child in children {
                if path == "/" {
                    path = format!("");
                }
                let stat = zk
                    .exists(&format!("{}/{}", path, child), false)
                    .into_diagnostic()?
                    .unwrap();
                if stat.num_children == 0 {
                    print!("{child} ");
                } else {
                    print!("{child}/ ");
                }
            }
            println!();
        }
        Command::Tree { path } => {
            let mut path = path.unwrap_or(String::from("/"));
            sanitize_path(&mut path);
            tree(&zk, &path, 0)?;
        }
        Command::Cat { mut file } => {
            sanitize_path(&mut file);
            let (data, _) = zk.get_data(&file, false).into_diagnostic()?;
            stdout().write_all(&data).into_diagnostic()?;
        }
        Command::Rm {
            mut file,
            recursive,
        } => {
            sanitize_path(&mut file);
            if recursive {
                recursive_delete(&zk, &file)?;
            } else {
                zk.delete(&file, None).into_diagnostic()?;
            }
        }
    }
    Ok(())
}

fn tree(zk: &ZooKeeper, mut path: &str, depth: usize) -> Result<()> {
    println!("{}{}", "  ".repeat(depth), path);
    let mut children = zk.get_children(&path, false).into_diagnostic()?;
    if path == "/" {
        path = ""
    }
    children.sort();
    for child in children {
        let stat = zk
            .exists(&format!("{}/{}", path, child), false)
            .into_diagnostic()?
            .unwrap();
        if stat.num_children == 0 {
            println!("{}{child}", "  ".repeat(depth + 1));
        } else {
            tree(zk, &format!("{path}/{child}"), depth + 1)?;
        }
    }
    Ok(())
}

fn recursive_delete(zk: &ZooKeeper, path: &str) -> Result<()> {
    let stat = zk.exists(&path, false).into_diagnostic()?.unwrap();

    if stat.num_children == 0 {
        zk.delete(&path, None).into_diagnostic()?;
        return Ok(());
    }

    let children = zk.get_children(&path, false).into_diagnostic()?;
    for child in children {
        let path = if path == "/" { "" } else { path };
        recursive_delete(zk, &format!("{}/{}", path, child))?;
    }

    zk.delete(&path, None).into_diagnostic()?;

    Ok(())
}

fn sanitize_path(path: &mut String) {
    if !path.starts_with("/") {
        log::warn!(
            "Invalid path, adding a `/` to the beginning of your path: `{path}` => `/{path}`"
        );
        *path = format!("/{path}");
    }
    if path.ends_with("/") && *path != "/" {
        *path = path.trim_end_matches("/").to_string();
        log::warn!("Invalid path, removing the `/` at the end of your path: `{path}/` => `{path}`");
    }
}

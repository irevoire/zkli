use std::{
    fmt::Display,
    io::{stdin, stdout, Read, Write},
    time::Duration,
};

use clap::{Parser, ValueEnum};
use colored::{ColoredString, Colorize};
use miette::{miette, Context, IntoDiagnostic, Result};
use zookeeper::{Acl, ZooKeeper};

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
    /// List directory contents.
    #[clap(aliases = &["list", "l", "ll"])]
    Ls {
        /// List directory contents from the given path.
        path: Option<String>,
    },
    /// List contents of directories in a tree-like format.
    #[clap(aliases = &["t"])]
    Tree {
        /// Print the tree from the given path.
        path: Option<String>,
    },
    /// Print file.
    #[clap(aliases = &["bat"])]
    Cat {
        /// Path of the file to cat.
        file: String,
        /// Use it to send binary data to stdout.
        #[clap(long, short, default_value_t = false)]
        binary: bool,
    },
    /// Remove directory entries.
    #[clap(aliases = &["rmdir"])]
    Rm {
        /// Path of the files to remove.
        paths: Vec<String>,
        /// Call itself recursively until every file and directory has been deleted.
        #[clap(long, short, default_value_t = false)]
        recursive: bool,
    },
    /// Write the content of stdin or argv to the specified path.
    /// The path must already exists. See the create command if you need to create a new node.
    #[clap(aliases = &["set"])]
    Write {
        /// Path of the file to write.
        path: String,
        /// Content to write in the file.
        content: Option<String>,
        /// Force:
        /// - If the file doesn't exsists create it as persistent.
        /// - If you don't send any content, erase the content of the file for nothing.
        #[clap(long, short, default_value_t = false)]
        force: bool,
    },
    /// Create a new file.
    /// Write the content of stdin or argv to the specified path.
    /// By default the file is created in persistent. If you override this value by ephemeral, the node will be deleted before the cli exit.
    /// By default the acls are set as: anyone can do anything.
    Create {
        /// Path of the file to write.
        path: String,
        /// Content to write in the file.
        content: Option<String>,
        /// Mode to use when creating the file.
        #[clap(long, default_values_t = vec![CreateMode::Persistent])]
        mode: Vec<CreateMode>,
    },
}

#[derive(Debug, Parser, Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum CreateMode {
    Persistent,
    Ephemeral,
    Sequential,
}

impl Display for CreateMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreateMode::Persistent => write!(f, "persistent"),
            CreateMode::Ephemeral => write!(f, "ephemeral"),
            CreateMode::Sequential => write!(f, "sequential"),
        }
    }
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
                let child = format_node_from_stat(&child, &stat);
                print!("{child} ");
            }
            println!();
        }
        Command::Tree { path } => {
            let mut path = path.unwrap_or(String::from("/"));
            sanitize_path(&mut path);
            let stat = zk.exists(&path, false).into_diagnostic()?.unwrap();
            println!("{}", format_node_from_stat(&path, &stat));
            tree(&zk, &path, 0)?;
        }
        Command::Cat { mut file, binary } => {
            sanitize_path(&mut file);
            let (data, _) = zk.get_data(&file, false).into_diagnostic()?;
            if binary {
                stdout().write_all(&data).into_diagnostic()?;
            } else {
                match String::from_utf8(data) {
                    Ok(s) => println!("{s}"),
                    err => {
                        err.into_diagnostic()
                            .wrap_err("To output the binary data use `-b` or `--binary`.")?;
                    }
                }
            }
        }
        Command::Rm { paths, recursive } => {
            for mut path in paths {
                let ret = || -> Result<()> {
                    sanitize_path(&mut path);
                    if recursive {
                        recursive_delete(&zk, &path)?;
                    } else {
                        zk.delete(&path, None).into_diagnostic()?;
                    }
                    Ok(())
                }();
                if let Err(e) = ret {
                    log::error!("`{}`: {}", path, e);
                }
            }
        }
        Command::Write {
            mut path,
            content,
            force,
        } => {
            sanitize_path(&mut path);
            let mut buffer = content
                .as_ref()
                .map_or(Vec::new(), |content| content.as_bytes().to_vec());
            if content.is_none() && atty::isnt(atty::Stream::Stdin) {
                stdin().read_to_end(&mut buffer).into_diagnostic()?;
            } else if content.is_none() && !force {
                return Err(miette!("Did you forgot to pipe something in the command? If you wanted to reset the content of the file use `--force` or `-f`."));
            }
            match zk.set_data(&path, buffer.clone(), None) {
                Ok(_) => (),
                Err(zookeeper::ZkError::NoNode) if force => {
                    zk.create(
                        &path,
                        buffer,
                        Acl::open_unsafe().clone(),
                        zookeeper::CreateMode::Persistent,
                    )
                    .into_diagnostic()?;
                }
                err => {
                    err.into_diagnostic()?;
                }
            }
        }
        Command::Create {
            mut path,
            content,
            mode,
        } => {
            sanitize_path(&mut path);
            let mut buffer = content
                .as_ref()
                .map_or(Vec::new(), |content| content.as_bytes().to_vec());
            if content.is_none() && atty::isnt(atty::Stream::Stdin) {
                stdin().read_to_end(&mut buffer).into_diagnostic()?;
            }
            let mode = match (
                mode.contains(&CreateMode::Persistent),
                mode.contains(&CreateMode::Ephemeral),
                mode.contains(&CreateMode::Sequential),
            ) {
                (true, true, _) => {
                    return Err(miette!(
                        "Can't use persistent and ephemeral at the same time."
                    ))
                }
                (true | false, false, true) => zookeeper::CreateMode::PersistentSequential,
                (true | false, false, false) => zookeeper::CreateMode::Persistent,
                (false, true, true) => zookeeper::CreateMode::EphemeralSequential,
                (false, true, false) => zookeeper::CreateMode::Ephemeral,
            };
            let ret = zk
                .create(&path, buffer, Acl::open_unsafe().clone(), mode)
                .into_diagnostic()?;

            println!("{ret}");
        }
    }
    Ok(())
}

fn tree(zk: &ZooKeeper, mut path: &str, depth: usize) -> Result<()> {
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
        println!(
            "{}{}",
            "  ".repeat(depth + 1),
            format_node_from_stat(&child, &stat)
        );
        if stat.num_children > 0 {
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

fn format_node_from_stat(name: &str, stat: &zookeeper::Stat) -> ColoredString {
    let name = if stat.num_children == 0 || name == "/" {
        name.to_string()
    } else {
        format!("{name}/ ")
    };
    let mut name = name.bold();
    name = name.blue();
    if stat.data_length > 0 {
        name = name.green();
    }
    if stat.is_ephemeral() {
        name = name.italic();
    }

    name
}

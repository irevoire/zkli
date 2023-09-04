Zkli
====

```
Cli around zookeeper

Usage: zkli [OPTIONS] <COMMAND>

Commands:
  ls      List directory contents
  tree    List contents of directories in a tree-like format
  cat     Print file
  rm      Remove directory entries
  write   Write the content of stdin or argv to the specified path. The path must already exists. See the create command if you need to create a new node
  create  Create a new file. Write the content of stdin or argv to the specified path. By default the file is created in persistent. If you override this value by ephemeral, the node will be deleted before the cli exit. By default the acls are set as: anyone can do anything
  help    Print this message or the help of the given subcommand(s)

Options:
  -a, --addr <ADDR>  The addr of the zookeeper server [default: localhost:2181/]
  -v...              The verbosity, the more `v` you use and the more verbose it gets
  -h, --help         Print help
```

## Installation

### If you're a rust user

```
cargo install zkli
```

### If not

Join the sect here: https://doc.rust-lang.org/book/

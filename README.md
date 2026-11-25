## Why?

- **Flexibility**: this tool does not rely on another multiplexer to work, as it is built in. Therefore, you can run your minecraft server seamlessly on a dedicated multiplexer.
- **Utility**: currently, this tool is able to start your minecraft servers for you with configurable java arguments and allows you to connect to them at any time by simply running mcserver and then connecting with the terminal user interface.

## Installation
If you have the [rust toolchain](https://rust-lang.org/tools/install/) installed
```
cargo install mcserver
```

### Nix

Alternatively, you can install it with nix. For example, if you use NixOS:

In your flake inputs, add:
```
{
    mcserver = {
      url = "github:Neovimproved/mcserver";
      inputs.nixpkgs.follows = "nixpkgs";
    };
}
```

In a system module:
```nix
environment.systemPackages = [
    mcserver.packages.${pkgs.stdenv.hostPlatform.system}.default
]
```

## Usage

Run `mcserver` in your terminal and you may start servers by pressing space. Ensure that you have some Minecraft servers in your servers directory, which is ~/Servers/ by default. Also, you should create a .mcserver directory inside of your server and put a file called jar_file.txt in it, which contains the file name of the server jar if you are creating the server manually.

## Configuration

Create a file `config.kdl` in a configuration directory (e.g. `$XDG_CONFIG_HOME/mcserver/` or just `~/.config/mcserver/` which is probably the same). [KDL](https://kdl.dev/) is a relatively new configuration language that solves many of the pitfalls that come with things like json and yaml.

## Roadmap

- [x] Server creation
- [x] Server deletion

## Architecture

How does this work?
- This tool uses a similar architecture to common multiplexers such as Zellij and tmux by spinning up a server instance when one doesn't exist but a client is attempting to connect to one.
- The single server process spawns all of the minecraft servers in PTYs to emulate the servers being run in a terminal normally.
- When clients connect, they are able to send input to the stdin buffer and receive output from the stdout buffer of the process.
- Currently, TCP (uses port 56602) and unix sockets are supported for connecting clients to the server instance. However, you must be mindful that there is no verification system yet, so do not open a port exposing the TCP connection to the internet. Unix sockets should be preferred when the server is hosted on the same machine, as they are faster.

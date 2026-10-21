## Why?

- **Flexibility**: this tool does not rely on another multiplexer to work, as it is built in. Therefore, you can run your minecraft server seamlessly on a dedicated multiplexer.
- **Utility**: currently, this tool is able to start your minecraft servers for you with configurable java arguments and allows you to connect to them at any time by simply running mcserver.

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

## Configuration

Create a file `config.kdl` in a configuration directory (e.g. `$XDG_CONFIG_HOME/mcserver/` or just `~/.config/mcserver/` which is probably the same). [KDL](https://kdl.dev/) is a relatively new configuration language that solves many of the pitfalls that come with things like json and yaml.

## Roadmap

- Server creation
- Server deletion

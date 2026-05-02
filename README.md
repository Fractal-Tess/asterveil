![asterveil banner](https://capsule-render.vercel.app/api?type=waving&color=0:0f172a,100:06b6d4&height=220&section=header&text=asterveil&fontSize=46&fontColor=ffffff&desc=NVIDIA%20GPU%20Power%20%26%20Fan%20Control%20TUI&descSize=18&descAlignY=62)

# asterveil

A terminal UI for monitoring and controlling NVIDIA GPUs — power limits, fan speeds, clocks, temperatures, and VRAM usage at a glance.

[![Release](https://github.com/Fractal-Tess/asterveil/actions/workflows/release.yml/badge.svg)](https://github.com/Fractal-Tess/asterveil/actions/workflows/release.yml)

![Rust](https://img.shields.io/badge/lang-Rust-DEA584)
![Ratatui](https://img.shields.io/badge/tui-Ratatui-blue)
![NVIDIA](https://img.shields.io/badge/gpu-NVIDIA-76B900)
![Nix](https://img.shields.io/badge/devshell-Nix-5277C3)
![License](https://img.shields.io/badge/license-MIT-green)

## Features

- **Live monitoring** — temperature, utilization, clocks, fan speed, VRAM, and power draw updated every 500ms.
- **Power limit control** — apply preset profiles (low / balanced / max / custom) per GPU.
- **Fan speed control** — set fixed fan speeds or return to automatic control.
- **Multi-GPU support** — navigate and manage multiple GPUs, with bulk selection.
- **Keyboard-driven** — fully navigable with keyboard shortcuts.

## Install

### Pre-built binaries

Grab the latest release from the [releases page](https://github.com/Fractal-Tess/asterveil/releases) — available for Linux x86_64 and aarch64.

### Nix flake

Add as an input and apply the overlay:

```nix
# flake.nix
inputs.asterveil.url = "github:Fractal-Tess/asterveil";

# In your NixOS configuration:
nixpkgs.overlays = [ inputs.asterveil.overlays.default ];
environment.systemPackages = [ pkgs.asterveil ];
```

### Build from source

```bash
cargo install --path .
```

## Usage

```bash
asterveil          # launch the TUI
asterveil --version  # print version
```

### Keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Move cursor down |
| `k` / `↑` | Move cursor up |
| `p` | Set power limit |
| `f` | Set fan speed |
| `Space` | Toggle GPU selection |
| `r` | Refresh GPU data |
| `q` / `Esc` | Quit |

## Requirements

- Linux with NVIDIA GPU(s)
- `nvidia-smi` and `nvidia-settings` on `$PATH`
- Root/sudo access for applying power and fan settings

## Stack

- **[Rust](https://www.rust-lang.org/)** — systems language
- **[Ratatui](https://ratatui.rs/)** — terminal UI framework
- **[Crossterm](https://github.com/crossterm-rs/crossterm)** — terminal manipulation

## License

MIT

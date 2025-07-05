# Metabolistic3D

A cellular metabolism simulation game built with the Bevy engine.

## Overview

Metabolistic3D simulates cellular metabolism through interconnected metabolic blocks that mirror real biological processes. Players must balance energy production, resource management, and metabolic efficiency while navigating through different cellular environments.

### Key Features

- **3D Environment**: Navigate through cellular landscapes with realistic physics
- **Metabolic Simulation**: Manage complex metabolic pathways including glycolysis, respiration, and photosynthesis
- **Resource Management**: Balance ATP, NADH/NADPH, and other cellular currencies
- **Dynamic Gameplay**: Adapt to different environmental conditions and resource availability
- **Educational**: Learn real biochemistry through engaging gameplay

## System Requirements

### Prerequisites
- **[Rust](https://rustup.rs/)** (latest stable version recommended)
- **Git** for cloning the repository
- **Graphics drivers** up to date (especially for integrated graphics)

### Additional Dependencies (Linux)
For Linux users, you may need additional packages:
```bash
# Ubuntu/Debian
sudo apt-get install libasound2-dev libudev-dev pkg-config

# Fedora/RHEL
sudo dnf install alsa-lib-devel systemd-devel pkgconf-pkg-config
```

## Installation & Getting Started

### Quick Start (Recommended)
1. **Install Rust** (if you haven't already):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source ~/.cargo/env
   ```

2. **Clone and run the game:**
   ```bash
   git clone <repository-url>
   cd metabolistic3d
   cargo run --profile release-native
   ```

   The game will automatically download dependencies and launch when ready.

### Build Options

Choose the build that best fits your needs:

- **Quick Play** (Recommended for most users):
  ```bash
  cargo run --profile release-native
  ```
  Optimized performance for desktop gaming.

- **Development Mode** (For contributors):
  ```bash
  cargo run
  ```
  Includes debug tools, hot reloading, and inspector (F12).

- **Web Build** (Advanced users):
  ```bash
  cargo build --release
  ```
  Optimized for web deployment with smaller file size.

### Development Features

The development build includes:
- Hot asset reloading
- Debug tools and inspector
- Physics debugging visualization
- Developer console

## How to Play

### Basic Controls
- **Movement**: `WASD` or arrow keys to move your character
- **Camera**: Mouse to look around and pan the camera
- **Interact**: Left-click to interact with metabolic blocks
- **Menu Navigation**: `Esc` to return to main menu

### Game Modes
- **3D Exploration**: Navigate the cellular environment in first person
- **2D Flowmap**: Press `2` to view metabolic pathways as a flow diagram  
- **Genome Editor**: Press `3` to modify and visualize the cell's genome
- **Main Menu**: Press `1` or `Esc` to return to the start screen

### Gameplay Basics
1. **Start in 3D mode** to explore the cellular environment
2. **Manage resources** by balancing ATP, NADH, and other metabolic currencies
3. **Activate metabolic blocks** to produce energy and maintain cellular function
4. **Use the genome editor** to control which metabolic pathways are active
5. **Monitor the 2D flowmap** to understand resource flows and bottlenecks

### Development Mode (Contributors Only)
- **Inspector**: `F12` to open debug tools and entity inspection
- **Physics Debug**: Collision boundaries and physics visualization
- **Hot Reload**: Assets automatically refresh during development

## Troubleshooting

### Common Issues

**Compilation errors:**
- Ensure you have the latest stable Rust: `rustup update`
- Clear the build cache: `cargo clean` then rebuild
- On Linux, install the required packages listed in the dependencies section

**Performance issues:**
- Use the optimized build: `cargo run --profile release-native`
- Close other graphics-intensive applications
- Lower your display resolution or graphics settings

**Audio issues (Linux):**
- Install ALSA development libraries: `sudo apt-get install libasound2-dev`
- Or use the provided setup script: `./codex-setup.sh`

### Getting Help

- **Issues**: Report bugs and problems on the project's issue tracker
- **Discussions**: For gameplay questions and feedback
- **Documentation**: Check `Summary.md` for detailed game mechanics

### For Developers

- **[CLAUDE.md](CLAUDE.md)**: Developer guidance for code contributions
- **[BEVYCONTEXT.md](BEVYCONTEXT.md)**: Bevy engine patterns and best practices  
- **[Summary.md](Summary.md)**: Game design document and metabolic system details

### Built With

- **[Bevy Engine](https://bevyengine.org/)**: Modern game engine with ECS architecture
- **[Avian3D](https://github.com/Jondolf/avian)**: Physics simulation
- **Rust**: High-performance systems programming language

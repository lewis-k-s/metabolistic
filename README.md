# Metabolistic3D

A 3D cellular metabolism simulation game built with the Bevy engine. Explore the fascinating world of cellular biochemistry by managing metabolic pathways, optimizing energy production, and surviving in various biological environments.

## Overview

Metabolistic3D simulates cellular metabolism through interconnected metabolic blocks that mirror real biological processes. Players must balance energy production, resource management, and metabolic efficiency while navigating through different cellular environments.

### Key Features

- **3D Environment**: Navigate through cellular landscapes with realistic physics
- **Metabolic Simulation**: Manage complex metabolic pathways including glycolysis, respiration, and photosynthesis
- **Resource Management**: Balance ATP, NADH/NADPH, and other cellular currencies
- **Dynamic Gameplay**: Adapt to different environmental conditions and resource availability
- **Educational**: Learn real biochemistry through engaging gameplay

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (latest stable version)
- Git

### Installation & Running

1. **Clone the repository:**
   ```bash
   git clone <repository-url>
   cd metabolistic3d
   ```

2. **Build the project**
    ```bash
    cargo build
    ```

    **Build Profiles**

    - **Development** (`cargo build`): Fast compilation with hot reloading
    - **Release** (`cargo build --release`): Optimized for web deployment
    - **Release Native** (`cargo build --profile release-native`): Optimized for desktop performance

2. **Run the game (development mode):**
   ```bash
   cargo run
   ```

3. **Run optimized build:**
   ```bash
   cargo run --profile release-native
   ```

### Development Features

The development build includes:
- Hot asset reloading
- Debug tools and inspector
- Physics debugging visualization
- Developer console

## Controls

- **Movement**: WASD or arrow keys
- **Camera**: Mouse look
- **Debug**: F12 for inspector (development mode)
- **Genome Editor**: Press `3` in-game to open the circular genome view

### Technical Documentation

- **[BEVYCONTEXT.md](BEVYCONTEXT.md)**: Comprehensive guide to Bevy engine concepts, patterns, and best practices used in this project
- **[Summary.md](Summary.md)**: Detailed game design document covering metabolic blocks, gameplay mechanics, and biological accuracy

### Key Technologies

- **[Bevy Engine](https://bevyengine.org/)**: Modern game engine with ECS architecture
- **[Avian3D](https://github.com/Jondolf/avian)**: Physics simulation
- **[bevy-inspector-egui](https://github.com/jakobhellermann/bevy-inspector-egui)**: Runtime debugging and inspection
- **[Leafwing Input Manager](https://github.com/Leafwing-Studios/leafwing-input-manager)**: Input handling

## Project Structure

```
src/
├── main.rs           # Application entry point and setup
├── lib.rs            # Library root
├── camera.rs         # Camera systems and controls
├── debug.rs          # Debug utilities and visualization
├── dev_tools.rs      # Development tools and helpers
├── inspector.rs      # Runtime inspection systems
├── molecules.rs      # Molecular entities and chemistry systems
├── shared.rs         # Shared utilities and components
├── blocks/           # Primary metabolistic processing blocks
    ├── genome.rs     # Manages the cell genome (state of expression of other blocks)
├── player/           # Player controller and movement systems
├── scenes/           # Scene management (menu, 2D, 3D scenes)
└── terrain/          # Terrain generation and management
```

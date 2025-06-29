# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Metabolistic3D is a 3D cellular metabolism simulation game built with the Bevy engine. It simulates cellular biochemistry through interconnected metabolic blocks that mirror real biological processes, teaching players about metabolism through engaging gameplay.

## Architecture

### Core Structure
- **Library-based architecture**: Main logic in `src/lib.rs` with binary entry point in `src/main.rs`
- **Plugin-based system**: Uses Bevy's plugin architecture for modular game systems
- **State management**: Implements game states (MainMenu, Scene3D, Scene2D, GenomeEditing) with proper transitions
- **ECS pattern**: Leverages Bevy's Entity-Component-System for game logic

### Key Modules
- `blocks/`: Metabolic pathway implementations (fermentation, genome editing)
- `molecules.rs`: Currency system for ATP, NADH/NADPH, and metabolic precursors
- `metabolism/`: Core metabolic flow simulation
- `scenes/`: Different game views (3D world, 2D flowmap, genome editor, menu)
- `player/`: Player controller and movement systems
- `camera.rs`: Camera management across different scenes
- `shared.rs`: Common resources and state transition systems

### Plugin Architecture
The app uses a plugin-based architecture where each major system is implemented as a Bevy plugin:
- `CurrencyPlugin`: Manages ATP, NADH, and other metabolic currencies
- `GenomePlugin`: Handles genome editing and visualization
- `FermentationPlugin`: Implements fermentation metabolic pathways
- `MetabolicFlowPlugin`: Core metabolic simulation engine
- Scene-specific plugins for different game states

## Development Commands

### Build Commands
- **Development build**: `cargo run` (includes hot reloading, debug tools)
- **Optimized build**: `cargo run --profile release-native`
- **Web-optimized build**: `cargo build --release`
- **Headless build**: `cargo build --features headless` (no graphics/audio)

### Testing
- **Run all tests**: `cargo test`
- **Run specific test**: `cargo test <test_name>`
- **Headless tests**: `cargo test --features headless`

### Feature Flags
- `full` (default): Complete build with graphics, audio, and UI
- `headless`: Minimal build for testing/CI (no graphics, audio, UI)
- `dev`: Development features with dynamic linking and dev tools
- `dev_native`: Development features plus file watching for hot reload

### Development Tools (In-Game)
- **Inspector**: F12 to open runtime debugging and inspection tools
- **Physics Debug**: Enabled by default in development builds
- **State Transitions**: Press number keys to switch between game states
- **Genome Editor**: Press `3` to open circular genome visualization

## Code Patterns

### Component Architecture
- Use `#[derive(Component)]` for game entities
- Implement required components with `#[require(...)]` attribute
- Prefer component composition over inheritance

### System Organization
- Systems are organized by functionality in plugin modules
- Use `Startup` schedule for initialization
- Use `Update` schedule for per-frame logic
- State-specific systems use run conditions like `.run_if(in_state(GameState::Scene3D))`

### Resource Management
- Currency systems use shared resources accessible across plugins
- State transitions managed through `NextState<GameState>` resource
- Asset loading handled through Bevy's `AssetServer`

### Error Handling
- Tests use `Result` types with proper error propagation
- Headless mode available for testing without graphics/audio dependencies
- Setup script (`codex-setup.sh`) handles environment dependencies

## Metabolic System Design

The game implements a sparse metabolic network with:
- **Currency Hub**: ATP, NADH/NADPH, Acetyl-CoA, Carbon Skeletons
- **Metabolic Blocks**: Light Capture, Sugar Catabolism, Respiration, Fermentation, etc.
- **Minimal Cross-talk**: Only essential metabolites flow between blocks
- **Real Biochemistry**: Based on actual cellular metabolism pathways

## Testing Strategy

The codebase includes comprehensive testing:
- Unit tests for individual components and systems
- Integration tests for plugin interactions
- Headless app creation for testing without graphics dependencies
- Currency system validation tests
- State management verification

## Environment Setup

For development in containers or headless environments, use the provided setup script:
```bash
./codex-setup.sh
```

This installs necessary audio system dependencies and configures the environment for both full and headless builds.
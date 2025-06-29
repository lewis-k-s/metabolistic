# MetabolicFlow & FlowEditor – Bevy 0.15 Implementation Guide

**Context** The game now separates metabolism solving (**MetabolicFlow**) from its editing overlay (**FlowEditor**). MetabolicFlow owns the resource‑routing graph and runs on its own low‑frequency schedule, automatically handling *all* flux. FlowEditor is an on‑demand UI layer ticking every frame for smooth interaction.

---

## 1  System Split

| Layer                    | Schedule label      | Tick rate               | Purpose                                                         |
| ------------------------ | ------------------- | ----------------------- | --------------------------------------------------------------- |
| **MetabolicFlow** (core) | `MetabolicSchedule` | **Fixed 0.25 s** (4 Hz) | Solve flux, propagate back‑pressure, update gameplay resources. |
| **FlowEditor** (UI)      | `EditorSchedule`    | **Every frame**         | Render canvas, handle drag‑drop, stage edits.                   |

MetabolicSchedule never stops; players see live metabolism even while editing.

---

## 2  Plugin Layout

```rust
// core plugin
pub struct MetabolicFlowPlugin;
impl Plugin for MetabolicFlowPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<MetabolicGraph>()   // dense cache (nodes, edges)
            .init_resource::<FlowDirty>()        // graph‑dirty flag
            .add_schedule(MetabolicSchedule, build_metabolic_schedule());
    }
}

// UI plugin
pub struct FlowEditorPlugin;
impl Plugin for FlowEditorPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_schedule(EditorSchedule, build_editor_schedule())
            .add_systems(OnEnter(GameState::FlowEditor), setup_editor_ui)
            .add_systems(OnExit(GameState::FlowEditor), teardown_editor_ui);
    }
}
```

---

## 3  Schedules

```rust
fn build_metabolic_schedule() -> Schedule {
    Schedule::new()
        .with_run_criteria(FixedTimestep::step(0.25)) // 4 Hz
        .add_systems((
            rebuild_graph.run_if(resource_changed::<FlowDirty>()),
            solve_flux_system,        // automatic routing happens here
            apply_flux_results_system,
        ))
}

fn build_editor_schedule() -> Schedule {
    Schedule::new()
        .add_systems((
            editor_input_system,             // drag‑drop etc.
            render_canvas_system.after(editor_input_system),
            commit_edits_system,             // sets FlowDirty when player clicks "Apply"
        ))
}
```

*`solve_flux_system` performs all routing; the global `Update` stage stays clean.*

---

## 4  State Switching

```rust
#[derive(States, Clone, Eq, PartialEq, Debug, Hash)]
pub enum GameState { Gameplay, FlowEditor }

app.add_state::<GameState>()
    .add_plugins((MetabolicFlowPlugin, FlowEditorPlugin))
    .add_systems(Update, toggle_editor.run_if(key_just_pressed(KeyCode::F)));
```

---

## 5  ECS Cheat‑Sheet

| Name                   | Kind                 | Notes                                              |
| ---------------------- | -------------------- | -------------------------------------------------- |
| `MetabolicGraph`       | `Resource`           | Dense vectors of nodes & edges used by solver.     |
| `FlowDirty(bool)`      | `Resource`           | True when edits require a graph rebuild.           |
| `FlowDraft`            | `Resource` (UI only) | Copy‑on‑write snapshot edited by FlowEditor.       |
| `FluxResult`           | `Resource`           | Per‑node rates + flags picked up by gameplay & UI. |
| `FlowNode`, `FlowEdge` | `Component`          | ECS rep mostly for editor / debug views.           |

---

## 6  Performance At‑a‑Glance

| Aspect       | MetabolicFlow                      | FlowEditor               |
| ------------ | ---------------------------------- | ------------------------ |
| Cadence      | Fixed 4 Hz                         | 60 Hz                    |
| Rebuild cost | Only when `FlowDirty`              | N/A                      |
| Maths        | Vec mul‑add, Rayon when >1 k edges | None                     |
| Memory       | Bit‑packed flags in `FluxResult`   | Reads flags only         |
| Threading    | Optional `AsyncComputeTaskPool`    | Main thread              |
| Dormant skip | `run_if` guard on ATP/paused       | UI shows "dormant" badge |

---

## 7  Frame Timeline

1. **UI frame** (60 Hz) – Player drags edge in FlowEditor → `FlowDraft` changes.
2. **Commit** – `commit_edits_system` diffs draft vs. graph → writes updates → `FlowDirty = true`.
3. **Next Metabolic tick** – `rebuild_graph` rebuilds cache; `solve_flux_system` computes rates; writes `FluxResult`.
4. **UI frames** animate edge colours from `FluxResult`.

---

## 8  Genome Integration

| Concept                       | Implementation                                                                                                              | Player impact                                                                             |
| ----------------------------- | --------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| **Genome is source of truth** | GenomePlugin raises `GenomeDiffEvent` on gene state change.                                                                 | Gene mutation instantly propagates to metabolism.                                         |
| **Status cache**              | MetabolicFlow maintains `BlockStatus` (Active / Mutated / Silent) for each `BlockKind`.                                     | Solver multiplies throughput by 1.0 / *m* / 0.0.                                          |
| **Event bridge**              | `on_genome_diff` system updates node statuses and sets `FlowDirty = true`.                                                  | Graph auto‑rebuilds next tick; no manual wiring lost.                                     |
| **No deletion**               | Silent or Mutated blocks remain in graph but throttle or zero flux.                                                         | Designer wiring never breaks; UI shows grey/amber tint.                                   |
| **Editor cues**               | FlowEditor colours nodes: green (Active), striped amber (Mutated), grey (Silent); dashed edges if either endpoint inactive. | Players immediately spot why a pathway stalls & can open Genome screen to repair/express. |

```rust
fn on_genome_diff(
    mut diff_reader: EventReader<GenomeDiffEvent>,
    genome: Res<Genome>,
    mut nodes: Query<&mut MetabolicNode>,
    mut dirty: ResMut<FlowDirty>,
) {
    if diff_reader.iter().next().is_some() {
        for mut node in &mut nodes {
            node.status = match genome.get_gene_state(&node.kind) {
                Some(GeneState::Expressed) => BlockStatus::Active,
                Some(GeneState::Mutated)   => BlockStatus::Mutated,
                _                          => BlockStatus::Silent,
            };
        }
        dirty.0 = true;
    }
}
```

---

### Why this layout?

* **Isolation** – Core solver untouched by UI & genome cadence differences.
* **Efficiency** – 4 Hz tick + rebuild‑on‑change keeps CPU cost low.
* **Flexibility** – All designer nodes stay wired; genome mutations simply scale their output.
* **Feedback** – Players get live colour / throughput hints and can react immediately.

---

*Ready to evolve further? We can next tackle persistence (save/load) or multiplayer sync.*

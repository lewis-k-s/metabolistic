# Metabolistic3D TODO

## Future Features

### Multi-Player Support
- [ ] **Multiple Player Cells**: Support multiple player entities, each with their own cell and genome
- [ ] **Cell Isolation**: Ensure complete resource isolation between different player cells
- [ ] **Inter-Cell Interactions**: Design systems for potential cell-to-cell communication or resource trading
- [ ] **Player Management**: Add player identification and management systems
- [ ] **Performance**: Optimize for multiple concurrent metabolic simulations

### Mitochondrial DNA System
- [ ] **Dual Genome Architecture**: Support both nuclear and mitochondrial genomes within the same cell
- [ ] **Mitochondrial-Specific Blocks**: Implement mitochondrial-specific metabolic pathways
  - Respiratory chain complexes (I-V)
  - Mitochondrial fatty acid oxidation
  - Pyruvate dehydrogenase complex
- [ ] **Resource Sharing**: Design controlled resource sharing between nuclear and mitochondrial systems
- [ ] **Inheritance Patterns**: Implement maternal inheritance for mitochondrial genomes
- [ ] **Cross-Genome Regulation**: Allow nuclear genes to influence mitochondrial gene expression

### Enhanced Architecture
- [ ] **Cell Division**: Implement cell division mechanics with genome inheritance
- [ ] **Organelle Systems**: Expand beyond mitochondria to include chloroplasts, peroxisomes, etc.
- [ ] **Metabolic Networks**: Support more complex metabolic pathway interactions
- [ ] **Evolutionary Mechanics**: Add mutation, selection, and evolution systems
- [ ] **Environmental Factors**: Include external conditions that affect metabolism

## Current Architecture Notes

### Cell Ownership Pattern
The current implementation uses a cell ownership pattern where:
- Each `Cell` entity owns metabolic blocks via `OwnedBy` components
- `CurrencyPools` are per-cell components, not global resources
- `DefaultCell` resource points to the single-player cell entity
- All metabolic operations are scoped to their owning cell

### Multi-Genome Preparation
The architecture is designed to support multiple genomes per cell:
- Genome system can be extended to support different genome types
- Metabolic blocks can be owned by specific genomes within a cell
- Resource pools can be shared or isolated based on genome configuration
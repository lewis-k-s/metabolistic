  RequiredComponent in Bevy 0.15


  The #[require] attribute lets you specify that a component must have other components on the same
  entity. If they are missing, Bevy automatically adds them.


  For example, we can redefine our Cell component like this:



   1 // In src/cell.rs
   2
   3 #[derive(Component)]
   4 #[require(Genome::default())]
   5 #[require(CurrencyPools::with_defaults())]
   6 #[require(MetabolicGraph::default())]
   7 pub struct Cell;



  Now, whenever you add the Cell component to an entity, Bevy will automatically add Genome,
  CurrencyPools, and MetabolicGraph with their default values. This guarantees that a Cell entity is
  always valid and has its core components.


  A Hybrid Solution for Power and Flexibility


  While #[require] is great for enforcing invariants, we still need a clean way to override those
  defaults for testing and specific gameplay scenarios (your "dependency injection" idea).


  I propose a hybrid solution that gives us the best of both worlds:


   1. Use `#[require]` for Core Invariants: We'll add the #[require] attribute to the Cell component as
      shown above. This makes our component structure robust and self-documenting.
   2. Introduce a `CellBuilder` for Customization: I will create a CellBuilder that provides a clean,
      chainable API for spawning cells. This builder will allow you to easily override the default
      components that #[require] adds.


  Here is how the CellBuilder would look in action, especially for your tests:



    1 // Example of the new test setup
    2 let mut app = App::new();
    3 // ... setup plugins ...
    4
    5 // Spawn a cell with a specific gene expressed
    6 let cell = CellBuilder::new(&mut app.world.commands())
    7     .with_expressed_genes(vec![BlockKind::Fermentation])
    8     .spawn();
    9
   10 // Spawn a cell with custom currency values
   11 let cell2 = CellBuilder::new(&mut app.world.commands())
   12     .with_currencies(HashMap::from([(Currency::ATP, 100.0)]))
   13     .spawn();


  This approach makes creating test environments much more straightforward and readable.

  My Refined Plan


   1. Refactor `src/cell.rs`: I will update the Cell component to use #[require] and implement the
      CellBuilder for flexible spawning, removing the old, confusing spawn_* methods.
   2. Simplify `src/metabolism/mod.rs`: I will clean up the MetabolicFlowPlugin to remove the outdated
      systems, relying on the new component-driven architecture.
   3. Rewrite `tests/metabolic_flow_test.rs`: I will update the tests to use the CellBuilder, which will
      make them much cleaner and easier to understand.

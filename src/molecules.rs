use std::marker::PhantomData;


mod atoms {
    struct Carbon;
    struct Oxygen;
    struct Hydrogen;
    struct Phosphate;
    struct Magnesium;
}

struct Activator<T> { value: };
struct Inhibitor<T>;
enum Regulator<T> {
    Activator(Activator<T>),
    Inhibitor(Inhibitor<T>),
}

type CO2 = (Carbon, Oxygen, Oxygen);
type H2O = (Hydrogen, Hydrogen, Oxygen);
type Mg2 = (Magnesium, Magnesium);

struct ATP {
    value: Option<Phosphate>,
}

struct ADP {
    value: Option<Phosphate>,
}


struct NAD {
    value: Option<Hydrogen>,
    regulators: Vec<Regulator<Hydrogen>>,
}


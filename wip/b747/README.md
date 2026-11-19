# Boeing 747 (wip)

Not in the game: the game reads its aircraft from `assets/aircraft`, and this folder is not there.
To try it, move it back to `assets/aircraft/b747` and add its `aircraft.ron` to `BUILT_IN`
(`crates/sim/sim-core/src/aircraft/file.rs`). Its flight model is tested from here
(`crates/sim/sim-core/tests/b747.rs`, `examples/b747.rs`).

A 747-100 (`sim-core`'s README, "Jet transports": four turbofans, Mach, a gear that retracts,
spoilers, reverse thrust). Its model is imported (`source/import.py`): the
airframe one part, and the five legs of the gear, each what its spring raises (`Fork`) and a
`Wheel` for each axle (a bogie has two: each turns about its own); the model's main gear had the
tyres of one axle, copied onto the eight. A gear that retracts swings up as it goes
(`render::retracting`: a leg under the wing in towards the fuselage, one under it forward) and is
not drawn once it is up. Its flight deck (`flight-deck.py`, which writes `cockpit.glb`
and `cockpit.ron` together: the model, the instruments and what is printed on the panels): a round
upper deck with its windows cut out, the six flight instruments before each pilot, between them
the four engines' fan speeds, exhaust temperatures and fuel flows (`Instrument::FanSpeed`,
`ExhaustTemperature`, `FuelFlow`), the flaps' gauge and the gear's lamps (`FlapPosition`,
`GearLamps`) and lever, the pedestal with the thrust levers, the speed brake and flap levers
(`Control::Spoilers`, `Control::Gear` move theirs) and the radios, the overhead panel, and the
flight engineer's station behind the first officer; the mouse takes the thrust and flap levers. The outside camera follows it from further, by its span (`multirotor::chase_scale`); its
sound is its jets' (`audio::Synth::jets`: a roar that grows with the thrust and the fan's whine);
the panel of the outside view has N1 where the tachometer is. Its keys: ⇧G the gear, ⇧B the
speed brakes, ⇧S reverse thrust (then W for more). What it has not yet: control surfaces that
move on the model. The game starts it at the nearest aerodrome with a runway of its size (the
generated world's international airports, 3300 m; `Start::Home` and the restarts, by
`AircraftType::runway`), and L lands it there: the autopilot flies it as a transport
(`sim-core`'s README, "autopilot"). With the gear up the doors are closed over the wells
(`Moves::GearOut` the open doors and the wells' linings, `Moves::GearUp` the patches the import
closes the skin's holes with).

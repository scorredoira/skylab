# Phase 3: fine tuning

Detail that makes a good simulator better but does not change what it is. Phase 1 is a realistic
and spectacular game on an architecture that takes any vehicle and any physics; phase 2 adds
vehicles and systems (`PHASE2.md`). This list waits until both are done.

## Damage and crashes

- A bent propeller and a wing bent and hanging, drawn.
- Scratches and scorch on the paint.
- A broken fin drawn when it breaks from height (today only the rudder is lost).
- The engine damaged by degrees (study point D).
- The smoke of a fire shading the ground beneath it; bubbles from pieces sinking.
- A tree felled by an aircraft gone from the world for everyone (today each aircraft remembers its own).
- Crush strengths and trunk breaking angles from published data (today estimates noted by each constant).

## Instruments and systems

- The C172 static calibration rows for flaps 10° and 30° checked against the POH.
- The game's autopilot (L, the Extra's P, the traffic) and the trained pilots flying on the
  instruments' air data, so a blocked pitot fools them too (the C172's GFC 700 already does).
- The GFC 700's details: the GA button on the throttle, CWS, the altitude alerter's aural tone,
  ESP, servo authority and rates from measured data (estimates today), trim-in-motion and PTRM.
- An electrical system: battery, alternator, buses, what goes off when it fails.
- Two BARO settings, standby altimeter and PFD, as in the real aircraft.
- Icing from humidity and clouds, not only from the test key.

## World

- Runways with their real slope instead of each aerodrome flattened to one height.
- Apron floodlight masts as obstacles.
- Continuous hedgerows along field edges instead of a bush every 25 m.
- Stop bars and green taxiway centre lines.
- Bridges: done (17 Nov 2026): OpenStreetMap's bridges through Overture with
  decks, piers and their structure, and the Golden Gate and the Brooklyn Bridge by hand
  (`geodata::bridges::LANDMARKS`). Still to do: the maps' `man_made=bridge` outlines and
  `bridge:support` piers used where they are drawn (today the decks are the ways' widths and the
  piers are spaced by rule), lattice drawn on truss girders, the Golden Gate's art deco tower
  detail and its lights at night, more landmarks (the Tower Bridge, the Sydney Harbour Bridge).
- Buildings: a source of measured roof shapes where it is open (the Netherlands'
  3D BAG, CC BY 4.0; Spain's PNOA LiDAR, CC BY 4.0), landmarks that are not bridges (the Puerta de
  Alcalá), window frames and depth from up close, shadows of the buildings of the maps.

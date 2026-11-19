# The GT's sound: sources and licence

Every recording here is made from sounds published on [Freesound](https://freesound.org) under
**CC0 1.0** (the [public domain dedication](https://creativecommons.org/publicdomain/zero/1.0/)):
anyone may copy, change and share them, for any purpose, without asking. Each one's licence was
checked on its own page (17 Nov 2026). No credit is required by the licence; it is given anyway.

They were taken from Freesound's high-quality previews (MP3 of 48 kHz, the same recordings as the
originals at a lower bit rate), which need no account. Nothing here comes from anywhere else.

## What was done to them

The scripts that did it are not in the repository; this is what they did.

- **The engine's loops** (`engine_<mic>_<on|off>_<rpm>.wav`): each recording's firing frequency
  was measured (on its spectrum; for the sweeps, tracked frame by frame and fitted with a smooth
  curve), and its rpm worked out as that frequency x 120 / 4 (a four-cylinder, four-stroke engine).
  A slice of it was resampled so its pitch is constant (the sweeps flattened against the tracked
  frequency; the steady clips only by a hair, so the slice holds a whole number of engine cycles),
  cut to a whole number of engine cycles (two turns of the crank) and crossfaded over its loop
  point (the steady clips over two cycles or 40 ms, the longer; the slices of sweeps over 40 % of their length),
  high-passed at 20 Hz and normalised to the same loudness (-18 dBFS RMS). The rpm in the file's name
  and in `sound.ron` is the measured one.
- **Loops of noise** (`squeal`, `skid`, `road`, `gravel`, `wind`, `scrape`): a slice, high-passed
  (30 to 300 Hz), its swells of loudness evened out (divided by its envelope over 0.4 s), its end
  crossfaded into its start (equal power), normalised.
- **The kerb** (`kerb.wav`): a slice of a car on a rumble strip, its beat (97.8 Hz) flattened to one
  pitch, cut to whole beats, crossfaded, normalised; `sound.ron` gives its beat.
- **One-shots** (`impact`, `starter`): a slice, faded in and out, normalised to its peak.

All are mono WAV, 16-bit PCM at 48 kHz.

## The engine: "The Dyno Sessions: 190E"

A Mercedes-Benz 190E 2.3-16V (Cosworth-headed four-cylinder) on a chassis dynamometer, recorded
for a Kickstarter-funded session by David of Track Time Audio, published by
[Chippy569](https://freesound.org/people/Chippy569/) as the pack
[The Dyno Sessions: 190E](https://freesound.org/people/Chippy569/packs/9658/). Microphones: a DPA
4062 taped to the radiator support above the driver's headlight (`Bay` in `sound.ron`), an Audix D6
a foot behind the exhaust outlet (`Exhaust`).

| Ours | Title | Page | What it is |
|---|---|---|---|
| `engine_exhaust_on_1289.wav` | 19_500_D6.wav | <https://freesound.org/people/Chippy569/sounds/162506/> | full load, steady (labelled ~500 rpm; measured 1289) |
| `engine_exhaust_on_2556.wav` | 19_2500_D6.wav | <https://freesound.org/people/Chippy569/sounds/162509/> | full load, steady |
| `engine_exhaust_on_6208.wav` | 20_6500_D6.wav | <https://freesound.org/people/Chippy569/sounds/162512/> | full load, steady |
| `engine_exhaust_on_1800.wav`, `_3400`, `_4300`, `_5200`, `_6700` | 17_ShortSweep_D6.wav | <https://freesound.org/people/Chippy569/sounds/162501/> | slices of an acceleration sweep |
| `engine_exhaust_off_1016.wav` | 21_500_D6.wav | <https://freesound.org/people/Chippy569/sounds/162515/> | no load, steady (idle) |
| `engine_exhaust_off_2430.wav` | 21_2500_D6.wav | <https://freesound.org/people/Chippy569/sounds/162518/> | no load, steady |
| `engine_exhaust_off_6294.wav` | 21_6500_D6.wav | <https://freesound.org/people/Chippy569/sounds/162521/> | no load, steady |
| `engine_exhaust_off_1600.wav`, `_3400`, `_4400`, `_5300`, `_6900` | 17_MedSweep_D6.wav | <https://freesound.org/people/Chippy569/sounds/162504/> | slices of a deceleration sweep |
| `engine_engine_on_1289.wav` | 19_500_4062.wav | <https://freesound.org/people/Chippy569/sounds/162507/> | full load, steady |
| `engine_engine_on_2556.wav` | 19_2500_4062.wav | <https://freesound.org/people/Chippy569/sounds/162510/> | full load, steady |
| `engine_engine_on_6207.wav` | 20_6500_4062.wav | <https://freesound.org/people/Chippy569/sounds/162513/> | full load, steady |
| `engine_engine_on_3400.wav`, `_4300`, `_5200`, `_6950` | Acceleration Sweep - 4062.wav | <https://freesound.org/people/Chippy569/sounds/162499/> | slices of an acceleration sweep |
| `engine_engine_off_1028.wav` | 21_500_4062.wav | <https://freesound.org/people/Chippy569/sounds/162516/> | no load, steady (idle) |
| `engine_engine_off_2430.wav` | 21_2500_4062.wav | <https://freesound.org/people/Chippy569/sounds/162519/> | no load, steady |
| `engine_engine_off_6296.wav` | 21_6500_4062.wav | <https://freesound.org/people/Chippy569/sounds/162522/> | no load, steady |
| `engine_engine_off_3400.wav`, `_4400`, `_5300`, `_6650` | 18_MedSweep_4062.wav | <https://freesound.org/people/Chippy569/sounds/162502/> | slices of a deceleration sweep |
| `starter.wav` | 23_StartBig_D6.wav | <https://freesound.org/people/Chippy569/sounds/162525/> | 0.45 to 3.2 s: the starter, the engine catching |

## The rest

| Ours | Title | Author | Page | Slice |
|---|---|---|---|---|
| `squeal.wav` | Chrysler LHS tire squeal 04 (04-25-2009).wav | audible-edge | <https://freesound.org/people/audible-edge/sounds/71739/> | 4.6 to 9.1 s |
| `skid.wav` | Nissan Maxima burnout (04-25-2009).wav | audible-edge | <https://freesound.org/people/audible-edge/sounds/71740/> | 3.6 to 5.3 s |
| `road.wav` | Tire noise on the highway / Bruit des pneus sur l'autoroute | ViveleVent | <https://freesound.org/people/ViveleVent/sounds/817389/> | 12 to 16 s |
| `kerb.wav` | rumble strip exit 41.wav | bsumusictech | <https://freesound.org/people/bsumusictech/sounds/62652/> | 3 s round 7.8 s |
| `gravel.wav` | Tires on Gravel Road 1 | OBXJohn | <https://freesound.org/people/OBXJohn/sounds/251661/> | 3 to 6 s |
| `wind.wav` | Wind-inside car-091007.wav | ra_gun | <https://freesound.org/people/ra_gun/sounds/82267/> | 60 to 65 s |
| `impact.wav` | Car Crash | squareal | <https://freesound.org/people/squareal/sounds/237375/> | 0.3 to 1.3 s (the metal hit, before the grit and glass) |
| `scrape.wav` | MetalScrape_4.wav | deerlord | <https://freesound.org/people/deerlord/sounds/534853/> | 2.6 to 5.0 s |

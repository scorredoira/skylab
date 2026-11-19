# The Boeing 747's flight deck, made in Blender:
#
#   /Applications/Blender.app/Contents/MacOS/Blender -b --python wip/b747/flight-deck.py
#
# writes wip/b747/cockpit.glb and cockpit.ron (what moves of it, where the eye can be,
# its instruments, what is printed on its panels). Run from the repository's root. (Its exterior
# is an imported model: wip/b747/source/import.py.)
#
# Everything is built in the game's body axes, as the game reads a model: x forward, y right,
# z down, metres from the centre of gravity, and exported as it is. The flight deck is on the
# upper deck: the captain's eye 5.2 m behind the nose and 8.7 m above the ground, 0.55 m left of
# the middle. A 747-100's, as it was flown by three: the two pilots' panels with the six flight
# instruments each, the engines' gauges between them (fan speed, exhaust temperature and fuel
# flow of the four), the gear's lever and lamps and the flaps' gauge; the glareshield with the
# autopilot's panel; the pedestal with the four thrust levers, the speed brake and flap levers
# and the trim wheels; the overhead panel; and behind the first officer the flight engineer's
# station. The objects are the nodes cockpit.ron names, each with its origin where it turns; the
# materials' names say what they are to the game (vinyl, plastic, fabric, carpet, metal, leather,
# chrome; `printed` the panels the legends are printed on; `dial` the instruments, each a square
# of the instruments' canvas by its texture coordinates).

import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from shapes import Builder, clear, loft, tube, superellipse, export  # noqa: E402

OUT = "wip/b747"

LINING = ("vinyl", (0.46, 0.43, 0.36), 0.0, 0.8)
LOW = ("vinyl.low", (0.20, 0.20, 0.19), 0.0, 0.8)
HOOD = ("vinyl.hood", (0.015, 0.015, 0.015), 0.0, 0.92)
PANEL = ("printed", (0.17, 0.19, 0.19), 0.0, 0.55)
PLATE = ("plastic.plate", (0.15, 0.17, 0.17), 0.0, 0.55)
TRIM = ("plastic.trim", (0.03, 0.032, 0.034), 0.0, 0.5)
FLOOR = ("carpet", (0.07, 0.075, 0.09), 0.0, 0.95)
CLOTH = ("fabric", (0.30, 0.27, 0.20), 0.0, 0.95)
FLEECE = ("fabric.fleece", (0.62, 0.58, 0.48), 0.0, 1.0)
HIDE = ("leather", (0.03, 0.03, 0.03), 0.0, 0.6)
METAL = ("metal", (0.5, 0.51, 0.52), 1.0, 0.4)
SHINE = ("chrome", (0.8, 0.8, 0.8), 1.0, 0.1)
WHITE = ("plastic.white", (0.8, 0.8, 0.76), 0.0, 0.4)
RED = ("plastic.red", (0.6, 0.05, 0.04), 0.0, 0.4)
DIALS = ("dial", (1.0, 1.0, 1.0), 0.0, 0.3)
WINDOW = ("glass", (0.75, 0.82, 0.85), 0.0, 0.06)

# The cabin: a round upper deck from the bulkhead behind the flight engineer to the nose, where
# it closes as a dome; the windows are cut out of its lining.
BACK, NOSE = 24.0, 27.58
CENTRE_Z = -2.9
FLOOR_Z = -2.1
SILL, HEAD = 86.0, 40.0
SIDE_WINDOWS = (25.5, 26.55)
POSTS = (25.5, 26.02, 26.55)
# The panel's face, under the hood; where the pilots sit.
PANEL_X = 26.95
PANEL_Y = 1.25
PANEL_Z = (-3.02, -2.42)
SEATS = (-0.55, 0.55)
EYE = (26.2, SEATS[0], -3.3)

# The instruments' canvas (cockpit.ron's): cells of 256 points, five across and four down, and
# what is painted in each.
CELL = 256.0
CANVAS = (5 * CELL, 4 * CELL)
INSTRUMENTS = ["Airspeed", "Attitude", "Altimeter", "TurnCoordinator", "Heading", "VerticalSpeed", "FlapPosition", "GearLamps"]
INSTRUMENTS += [f"FanSpeed(engine: {i})" for i in range(4)] + [f"ExhaustTemperature(engine: {i})" for i in range(4)] + [f"FuelFlow(engine: {i})" for i in range(4)]
CELLS = {name: (k % 5, k // 5) for k, name in enumerate(INSTRUMENTS)}

# Each pilot's six, from the seat's middle (y, z, half its side).
SIX = (
    ("Airspeed", -0.175, -2.855, 0.062),
    ("Attitude", 0.0, -2.855, 0.078),
    ("Altimeter", 0.175, -2.855, 0.062),
    ("TurnCoordinator", -0.175, -2.68, 0.062),
    ("Heading", 0.0, -2.68, 0.078),
    ("VerticalSpeed", 0.175, -2.68, 0.062),
)
# The engines' gauges in the middle: a column for each engine, a row for each kind.
ENGINE_Y = [-0.165, -0.055, 0.055, 0.165]
ENGINE_ROWS = (("FanSpeed", -2.925, "N1"), ("ExhaustTemperature", -2.815, "EGT"), ("FuelFlow", -2.705, "FUEL FLOW"))
ENGINE_HALF = 0.046

# What is printed: the canvas, in points, its part for the panel across the top (2048 points
# for its width) and under it the part for the pedestal's top.
PRINT_W = 2048.0
PANEL_PRINT = (-PANEL_Y, PANEL_Z[0], 2 * PANEL_Y, PANEL_Z[1] - PANEL_Z[0])
PANEL_POINTS = PRINT_W * PANEL_PRINT[3] / PANEL_PRINT[2]
PEDESTAL = (25.72, 26.9, 0.25)
PEDESTAL_Z = -2.71
# (The floor's part is a square of the canvas: a square of the cabin, the pedestal along its middle.)
FLOOR_SIDE = PEDESTAL[1] - PEDESTAL[0]
FLOOR_PRINT = (-FLOOR_SIDE / 2, PEDESTAL[0], FLOOR_SIDE, FLOOR_SIDE)
FLOOR_POINTS = 1024.0
PRINT_H = PANEL_POINTS + FLOOR_POINTS

INK = (240, 240, 230, 255)
DIM = (200, 200, 190, 255)
# How much larger than asked the legends are printed: a point of the canvas is over a millimetre.
LETTERS = 1.7
SEAM = (12, 13, 13, 255)
shapes = []


def on_panel(y, z):
    return ((y - PANEL_PRINT[0]) / PANEL_PRINT[2] * PRINT_W, (z - PANEL_PRINT[1]) / PANEL_PRINT[3] * PANEL_POINTS)


def on_pedestal(y, x):
    # Forward is up.
    return ((y - FLOOR_PRINT[0]) / FLOOR_PRINT[2] * FLOOR_POINTS, PANEL_POINTS + (1.0 - (x - FLOOR_PRINT[1]) / FLOOR_PRINT[3]) * FLOOR_POINTS)


def text(at, words, size=11.0, color=INK, centred=True):
    x, y = at
    size *= LETTERS
    if centred:
        x -= 0.29 * size * len(words)
    shapes.append(f'Text(at: ({x:.1f}, {y - size * 0.6:.1f}), text: "{words}", size: {size:.1f}, color: {color})')


def rect(lo, hi, stroke=1.5, color=SEAM, fill=None, radius=3):
    filled = f", fill: {fill}" if fill else ""
    shapes.append(f"Rect(min: ({min(lo[0], hi[0]):.1f}, {min(lo[1], hi[1]):.1f}), max: ({max(lo[0], hi[0]):.1f}, {max(lo[1], hi[1]):.1f}), radius: ({radius}, {radius}, {radius}, {radius}){filled}, stroke: ({stroke:.1f}, {color}))")


def dot(at, radius=2.2, fill=SEAM):
    shapes.append(f"Circle(centre: ({at[0]:.1f}, {at[1]:.1f}), radius: {radius:.1f}, fill: {fill})")


def plate(y0, y1, z0, z1, title=None):
    """A plate of the panel: its seam, a screw in each corner, its name along its top."""
    rect(on_panel(y0, z0), on_panel(y1, z1))
    for y in (y0 + 0.012, y1 - 0.012):
        for z in (z0 + 0.012, z1 - 0.012):
            dot(on_panel(y, z))
    if title:
        text(on_panel((y0 + y1) / 2, z0 + 0.016), title, 9.0, DIM)


def half_width(x):
    if x <= 26.5:
        return 1.8 - 0.35 * (x - BACK) / (26.5 - BACK)
    return 1.45 * math.sqrt(max(0.0, 1.0 - ((x - 26.5) / 1.55) ** 2))


def roof(x):
    if x <= 26.55:
        return 1.22
    return 1.22 * math.sqrt(max(0.0, 1.0 - ((x - 26.55) / 1.05) ** 2))


def hull(x, a, inset=0.0):
    """A point of the cabin's wall at x, `a` round from the top (radians, to the right)."""
    c = math.cos(a)
    up = roof(x) if c > 0 else 0.92
    return (x, (half_width(x) - inset) * math.sin(a), CENTRE_Z - (up - inset) * c)


def is_window(x, a):
    a = abs(math.degrees(math.atan2(math.sin(a), math.cos(a))))
    if SIDE_WINDOWS[0] < x < SIDE_WINDOWS[1]:
        return HEAD < a < SILL
    return SIDE_WINDOWS[1] <= x < NOSE and 5.0 < a < SILL


def cockpit():
    b = Builder()
    around = 120
    xs = [BACK + (NOSE - BACK) * (1 - (1 - i / 89) ** 1.5) for i in range(90)]
    rings = [[hull(x, math.tau * k / around) for k in range(around)] for x in xs]

    def lined(i, k):
        x, a = (xs[i] + xs[i + 1]) / 2, math.tau * (k + 0.5) / around
        if is_window(x, a):
            return None
        return LINING if math.cos(a) > math.cos(math.radians(SILL)) else LOW

    # The lining, facing in.
    shell = Builder()
    loft(shell, rings, LINING, colour=lined)
    names = {index: material for index, material in shell.slots.values()}
    for face, slot in zip(shell.faces, shell.materials):
        corners = [shell.points[i] for i in face]
        centre = [sum(c[j] for c in corners) / len(corners) for j in range(3)]
        b.face(corners, names[slot], towards=(0.0, -centre[1], CENTRE_Z - centre[2]))
    # The bulkhead behind, with its door, and the floor.
    ring = rings[0]
    for k in range(around):
        b.face([(BACK, 0.0, CENTRE_Z), ring[k], ring[(k + 1) % around]], LINING, towards=(1.0, 0.0, 0.0))
    b.box((BACK, -0.38, -3.85), (BACK + 0.03, 0.38, FLOOR_Z), PLATE, round=0.01)
    b.box((BACK + 0.02, 0.26, -3.05), (BACK + 0.06, 0.3, -2.95), SHINE, round=0.01)
    b.face([(BACK, -1.8, FLOOR_Z), (27.3, -1.2, FLOOR_Z), (27.3, 1.2, FLOOR_Z), (BACK, 1.8, FLOOR_Z)], FLOOR, towards=(0.0, 0.0, -1.0))

    # The windows' frames: posts up the wall, and the sills and heads along it.
    def bar(point, r=0.03, steps=24):
        tube(b, [point(i / steps) for i in range(steps + 1)], r, TRIM, sides=8)

    sill, head = math.radians(SILL), math.radians(HEAD)
    for s in (-1, 1):
        bar(lambda t, s=s: hull(SIDE_WINDOWS[0] + (NOSE - 0.02 - SIDE_WINDOWS[0]) * t, s * sill, 0.012), 0.032, 40)
        bar(lambda t, s=s: hull(SIDE_WINDOWS[0] + (SIDE_WINDOWS[1] - SIDE_WINDOWS[0]) * t, s * head, 0.012), 0.03)
        for x in POSTS:
            top = head if x < SIDE_WINDOWS[1] else math.radians(5.0)
            bar(lambda t, x=x, s=s, top=top: hull(x, s * (top + (sill - top) * t), 0.012), 0.036 if x == SIDE_WINDOWS[1] else 0.028)
        # The middle post of the windscreen, either side of the centre line, down to the hood.
        bar(lambda t, s=s: hull(SIDE_WINDOWS[1] + (NOSE - 0.03 - SIDE_WINDOWS[1]) * t, s * math.radians(5.0), 0.012), 0.03)
    # Along the roof between the windscreen's two halves.
    b.box((SIDE_WINDOWS[1] - 0.05, -0.12, CENTRE_Z - 1.2), (SIDE_WINDOWS[1] + 0.4, 0.12, CENTRE_Z - 1.12), TRIM, round=0.02)

    # The hood over the panel, with the autopilot's panel in its face, and the panel.
    hood_z = PANEL_Z[0] - 0.035
    for i in range(12):
        # Across, it follows the windscreen's foot round.
        y0, y1 = -PANEL_Y - 0.1 + (2 * PANEL_Y + 0.2) * i / 12, -PANEL_Y - 0.1 + (2 * PANEL_Y + 0.2) * (i + 1) / 12
        b.box((PANEL_X - 0.09, y0, hood_z - 0.03), (27.5, y1, hood_z + 0.012), HOOD)
    b.box((PANEL_X - 0.11, -PANEL_Y - 0.1, hood_z - 0.035), (PANEL_X - 0.08, PANEL_Y + 0.1, hood_z + 0.02), HOOD, round=0.012)
    b.box((PANEL_X - 0.125, -0.34, hood_z - 0.085), (PANEL_X + 0.05, 0.34, hood_z - 0.025), PLATE, round=0.006)
    for k in range(9):
        y = -0.3 + 0.075 * k
        if k % 2 == 0:
            tube(b, [(PANEL_X - 0.125, y, hood_z - 0.055), (PANEL_X - 0.143, y, hood_z - 0.055)], 0.011, TRIM, sides=12)
        else:
            b.box((PANEL_X - 0.131, y - 0.02, hood_z - 0.07), (PANEL_X - 0.125, y + 0.02, hood_z - 0.04), HIDE, round=0.003)
    b.face([(PANEL_X, -PANEL_Y, PANEL_Z[0]), (PANEL_X, PANEL_Y, PANEL_Z[0]), (PANEL_X, PANEL_Y, PANEL_Z[1]), (PANEL_X, -PANEL_Y, PANEL_Z[1])], PANEL, towards=(-1.0, 0.0, 0.0))
    b.box((PANEL_X + 0.004, -PANEL_Y - 0.25, PANEL_Z[0] - 0.02), (PANEL_X + 0.08, PANEL_Y + 0.25, PANEL_Z[1]), PLATE)
    # Under it, back to the nose's wall; the pedals' wells are dark.
    b.box((PANEL_X + 0.03, -PANEL_Y - 0.2, PANEL_Z[1]), (27.3, PANEL_Y + 0.2, FLOOR_Z), TRIM)

    def dial(name, y, z, half, x=PANEL_X, facing=(-1.0, 0.0, 0.0)):
        cell = CELLS[name]
        m = 0.007
        face_x = x - 0.004
        for lo, hi in (((y - half - m, z - half - m), (y + half + m, z - half)), ((y - half - m, z + half), (y + half + m, z + half + m)), ((y - half - m, z - half), (y - half, z + half)), ((y + half, z - half), (y + half + m, z + half))):
            b.box((face_x - 0.006, lo[0], lo[1]), (x, hi[0], hi[1]), TRIM)
        corners = [(face_x, y - half, z - half), (face_x, y + half, z - half), (face_x, y + half, z + half), (face_x, y - half, z + half)]
        uv = [((cell[0] + u) * CELL / CANVAS[0], (cell[1] + v) * CELL / CANVAS[1]) for u, v in ((0, 0), (1, 0), (1, 1), (0, 1))]
        b.face(corners, DIALS, towards=facing, uv=uv)

    for seat, who in zip(SEATS, ("CAPTAIN", "FIRST OFFICER")):
        for name, y, z, half in SIX:
            dial(name, seat + y, z, half)
        plate(seat - 0.27, seat + 0.27, PANEL_Z[0] + 0.012, PANEL_Z[1] - 0.105, who)
        # A row of annunciators under the six, and the clock's plate beside them.
        plate(seat - 0.27, seat + 0.27, PANEL_Z[1] - 0.1, PANEL_Z[1] - 0.012)
        for k, word in enumerate(("MARKER", "A/P DISC", "ALT ALERT", "INST WARN", "STAB TRIM", "SPEED BRK")):
            y = seat - 0.225 + 0.09 * k
            rect(on_panel(y - 0.036, PANEL_Z[1] - 0.085), on_panel(y + 0.036, PANEL_Z[1] - 0.045), 1.0, (60, 60, 55, 255), (16, 16, 15, 255), 2)
            text(on_panel(y, PANEL_Z[1] - 0.064), word, 6.5, (190, 150, 40, 255))
    for name, z, word in ENGINE_ROWS:
        for i, y in enumerate(ENGINE_Y):
            dial(f"{name}(engine: {i})", y, z, ENGINE_HALF)
        text(on_panel(-0.245, z + 0.004), word, 7.5, DIM)
    for i, y in enumerate(ENGINE_Y):
        text(on_panel(y, PANEL_Z[0] + 0.03), str(i + 1), 12.0)
    plate(-0.27, 0.27, PANEL_Z[0] + 0.012, PANEL_Z[1] - 0.012, "ENGINES")
    # Between the engines' plate and each pilot's: the flaps' gauge on the left, the gear on the right.
    plate(-PANEL_Y + 0.42, -0.28, PANEL_Z[0] + 0.012, PANEL_Z[1] - 0.012)
    plate(0.28, PANEL_Y - 0.42 - 0.54 + 0.54, PANEL_Z[0] + 0.012, PANEL_Z[1] - 0.012)
    gear_y = 0.2825
    # (The pilots' plates begin 0.27 from the seats: there is no room between; the gear and the
    # flaps go on the outer plates.)
    outer = PANEL_Y - 0.19
    dial("FlapPosition", -outer, -2.86, 0.062)
    text(on_panel(-outer, -2.95), "FLAPS", 9.0)
    dial("GearLamps", outer, -2.9, 0.07)
    text(on_panel(outer, -2.99), "LANDING GEAR", 9.0)
    for word, z in (("UP", -2.775), ("OFF", -2.715), ("DN", -2.655)):
        text(on_panel(outer + 0.07, z), word, 8.0)
    plate(-PANEL_Y + 0.012, -PANEL_Y + 0.4, PANEL_Z[0] + 0.012, PANEL_Z[1] - 0.012)
    plate(PANEL_Y - 0.4, PANEL_Y - 0.012, PANEL_Z[0] + 0.012, PANEL_Z[1] - 0.012)
    text(on_panel(-outer, -2.56), "BOEING 747", 10.0, DIM)
    text(on_panel(-outer, -2.52), "JA8157", 13.0)
    for k, word in enumerate(("RAD ALT", "CLOCK")):
        # Small round faces drawn, not instruments: the clock and the radio altimeter's.
        at = on_panel(-outer, -2.72 + 0.0 * k) if k == 0 else on_panel(outer - 0.08, -2.56)
        shapes.append(f"Circle(centre: ({at[0]:.1f}, {at[1]:.1f}), radius: 34.0, fill: (8, 8, 8, 255), stroke: (2.0, {SEAM}))")
        for tick in range(12):
            t = math.tau * tick / 12
            shapes.append(f"Line(from: ({at[0] + 27 * math.sin(t):.1f}, {at[1] - 27 * math.cos(t):.1f}), to: ({at[0] + 32 * math.sin(t):.1f}, {at[1] - 32 * math.cos(t):.1f}), width: 1.5, color: {INK})")
        text((at[0], at[1] + 12), word, 6.5, DIM)

    # The pedestal between the seats: its top printed, the trim wheels at its sides, radios behind.
    x0, x1, w = PEDESTAL
    b.box((x0, -w, PEDESTAL_Z + 0.004), (x1 + 0.08, w, FLOOR_Z), PLATE, round=0.015)
    b.face([(x0, -w, PEDESTAL_Z), (x1, -w, PEDESTAL_Z), (x1, w, PEDESTAL_Z), (x0, w, PEDESTAL_Z)], PANEL, towards=(0.0, 0.0, -1.0))
    for s in (-1, 1):
        # A trim wheel: a disc on its side, with a white stripe.
        rim = [(26.36 + 0.115 * math.cos(math.tau * k / 28), s * (w + 0.012), PEDESTAL_Z - 0.01 + 0.115 * math.sin(math.tau * k / 28)) for k in range(28)]
        inner = [(p[0], s * (w + 0.04), p[2]) for p in rim]
        loft(b, [rim, inner], TRIM, caps=True)
        b.box((26.36 - 0.01, s * (w + 0.04), PEDESTAL_Z - 0.125), (26.36 + 0.01, s * (w + 0.043), PEDESTAL_Z - 0.01), WHITE)
    for k, (word, lo, hi) in enumerate((("THRUST", 26.3, 26.62), ("SPEED BRAKE", 26.3, 26.62), ("FLAP", 26.2, 26.55))):
        y = (0.0, -0.185, 0.185)[k]
        half = (0.12, 0.035, 0.035)[k]
        rect(on_pedestal(y - half, hi), on_pedestal(y + half, lo), 1.5, SEAM, (10, 10, 10, 255), 4)
        text(on_pedestal(y, hi + 0.035), word, 8.0)
    for word, x in (("DOWN", 26.59), ("ARMED", 26.53), ("FLIGHT", 26.44), ("UP", 26.33)):
        text(on_pedestal(-0.13, x), word, 5.5, DIM)
    for word, x in (("UP", 26.53), ("1", 26.49), ("5", 26.45), ("10", 26.41), ("20", 26.37), ("25", 26.33), ("30", 26.29)):
        text(on_pedestal(0.135, x), word, 6.0, DIM)
    for k, word in enumerate(("VHF 1", "VHF 2", "ADF 1", "ADF 2", "ATC", "WX RADAR")):
        y, x = (-0.125, 0.125)[k % 2], 26.13 - 0.13 * (k // 2)
        rect(on_pedestal(y - 0.11, x + 0.055), on_pedestal(y + 0.11, x - 0.055), 1.5, SEAM, (22, 24, 24, 255), 3)
        rect(on_pedestal(y - 0.08, x + 0.03), on_pedestal(y + 0.02, x - 0.005), 1.0, SEAM, (4, 8, 5, 255), 2)
        text(on_pedestal(y - 0.03, x + 0.012), ("118.30", "121.50", " 362", " 1020", "2000", "")[k], 9.0, (90, 230, 120, 255))
        text(on_pedestal(y, x - 0.03), word, 6.5, DIM)
        b.box((x - 0.012, y + 0.05, PEDESTAL_Z - 0.014), (x + 0.012, y + 0.074, PEDESTAL_Z), TRIM, round=0.005)
    plate_points = on_pedestal(-w, x1), on_pedestal(w, x0)
    rect(plate_points[0], plate_points[1], 2.0)

    # The overhead panel: rows of switches and lamps on a board under the roof.
    b.box((25.35, -0.5, CENTRE_Z - 1.13), (26.5, 0.5, CENTRE_Z - 1.06), PLATE, round=0.02)
    for row in range(7):
        for k in range(12):
            x, y = 25.44 + 0.155 * row, -0.44 + 0.08 * k
            z = CENTRE_Z - 1.06
            if (row + k) % 3 == 0:
                b.box((x, y - 0.014, z), (x + 0.028, y + 0.014, z + 0.012), WHITE if (row * 5 + k) % 4 else RED, round=0.004)
            else:
                tube(b, [(x + 0.014, y, z), (x + 0.02, y, z + 0.03)], 0.006, SHINE, sides=8)
                b.box((x - 0.006, y - 0.02, z - 0.001), (x + 0.036, y + 0.02, z + 0.003), TRIM)

    # The flight engineer's station, behind the first officer, facing in: a wall of gauges and
    # switches over a desk.
    fe_y = 1.28
    b.box((24.35, fe_y, -3.85), (25.4, fe_y + 0.3, -2.75), PLATE, round=0.02)
    b.box((24.35, fe_y - 0.42, -2.78), (25.4, fe_y + 0.05, -2.74), PLATE, round=0.015)
    for i in range(4):
        for row, name in enumerate(("FanSpeed", "ExhaustTemperature", "FuelFlow")):
            cell = CELLS[f"{name}(engine: {i})"]
            x, z, half = 25.28 - 0.115 * i, -3.72 + 0.115 * row, 0.047
            corners = [(x + half, fe_y - 0.004, z - half), (x - half, fe_y - 0.004, z - half), (x - half, fe_y - 0.004, z + half), (x + half, fe_y - 0.004, z + half)]
            uv = [((cell[0] + u) * CELL / CANVAS[0], (cell[1] + v) * CELL / CANVAS[1]) for u, v in ((0, 0), (1, 0), (1, 1), (0, 1))]
            b.face(corners, DIALS, towards=(0.0, -1.0, 0.0), uv=uv)
            b.box((x - half - 0.006, fe_y - 0.012, z - half - 0.006), (x + half + 0.006, fe_y - 0.002, z - half), TRIM)
            b.box((x - half - 0.006, fe_y - 0.012, z + half), (x + half + 0.006, fe_y - 0.002, z + half + 0.006), TRIM)
    for row in range(5):
        for k in range(11):
            x, z = 24.42 + 0.045 * k + (0.0 if k < 6 else 0.02), -3.36 + 0.1 * row
            if x > 24.88 and row < 0:
                continue
            if (row + k) % 4 == 0:
                tube(b, [(x, fe_y, z), (x, fe_y - 0.03, z + 0.008)], 0.006, SHINE, sides=8)
            elif (row * 3 + k) % 5 == 0:
                b.box((x - 0.014, fe_y - 0.012, z - 0.014), (x + 0.014, fe_y, z + 0.014), RED if k % 2 else WHITE, round=0.004)
            else:
                ring = [(x + 0.017 * math.cos(math.tau * j / 14), fe_y - 0.004, z + 0.017 * math.sin(math.tau * j / 14)) for j in range(14)]
                for j in range(14):
                    b.face([(x, fe_y - 0.004, z), ring[j], ring[(j + 1) % 14]], HIDE, towards=(0.0, -1.0, 0.0))
                loft(b, [[(p[0], fe_y, p[2]) for p in ring], ring], TRIM)

    # The seats: the two pilots' and the flight engineer's (turned to his panel), each a pan, a
    # back with its fleece cover, a headrest and armrests, on its base.
    def seat(x, y, heading=0.0):
        c, s = math.cos(heading), math.sin(heading)
        at = lambda dx, dy, z: (x + dx * c - dy * s, y + dx * s + dy * c, z)

        def slab(lo, hi, material, round):
            # A box turned with the seat: skinned over its corners.
            rows = []
            for z in (lo[2], hi[2]):
                rows.append([at(dx, dy, z) for dx, dy in ((lo[0], lo[1]), (hi[0], lo[1]), (hi[0], hi[1]), (lo[0], hi[1]))])
            loft(b, rows, material, caps=True)

        slab((-0.05, -0.25, -2.6), (0.5, 0.25, -2.48), CLOTH, 0.05)
        rows = []
        for i in range(9):
            t = i / 8
            z = -2.55 - 0.74 * t
            w = 0.245 - 0.04 * t * t
            rows.append([at(-0.02 - 0.13 * t + 0.06 * superellipse(math.tau * k / 20, 3.0)[0], w * superellipse(math.tau * k / 20, 3.0)[1], z) for k in range(20)])
        loft(b, rows, FLEECE, caps=True)
        slab((-0.21, -0.13, -3.54), (-0.11, 0.13, -3.32), HIDE, 0.04)
        for side in (-1, 1):
            slab((0.0, side * 0.27 - 0.03, -2.82), (0.42, side * 0.27 + 0.03, -2.78), HIDE, 0.015)
        slab((0.02, -0.2, -2.48), (0.42, 0.2, FLOOR_Z), TRIM, 0.02)

    for y in SEATS:
        seat(25.6, y)
        # Each yoke's column, from the floor before the seat, and the pedals.
        tube(b, [(26.66, y, FLOOR_Z), (26.63, y, -2.45), (26.57, y, -2.64)], 0.032, TRIM, sides=12, smooth_path=True)
        for side in (-1, 1):
            b.box((26.78, y + side * 0.13 - 0.05, -2.34), (26.8, y + side * 0.13 + 0.05, -2.18), METAL, round=0.008)
            tube(b, [(26.8, y + side * 0.13, -2.26), (26.92, y + side * 0.13, -2.2)], 0.012, METAL, sides=8)
    seat(24.9, 0.75, math.pi / 2)
    # The pilots' side consoles, with the nose wheel's tiller on the captain's.
    for s in (-1, 1):
        b.box((25.3, s * 1.0, -2.72), (26.9, s * 1.52, -2.62), PLATE, round=0.02)
    tube(b, [(26.35, -1.12, -2.72), (26.35, -1.12, -2.8)], 0.012, METAL, sides=8)
    tube(b, [(26.35, -1.12, -2.8), (26.27, -1.12, -2.82)], 0.016, HIDE, sides=10)
    cabin = b.object("cabin")

    # The yokes: a boss on the column and two horns up and round. Each turns about its shaft with
    # the ailerons and comes back with the elevator.
    for name, y in (("yoke_left", SEATS[0]), ("yoke_right", SEATS[1])):
        k = Builder()
        hub = (26.57, y, -2.64)
        k.box((hub[0] - 0.045, y - 0.08, hub[2] - 0.04), (hub[0] + 0.02, y + 0.08, hub[2] + 0.04), TRIM, round=0.02)
        k.box((hub[0] - 0.048, y - 0.035, hub[2] - 0.02), (hub[0] - 0.044, y + 0.035, hub[2] + 0.02), WHITE, round=0.004)
        for side in (-1, 1):
            path = [(hub[0] - 0.015, y + side * 0.07, hub[2]), (hub[0] - 0.02, y + side * 0.17, hub[2] + 0.015), (hub[0] - 0.03, y + side * 0.215, hub[2] - 0.05), (hub[0] - 0.035, y + side * 0.2, hub[2] - 0.15), (hub[0] - 0.035, y + side * 0.17, hub[2] - 0.18)]
            tube(k, path, 0.017, HIDE, sides=12, smooth_path=True)
        k.object(name, origin=hub, parent=cabin)

    # The four thrust levers on one hinge in the pedestal, forward with the throttle; the speed
    # brake lever at their left, back with the speed brakes; the flap lever at their right,
    # forward with the flaps up.
    hinge = (26.42, 0.0, PEDESTAL_Z + 0.02)
    t = Builder()
    for i in range(4):
        y = -0.078 + 0.052 * i
        tube(t, [(hinge[0], y, hinge[2]), (hinge[0] - 0.075, y, hinge[2] - 0.225)], 0.008, METAL, sides=8)
        t.box((hinge[0] - 0.098, y - 0.021, hinge[2] - 0.262), (hinge[0] - 0.055, y + 0.021, hinge[2] - 0.222), WHITE, round=0.014)
        # Its reverse lever, lying forward of it.
        tube(t, [(hinge[0] - 0.06, y, hinge[2] - 0.18), (hinge[0] + 0.02, y, hinge[2] - 0.2)], 0.006, METAL, sides=6)
    t.object("levers", origin=hinge, parent=cabin)
    brake_hinge = (26.5, -0.185, PEDESTAL_Z + 0.02)
    s = Builder()
    tube(s, [brake_hinge, (brake_hinge[0] + 0.05, -0.185, brake_hinge[2] - 0.17)], 0.008, METAL, sides=8)
    s.box((brake_hinge[0] + 0.035, -0.215, brake_hinge[2] - 0.2), (brake_hinge[0] + 0.065, -0.155, brake_hinge[2] - 0.165), HIDE, round=0.012)
    s.object("speed_brake", origin=brake_hinge, parent=cabin)
    flap_hinge = (26.4, 0.185, PEDESTAL_Z + 0.02)
    f = Builder()
    tube(f, [flap_hinge, (flap_hinge[0] - 0.07, 0.185, flap_hinge[2] - 0.16)], 0.008, METAL, sides=8)
    f.box((flap_hinge[0] - 0.09, 0.15, flap_hinge[2] - 0.185), (flap_hinge[0] - 0.05, 0.22, flap_hinge[2] - 0.155), WHITE, round=0.01)
    f.object("flap_lever", origin=flap_hinge, parent=cabin)
    # The gear lever on the right of the panel, a wheel for its knob: built up, it comes down.
    gear_hinge = (PANEL_X - 0.005, PANEL_Y - 0.19, -2.715)
    g = Builder()
    tube(g, [gear_hinge, (gear_hinge[0] - 0.1, gear_hinge[1], gear_hinge[2] - 0.045)], 0.008, METAL, sides=8)
    rim = [(gear_hinge[0] - 0.1 + 0.03 * math.cos(math.tau * k / 16), gear_hinge[1] - 0.012, gear_hinge[2] - 0.045 + 0.03 * math.sin(math.tau * k / 16)) for k in range(16)]
    loft(g, [rim, [(p[0], gear_hinge[1] + 0.012, p[2]) for p in rim]], HIDE, caps=True)
    g.object("gear_lever", origin=gear_hinge, parent=cabin)

    # The glass, drawn over the world: the wall where the windows are, a little inside it.
    glass = Builder()
    for i in range(len(xs) - 1):
        for k in range(around):
            x, a = (xs[i] + xs[i + 1]) / 2, math.tau * (k + 0.5) / around
            if is_window(x, a):
                corners = [hull(xs[i], math.tau * k / around, 0.004), hull(xs[i + 1], math.tau * k / around, 0.004), hull(xs[i + 1], math.tau * (k + 1) / around, 0.004), hull(xs[i], math.tau * (k + 1) / around, 0.004)]
                centre = [sum(c[j] for c in corners) / 4 for j in range(3)]
                glass.face(corners, WINDOW, towards=(0.0, centre[1], centre[2] - CENTRE_Z))
    glass.object("glass", parent=cabin)
    export(f"{OUT}/cockpit.glb")
    return hinge, flap_hinge


def ron(hinge, flap_hinge):
    """cockpit.ron: the parts and how they move, the eye's places, the instruments, the print."""
    fitted = "\n".join(f"        (kind: {name}, at: ({CELLS[name][0] * CELL:.1f}, {CELLS[name][1] * CELL:.1f}, {CELL:.1f}, {CELL:.1f}))," for name in INSTRUMENTS)
    printed = "\n".join(f"            {shape}," for shape in shapes)
    # Where the mouse takes the levers: the thrust levers' knobs at idle and at full, the flap
    # lever's with the flaps full and up.
    turned = lambda at, pivot, angle: (pivot[0] + (at[0] - pivot[0]) * math.cos(angle) + (at[2] - pivot[2]) * math.sin(angle), at[1], pivot[2] - (at[0] - pivot[0]) * math.sin(angle) + (at[2] - pivot[2]) * math.cos(angle))
    knob = (hinge[0] - 0.076, 0.0, hinge[2] - 0.242)
    flap_knob = (flap_hinge[0] - 0.07, 0.185, flap_hinge[2] - 0.17)
    point = lambda p: f"({p[0]:.4f}, {p[1]:.4f}, {p[2]:.4f})"
    return f"""// The cockpit of cockpit.glb: what moves of it with the controls, where the eye can be, the
// instruments and where they are painted, what is printed on its panels.
// `crates/games/flight/src/cockpit/file.rs` says what each field is. Body axes (x forward,
// y right, z down), metres, radians; the canvases in points.
// Written, with the model, by `wip/b747/flight-deck.py`: change it there.
(
    parts: [
        (node: "cabin"),
        // Each yoke turns with the ailerons and comes back with the elevator.
        (node: "yoke_left", moves: Yoke(turn: 1.2, travel: 0.12), axis: (1.0, 0.0, 0.0)),
        (node: "yoke_right", moves: Yoke(turn: 1.2, travel: 0.12), axis: (1.0, 0.0, 0.0)),
        // The four thrust levers go forward with the throttle, the flap lever with the flaps
        // coming up; the speed brake lever comes back with the speed brakes; the gear lever
        // comes down with the gear.
        (node: "levers", moves: Turn(by: Throttle, angle: {THROTTLE_TURN}), axis: (0.0, 1.0, 0.0)),
        (node: "flap_lever", moves: Turn(by: FlapLever, angle: {FLAP_TURN}), axis: (0.0, 1.0, 0.0)),
        (node: "speed_brake", moves: Turn(by: Spoilers, angle: 0.75), axis: (0.0, 1.0, 0.0)),
        (node: "gear_lever", moves: Turn(by: Gear, angle: 0.85), axis: (0.0, 1.0, 0.0)),
    ],
    glass: "glass",
    spots: [
        // The captain's seat, on the left.
        Seat(name: "captain", eye: {point(EYE)}, fov: 72.0, shift: 0.3),
        Seat(name: "captain, leaning in", eye: ({EYE[0] + 0.22}, {EYE[1]}, {EYE[2] + 0.1}), fov: 54.0, shift: 0.6),
        LookingAt(name: "panel", eye: (25.75, 0.0, -3.0), at: ({PANEL_X}, 0.0, -2.74), height: 0.9),
        LookingAt(name: "flight instruments", eye: (26.42, {SEATS[0]}, -2.95), at: ({PANEL_X}, {SEATS[0]}, -2.76), height: 0.42),
        LookingAt(name: "engines", eye: (26.45, 0.0, -2.98), at: ({PANEL_X}, 0.0, -2.8), height: 0.4),
        LookingAt(name: "pedestal", eye: (26.05, 0.0, -3.45), at: (26.4, 0.0, {PEDESTAL_Z}), height: 0.75),
        LookingAt(name: "flight engineer", eye: (25.32, 0.1, -3.5), at: (24.92, 1.28, -3.35), height: 1.0),
    ],
    sky_box: (({BACK}, -1.9, -4.2), (27.6, 1.9, {FLOOR_Z})),
    // Cells of 256 points.
    canvas: ({CANVAS[0]:.1f}, {CANVAS[1]:.1f}),
    instruments: [
{fitted}
    ],
    // The panel's plates and legends across the top of the canvas; under them the pedestal's top.
    print: (
        size: ({PRINT_W:.1f}, {PRINT_H:.1f}),
        panel: (min: ({PANEL_PRINT[0]}, {PANEL_PRINT[1]}), size: ({PANEL_PRINT[2]}, {PANEL_PRINT[3]:.4f}), top: 0.0, points: {PRINT_W:.1f}),
        floor: (min: ({FLOOR_PRINT[0]}, {FLOOR_PRINT[1]}), size: ({FLOOR_PRINT[2]}, {FLOOR_PRINT[3]:.4f}), top: {PANEL_POINTS:.1f}, points: {FLOOR_POINTS:.1f}),
        panel_x: {PANEL_X - 0.05},
        shapes: [
{printed}
        ],
    ),
    levers: [
        (Throttle, ({point(knob)}, {point(turned(knob, hinge, THROTTLE_TURN))})),
        (Flaps, ({point(flap_knob)}, {point(turned(flap_knob, flap_hinge, FLAP_TURN))})),
    ],
)
"""


THROTTLE_TURN = -0.95
FLAP_TURN = -0.9

clear()
hinges = cockpit()
with open(f"{OUT}/cockpit.ron", "w") as out:
    out.write(ron(*hinges))
print(f"{OUT}/cockpit.ron: {len(shapes)} shapes printed")

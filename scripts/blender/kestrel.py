# The Kestrel tilt-rotor's models, made in Blender:
#
#   /Applications/Blender.app/Contents/MacOS/Blender -b --python scripts/blender/kestrel.py
#
# writes assets/aircraft/kestrel/exterior.glb and cockpit.glb. Run from the repository's root.
#
# Everything is built in the game's body axes, as the game reads a model: x forward, y right,
# z down, metres from the centre of gravity, and exported as it is (no change of axes). The
# points the simulation flies are those of assets/aircraft/kestrel/multirotor.ron: the nacelles'
# pivot 0.4 m above the centre of gravity, the rings' middles 3.9 m out, the hubs 0.25 m over the
# pivot and 0.05 m under it, the rotors 2 m of radius, the skids 2.45 m below. The objects are
# the nodes exterior.ron names, each with its origin where it turns: airframe, canopy, elevator,
# nacelle_left and nacelle_right (at the pivot, upright), prop_1...prop_4 (at their hubs, in the
# motors' order), disc_1...disc_4, and the lights (empties). The cockpit's are those cockpit.ron
# names: cabin, stick, collective, pedals_left, pedals_right and glass; its materials' names say
# what they are to the game (vinyl, plastic, fabric, rubber, metal, leather; `screen` the
# displays, each a rectangle of the instruments' canvas by its texture coordinates).

import math
import os
import sys

import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from shapes import Builder, catmull, clear, loft, revolve, tube, aerofoil, superellipse, export  # noqa: E402

PIVOT_Z = -0.4
OUT = 3.9
HUBS = (-0.25, 0.05)
ROTOR = 2.0

# The paint: one colour each, (red, green, blue), metallic, roughness.
GREEN = ("paint", (0.075, 0.088, 0.082), 0.0, 0.55)
UNDER = ("paint_under", (0.05, 0.058, 0.056), 0.0, 0.6)
ROCKET = ("rocket", (0.75, 0.2, 0.03), 0.0, 0.5)
DARK = ("dark", (0.012, 0.013, 0.014), 0.0, 0.85)
GUN = ("gunmetal", (0.045, 0.048, 0.05), 0.9, 0.45)
STEEL = ("steel", (0.2, 0.21, 0.22), 1.0, 0.4)
GLASS = ("glass", (0.006, 0.012, 0.018), 0.0, 0.06)
FRAME = ("frame", (0.035, 0.042, 0.038), 0.0, 0.5)
BLADE = ("blade", (0.02, 0.021, 0.022), 0.0, 0.45)
TIP = ("blade_tip", (0.55, 0.42, 0.03), 0.0, 0.5)
BLUR = ("blur", (0.3, 0.31, 0.32), 0.0, 0.5)

# The hull: (x, half width, top, bottom, how square) of its sections from the tail to the nose.
# z down: the top is the smaller number.
HULL = [
    (-5.20, 0.12, 0.08, 0.42, 2.2),
    (-4.40, 0.20, 0.00, 0.62, 2.2),
    (-3.40, 0.36, -0.14, 1.00, 2.3),
    (-2.60, 0.62, -0.32, 1.52, 2.5),
    (-1.80, 0.90, -0.48, 1.98, 2.8),
    (-0.80, 1.00, -0.55, 2.10, 3.0),
    (0.60, 1.00, -0.55, 2.10, 3.0),
    (1.70, 1.00, -0.48, 2.10, 3.0),
    (2.60, 0.99, -0.26, 2.10, 3.0),
    (3.40, 0.97, 0.02, 2.10, 3.0),
    (4.10, 0.92, 0.36, 2.07, 3.0),
    (4.55, 0.83, 0.64, 2.00, 2.9),
    (4.85, 0.68, 0.90, 1.89, 2.7),
    (5.00, 0.46, 1.12, 1.74, 2.5),
    (5.06, 0.22, 1.30, 1.58, 2.2),
]
AROUND = 48


def hull_section(x):
    """The hull's section at x: half width, top, bottom and squareness."""
    return tuple(catmull([(s[0], s[k]) for s in HULL], x) for k in (1, 2, 3, 4))


def hull_point(x, a, out=0.0):
    """The point of the hull at x and angle a round it (0 the right side, a quarter turn the
    belly), `out` metres off its surface."""
    half, top, bottom, square = hull_section(x)
    half, height = max(half, 0.02) + out, (bottom - top) / 2 + out
    y, z = superellipse(a, square)
    return (x, half * y, (top + bottom) / 2 + height * z)


def hull_stations(first=-5.2, last=5.06, count=96):
    # Closer together at the nose, where it turns quickly.
    xs = []
    for i in range(count):
        t = i / (count - 1)
        t = 1 - (1 - t) ** 1.6
        xs.append(first + (last - first) * t)
    return xs


def angle_of(k):
    return math.tau * k / AROUND


# The glass. Above the belt, the windscreen and the side windows: from x = 2.3 to 4.62, the top
# and the sides down to `belt` above the widest of the section. Below it, a chin window each
# side of the nose: from x = 3.95 to 4.86, from `CHIN[0]` to `CHIN[1]` below the widest.
SCREEN = (2.3, 4.62)
CHIN_X = (3.7, 4.9)
CHIN = (0.0, 0.62)
TOP = -math.pi / 2


def belt(x):
    """How far above the side (an angle) the windscreen and side windows come down to at x."""
    return 0.17 + 0.08 * max(0.0, (x - 3.6))


def screen_angles(x, side):
    """From the middle of the roof down one side to the belt."""
    return (TOP, TOP + side * (math.pi / 2 - belt(x)))


def chin_angles(side):
    """A chin window's angles on one side (the right from the side towards the belly)."""
    return (CHIN[0], CHIN[1]) if side > 0 else (math.pi - CHIN[0], math.pi - CHIN[1])


def airframe():
    b = Builder()
    xs = hull_stations()
    # The hull, closed at both ends; darker under the waterline.
    rings = [[hull_point(x, angle_of(k)) for k in range(AROUND)] for x in xs]
    loft(b, rings, GREEN, caps=True, colour=lambda i, k: UNDER if 0.16 < (k + 0.5) / AROUND < 0.34 else GREEN)

    # The windows' frames: bars along the hull, over the glass (which is `canopy`).
    def bar(point, r=0.026):
        tube(b, [point(i / 32) for i in range(33)], r, FRAME, sides=8)

    def along(a, x0, x1, r=0.026):
        bar(lambda t: hull_point(x0 + (x1 - x0) * t, a(x0 + (x1 - x0) * t), 0.012), r)

    def across(x, a0, a1, r=0.026):
        bar(lambda t: hull_point(x, a0 + (a1 - a0) * t, 0.012), r)

    along(lambda x: TOP, SCREEN[0], SCREEN[1], r=0.034)
    for side in (1, -1):
        along(lambda x, s=side: screen_angles(x, s)[1], SCREEN[0], SCREEN[1])
        along(lambda x, s=side: TOP + s * 0.95, 3.25, SCREEN[1], r=0.022)
        for x in (SCREEN[0], 3.25, SCREEN[1]):
            across(x, *screen_angles(x, side))
        a0, a1 = chin_angles(side)
        along(lambda x, a=a0: a, *CHIN_X)
        along(lambda x, a=a1: a, *CHIN_X)
        for x in CHIN_X:
            across(x, a0, a1)

    # The hump on the roof (the gearbox) and the beam across it to the nacelles.
    hump = []
    for i in range(25):
        t = i / 24
        x = 1.15 - 3.2 * t
        w = 0.5 * math.sin(math.pi * min(1.0, 0.12 + t * 0.95)) ** 0.5 + 0.02
        h = 0.52 * math.sin(math.pi * min(1.0, 0.1 + t * 0.95)) ** 0.6
        hump.append([(x, w * superellipse(math.tau * k / 24, 2.6)[0], -0.45 - h / 2 + (h / 2 + 0.12) * superellipse(math.tau * k / 24, 2.6)[1]) for k in range(24)])
    loft(b, hump, GREEN, caps=True)
    beam = []
    for i in range(21):
        y = -1.78 + 3.56 * i / 20
        thin = 1 - 0.35 * abs(y) / 1.78
        beam.append([(0.02 + 0.66 * thin * superellipse(math.tau * k / 20, 3.0)[0], y, PIVOT_Z - 0.02 + 0.21 * thin * superellipse(math.tau * k / 20, 3.0)[1]) for k in range(20)])
    loft(b, beam, GREEN, caps=True)

    for side in (-1, 1):
        # A turboshaft on each shoulder: a barrel, its intake's lip round a dark mouth ahead, its
        # exhaust turned out and back behind.
        y = side * 0.78
        body = [(1.62, 0.25), (1.5, 0.31), (1.2, 0.335), (0.4, 0.34), (-0.6, 0.33), (-1.3, 0.29), (-1.6, 0.25)]
        revolve(b, [(r, x) for x, r in body], GREEN, centre=(0, y, -0.66), axis="x", sides=28)
        revolve(b, [(0.0, 1.5), (0.24, 1.5), (0.25, 1.62)], DARK, centre=(0, y, -0.66), axis="x", sides=28)
        pipe = [(-1.5, y, -0.66), (-1.8, y + side * 0.06, -0.67), (-2.1, y + side * 0.2, -0.68), (-2.35, y + side * 0.38, -0.69)]
        tube(b, pipe, 0.2, GUN, sides=16, cap=False)
        tube(b, [pipe[-1], (pipe[-1][0] + 0.04, pipe[-1][1] - side * 0.02, pipe[-1][2])], 0.17, DARK, sides=16)
        # The trunnion the nacelle turns on.
        revolve(b, [(0.0, 1.7), (0.2, 1.7), (0.17, 1.9), (0.0, 1.9)], STEEL, centre=(0, 0, PIVOT_Z), axis="y", sides=20, sign=side)

        # The hold's door, open: a dark opening, the door slid back, a step under it.
        b.box((-0.95, side * 0.985 - 0.03, 0.2), (0.6, side * 0.985 + 0.03, 1.85), DARK)
        b.box((-2.3, side * 1.0 - 0.025, 0.25), (-1.0, side * 1.0 + 0.045, 1.8), GREEN, round=0.03)
        b.box((-0.95, side * 1.0 - 0.16, 1.9), (0.6, side * 1.0 + 0.16, 1.94), GUN, round=0.015)

        # The skid: a tube turned up ahead and behind, on two bowed legs.
        ys = side * 1.15
        skid = [(-1.95, ys, 2.25), (-1.75, ys, 2.37), (-1.5, ys, 2.4), (1.8, ys, 2.4), (2.15, ys, 2.36), (2.45, ys, 2.2)]
        tube(b, skid, 0.05, DARK, sides=12, smooth_path=True)
        for x in (1.3, -1.0):
            leg = [(x, ys, 2.38), (x, side * 1.05, 2.2), (x, side * 0.85, 2.02), (x, side * 0.55, 1.95)]
            tube(b, leg, 0.045, GUN, sides=10, smooth_path=True)

        # The stub wing low on the cabin's side, and its pod: a barrel of tubes.
        section = aerofoil(0.14, 12)
        wing = []
        for i in range(9):
            y_ = side * (0.9 + 1.3 * i / 8)
            chord = 1.25 - 0.3 * i / 8
            wing.append([(0.2 + (0.3 - s) * chord, y_, 1.3 + t * chord) for s, t in section])
        loft(b, wing, GREEN, caps=True)
        # A pod of rockets on a pylon beside the nose: a box, six noses showing.
        b.box((3.3, side * 0.9, 1.62), (3.75, side * 1.25, 1.74), GREEN, round=0.03)
        b.box((2.95, side * 1.2, 1.5), (4.0, side * 1.72, 1.9), GUN, round=0.06)
        for row in range(2):
            for col in range(3):
                revolve(b, [(0.0, 4.07), (0.035, 4.04), (0.055, 3.98), (0.055, 3.9)], ROCKET, centre=(0, side * (1.31 + 0.15 * col), 1.61 + 0.18 * row), axis="x", sides=10)

        # A fin of the tail, leaning out: swept, 10 % thick.
        fin_section = aerofoil(0.10, 10)
        lean = math.radians(28)
        fin = []
        for i in range(9):
            up = i / 8
            lead, trail = -4.0 - 0.95 * up, -5.45 - 0.2 * up
            reach = 1.75 * up
            fin.append([(lead + (trail - lead) * s, side * (0.06 + reach * math.sin(lean)) + t * (lead - trail) * math.cos(lean), 0.1 - reach * math.cos(lean) + side * t * (lead - trail) * math.sin(lean)) for s, t in fin_section])
        loft(b, fin, GREEN, caps=True)

    # The tailplane across the boom, its front half fixed (the rear is `elevator`).
    plane = aerofoil(0.11, 12)
    front = [(s, t) for s, t in plane if s <= 0.5]
    tail = []
    for i in range(13):
        y = -1.7 + 3.4 * i / 12
        chord = 1.05 - 0.2 * abs(y) / 1.7
        tail.append([(-4.6 - 0.1 * abs(y) / 1.7 - s * chord, y, PIVOT_Z + t * chord) for s, t in front])
    loft(b, tail, GREEN, caps=True)

    # On the nose's left cheek, the sensor's turret: a drum with its eye; a lamp on the right.
    revolve(b, [(0.0, 0.0), (0.2, 0.0), (0.22, 0.12), (0.2, 0.3), (0.12, 0.36), (0.0, 0.37)], GUN, centre=(4.72, -0.42, 1.42), axis="x", sides=24)
    revolve(b, [(0.0, 0.385), (0.1, 0.375), (0.12, 0.33)], GLASS, centre=(4.72, -0.42, 1.42), axis="x", sides=20)
    revolve(b, [(0.0, 0.0), (0.1, 0.0), (0.11, 0.2), (0.0, 0.24)], GUN, centre=(4.8, 0.4, 1.5), axis="x", sides=16)
    # Aerials on the roof and the nose.
    for x, y, z, h in ((1.9, 0.25, -0.42, 0.4), (-2.7, 0.0, -0.3, 0.5)):
        tube(b, [(x, y, z), (x - h * 0.3, y, z - h)], 0.012, DARK, sides=6)
    return b.object("airframe")


def canopy(parent):
    """The glass: the hull's surface where the windows are, a centimetre off it."""
    b = Builder()
    inside = (3.4, 0.0, 1.2)
    for side in (1, -1):
        rows = []
        for i in range(33):
            x = SCREEN[0] + (SCREEN[1] - SCREEN[0]) * i / 32
            a0, a1 = screen_angles(x, side)
            rows.append([hull_point(x, a0 + (a1 - a0) * k / 24, 0.01) for k in range(25)])
        loft(b, rows, GLASS, closed=False, away_from=inside)
        a0, a1 = chin_angles(side)
        rows = [[hull_point(CHIN_X[0] + (CHIN_X[1] - CHIN_X[0]) * i / 16, a0 + (a1 - a0) * k / 12, 0.01) for k in range(13)] for i in range(17)]
        loft(b, rows, GLASS, closed=False, away_from=inside)
    return b.object("canopy", parent=parent)


def elevator(parent):
    b = Builder()
    plane = aerofoil(0.11, 12)
    rear = [(s, t) for s, t in plane if s >= 0.5]
    rows = []
    for i in range(13):
        y = -1.7 + 3.4 * i / 12
        chord = 1.05 - 0.2 * abs(y) / 1.7
        rows.append([(-4.6 - 0.1 * abs(y) / 1.7 - s * chord, y, PIVOT_Z + t * chord) for s, t in rear])
    loft(b, rows, UNDER, caps=True)
    # It turns about its leading edge in the middle.
    return b.object("elevator", origin=(-5.125, 0.0, PIVOT_Z), parent=parent)


def nacelle(name, side, parent):
    """A nacelle upright, about its pivot: the ring, the hub's housing, three spokes under the
    rotors (one to the trunnion), two fins on its outer side."""
    b = Builder()
    pivot = (0.0, side * OUT, PIVOT_Z)
    # The ring: a thin shroud, an aerofoil turned round the axis, its nose up.
    profile = [(2.07, 0.25), (2.045, 0.1), (2.04, -0.15), (2.06, -0.33), (2.11, -0.4), (2.17, -0.39), (2.21, -0.3), (2.225, -0.12), (2.2, 0.08), (2.13, 0.24)]
    revolve(b, profile, GREEN, centre=pivot, axis="z", sides=96, closed=True)
    # A dark band inside, where the blades' tips pass.
    revolve(b, [(2.035, -0.32), (2.035, 0.12)], DARK, centre=pivot, axis="z", sides=96)
    # The hub's housing through the middle.
    revolve(b, [(0.0, -0.52), (0.1, -0.5), (0.19, -0.42), (0.22, -0.3), (0.26, 0.1), (0.3, 0.3), (0.2, 0.4), (0.0, 0.42)], GUN, centre=pivot, axis="z", sides=28)
    for k in range(3):
        # One spoke towards the fuselage, the others a third of a turn either side.
        a = (math.pi / 2 if side < 0 else -math.pi / 2) + math.tau * k / 3
        d = (math.cos(a), math.sin(a))
        rows = []
        for i in range(9):
            r = 0.2 + 1.86 * i / 8
            w, h = 0.16 - 0.04 * i / 8, 0.075
            rows.append([(pivot[0] + d[0] * r - d[1] * w * superellipse(math.tau * j / 14, 2.4)[0], pivot[1] + d[1] * r + d[0] * w * superellipse(math.tau * j / 14, 2.4)[0], pivot[2] + 0.24 + h * superellipse(math.tau * j / 14, 2.4)[1]) for j in range(14)])
        loft(b, rows, GREEN, caps=True)
    # Where the trunnion comes in.
    b.box((-0.3, side * (OUT - 2.26), PIVOT_Z - 0.2), (0.3, side * (OUT - 2.0), PIVOT_Z + 0.26), GREEN, round=0.08)
    # The fins: two blades on the ring's outer side, pointing out and back.
    section = aerofoil(0.08, 8)
    for ahead, back in ((0.6, 1.0), (-0.5, 1.3)):
        rows = []
        for i in range(7):
            t = i / 6
            y = side * (OUT + math.sqrt(2.2 ** 2 - ahead ** 2) - 0.03 + 0.85 * t)
            chord = 0.75 * (1 - t) + 0.04
            lead = ahead + 0.3 - back * t
            rows.append([(lead - s * chord, y, PIVOT_Z - 0.08 + tt * chord) for s, tt in section])
        loft(b, rows, STEEL, caps=True)
    return b.object(name, origin=pivot, parent=parent)


def rotor(index, side, hub_z, parent):
    """A rotor at its hub: three blades, turning clockwise seen from above (from x towards y) or
    against it, as multirotor.ron's spins: the upper and lower of a ring against each other, the
    rings mirrored."""
    b = Builder()
    hub = (0.0, side * OUT, PIVOT_Z + hub_z)
    clockwise = index in (0, 3)
    turn = 1 if clockwise else -1
    section = aerofoil(0.12, 10)
    for k in range(3):
        a = math.tau * k / 3 + 0.5 * index
        out, ahead = (math.cos(a), math.sin(a)), (-math.sin(a) * turn, math.cos(a) * turn)
        rows, tips = [], []
        for i in range(15):
            t = i / 14
            r = 0.3 + (ROTOR - 0.3) * t
            chord = 0.32 - 0.14 * t - (0.12 * max(0.0, t - 0.92) / 0.08)
            pitch = math.radians(20 - 14 * t)
            row = []
            for s, tt in section:
                # Along the chord from the leading edge back, the leading edge high (z down: up is −z).
                c, n = (0.3 - s) * chord, tt * chord
                f, u = c * math.cos(pitch) - n * math.sin(pitch), c * math.sin(pitch) + n * math.cos(pitch)
                row.append((hub[0] + out[0] * r + ahead[0] * f, hub[1] + out[1] * r + ahead[1] * f, hub[2] - u))
            (tips if t > 0.86 else rows).append(row)
            if abs(t - 12 / 14) < 1e-6:
                tips.append(row)
        loft(b, rows, BLADE, caps=True)
        loft(b, tips, TIP, caps=True)
    revolve(b, [(0.0, -0.07), (0.3, -0.06), (0.34, 0.0), (0.3, 0.06), (0.0, 0.07)], STEEL, centre=hub, axis="z", sides=24)
    b.object(f"prop_{index + 1}", origin=hub, parent=parent)
    # Its blur: a flat ring, both faces.
    d = Builder()
    for z in (-0.002, 0.002):
        revolve(d, [(0.34, z), (ROTOR, z)], BLUR, centre=hub, axis="z", sides=64, flip=z > 0)
    d.object(f"disc_{index + 1}", origin=hub, parent=parent)


# The cockpit's materials: the game knows them by their names.
LINING = ("vinyl", (0.05, 0.055, 0.052), 0.0, 0.8)
TRIM = ("plastic", (0.018, 0.02, 0.02), 0.0, 0.55)
FLOOR = ("rubber", (0.02, 0.021, 0.022), 0.0, 0.9)
CLOTH = ("fabric", (0.06, 0.065, 0.055), 0.0, 0.95)
HIDE = ("leather", (0.03, 0.03, 0.03), 0.0, 0.6)
METAL = ("metal", (0.25, 0.26, 0.27), 1.0, 0.45)
KEYS = ("plastic_keys", (0.06, 0.065, 0.07), 0.0, 0.5)
SCREENS = ("screen", (1.0, 1.0, 1.0), 0.0, 0.3)
WINDOW = ("glass", (0.75, 0.82, 0.85), 0.0, 0.06)

# The instruments' canvas (cockpit.ron's) and each display's rectangle in it, and where it is on
# the panel: from y to y, from z to z (the pilot sits on the right: y positive).
CANVAS = (1360.0, 400.0)
DISPLAYS = (
    ((0.0, 0.0, 640.0, 400.0), (0.16, 0.68), (0.93, 1.255)),
    ((640.0, 0.0, 320.0, 400.0), (-0.13, 0.13), (0.93, 1.255)),
    ((960.0, 0.0, 400.0, 400.0), (-0.665, -0.34), (0.93, 1.255)),
)
PANEL_X = 3.5


def is_glass(x, a):
    """Whether the hull at x and angle a is a window."""
    a = math.atan2(math.sin(a), math.cos(a))
    off_top = abs(math.atan2(math.sin(a - TOP), math.cos(a - TOP)))
    if SCREEN[0] < x < SCREEN[1] and off_top < math.pi / 2 - belt(x):
        return True
    return CHIN_X[0] < x < CHIN_X[1] and (CHIN[0] < a < CHIN[1] or math.pi - CHIN[1] < a < math.pi - CHIN[0])


def cockpit():
    """The cockpit: the hull's lining from the bulkhead behind the seats to the nose with the
    windows' frames, the floor, the panel under its hood with three displays, a console between
    two seats, and what moves: the stick, the collective lever at the pilot's left, the pedals."""
    b = Builder()
    back, around = 2.0, 96
    xs = [back + (5.06 - back) * (1 - (1 - i / 79) ** 1.4) for i in range(80)]
    rings = [[hull_point(x, math.tau * k / around, -0.03) for k in range(around)] for x in xs]
    inside = (3.4, 0.0, 1.0)

    def lined(i, k):
        x, a = (xs[i] + xs[i + 1]) / 2, math.tau * (k + 0.5) / around
        return None if is_glass(x, a) else LINING

    # Facing in: away from a point outside is not one point for a hull, so each face from its
    # own ring's middle, turned over.
    shell = Builder()
    loft(shell, rings, LINING, colour=lined)
    for face in shell.faces:
        corners = [shell.points[i] for i in face]
        centre = [sum(c[j] for c in corners) / len(corners) for j in range(3)]
        b.face(corners, LINING, towards=(0.0, -centre[1], 1.0 - centre[2]))
    # The bulkhead behind the seats, and the floor.
    ring = rings[0]
    for k in range(around):
        b.face([(back, 0.0, 0.9), ring[k], ring[(k + 1) % around]], LINING, towards=(1.0, 0.0, 0.0))
    b.face([(back, -0.97, 1.9), (4.7, -0.7, 1.9), (4.7, 0.7, 1.9), (back, 0.97, 1.9)], FLOOR, towards=(0.0, 0.0, -1.0))

    # The windows' frames, as outside, a little inside the glass.
    def bar(point, r=0.026):
        tube(b, [point(i / 32) for i in range(33)], r, TRIM, sides=8)

    def along(a, x0, x1, r=0.026):
        bar(lambda t: hull_point(x0 + (x1 - x0) * t, a(x0 + (x1 - x0) * t), -0.012), r)

    def across(x, a0, a1, r=0.026):
        bar(lambda t: hull_point(x, a0 + (a1 - a0) * t, -0.012), r)

    along(lambda x: TOP, SCREEN[0], SCREEN[1], r=0.034)
    for side in (1, -1):
        along(lambda x, s=side: screen_angles(x, s)[1], SCREEN[0], SCREEN[1])
        along(lambda x, s=side: TOP + s * 0.95, 3.25, SCREEN[1], r=0.022)
        for x in (SCREEN[0], 3.25, SCREEN[1]):
            across(x, *screen_angles(x, side))
        a0, a1 = chin_angles(side)
        along(lambda x, a=a0: a, *CHIN_X)
        along(lambda x, a=a1: a, *CHIN_X)
        for x in CHIN_X:
            across(x, a0, a1)

    # The panel across the nose, its hood over it, and the displays, each in its bezel.
    b.box((PANEL_X, -0.84, 0.88), (PANEL_X + 0.1, 0.84, 1.42), TRIM, round=0.02)
    b.box((PANEL_X - 0.24, -0.88, 0.835), (PANEL_X + 0.12, 0.88, 0.88), LINING, round=0.018)
    for cell, (y0, y1), (z0, z1) in DISPLAYS:
        x = PANEL_X - 0.006
        m = 0.016
        for lo, hi in (((y0 - m, z0 - m), (y1 + m, z0)), ((y0 - m, z1), (y1 + m, z1 + m)), ((y0 - m, z0), (y0, z1)), ((y1, z0), (y1 + m, z1))):
            b.box((x - 0.006, lo[0], lo[1]), (PANEL_X, hi[0], hi[1]), LINING)
        corners = [(x, y0, z0), (x, y1, z0), (x, y1, z1), (x, y0, z1)]
        uv = [((cell[0] + u * cell[2]) / CANVAS[0], (cell[1] + v * cell[3]) / CANVAS[1]) for u, v in ((0, 0), (1, 0), (1, 1), (0, 1))]
        b.face(corners, SCREENS, towards=(-1.0, 0.0, 0.0), uv=uv)
    # Keys under the displays, the console between the seats with its levers' gate, the side shelves.
    for cell, (y0, y1), _ in DISPLAYS:
        n = 8
        for k in range(n):
            y = y0 + (y1 - y0) * (k + 0.5) / n
            b.box((PANEL_X - 0.012, y - 0.014, 1.285), (PANEL_X, y + 0.014, 1.305), KEYS, round=0.004)
    b.box((2.45, -0.14, 1.46), (PANEL_X + 0.04, 0.14, 1.9), TRIM, round=0.03)
    b.box((2.6, -0.1, 1.44), (3.3, 0.1, 1.47), LINING, round=0.01)
    for side in (-1, 1):
        b.box((2.2, side * 0.78, 1.4), (3.5, side * 0.97, 1.5), TRIM, round=0.02)

    # The seats: a pan, a back leaning a little, a headrest, on a base.
    for y in (-0.42, 0.42):
        b.box((2.32, y - 0.24, 1.44), (2.84, y + 0.24, 1.56), CLOTH, round=0.05)
        rows = []
        for i in range(9):
            t = i / 8
            z = 1.5 - 0.78 * t
            x = 2.3 - 0.1 * t
            w = 0.23 - 0.04 * t * t
            rows.append([(x + 0.055 * superellipse(math.tau * k / 20, 3.0)[0], y + w * superellipse(math.tau * k / 20, 3.0)[1], z) for k in range(20)])
        loft(b, rows, CLOTH, caps=True)
        b.box((2.13, y - 0.12, 0.47), (2.23, y + 0.12, 0.69), HIDE, round=0.04)
        b.box((2.36, y - 0.18, 1.56), (2.78, y + 0.18, 1.9), TRIM, round=0.02)
    cabin = b.object("cabin")

    # The pilot's stick, on the floor before the right seat: it leans about its foot.
    s = Builder()
    foot = (3.0, 0.42, 1.9)
    tube(s, [foot, (2.97, 0.42, 1.6), (2.93, 0.42, 1.38)], 0.016, METAL, sides=10, smooth_path=True)
    tube(s, [(2.93, 0.42, 1.38), (2.9, 0.42, 1.24)], 0.026, HIDE, sides=12)
    s.object("stick", origin=foot, parent=cabin)
    # The collective lever at the pilot's left, hinged behind: its grip rises with the collective.
    c = Builder()
    hinge = (2.3, 0.15, 1.6)
    tube(c, [hinge, (2.8, 0.15, 1.56)], 0.016, METAL, sides=10)
    tube(c, [(2.8, 0.15, 1.56), (2.96, 0.15, 1.54)], 0.027, HIDE, sides=12)
    c.object("collective", origin=hinge, parent=cabin)
    for name, y in (("pedals_left", 0.3), ("pedals_right", 0.54)):
        d = Builder()
        at = (3.32, y, 1.78)
        d.box((at[0] - 0.012, y - 0.05, at[2] - 0.12), (at[0] + 0.012, y + 0.05, at[2]), METAL, round=0.008)
        tube(d, [at, (at[0] + 0.1, y, at[2] + 0.1)], 0.012, METAL, sides=8)
        d.object(name, origin=at, parent=cabin)

    # The glass, drawn over the world.
    g = Builder()
    for side in (1, -1):
        rows = []
        for i in range(33):
            x = SCREEN[0] + (SCREEN[1] - SCREEN[0]) * i / 32
            a0, a1 = screen_angles(x, side)
            rows.append([hull_point(x, a0 + (a1 - a0) * k / 24, -0.005) for k in range(25)])
        loft(g, rows, WINDOW, closed=False, away_from=inside)
        a0, a1 = chin_angles(side)
        rows = [[hull_point(CHIN_X[0] + (CHIN_X[1] - CHIN_X[0]) * i / 16, a0 + (a1 - a0) * k / 12, -0.005) for k in range(13)] for i in range(17)]
        loft(g, rows, WINDOW, closed=False, away_from=inside)
    g.object("glass", parent=cabin)
    export("assets/aircraft/kestrel/cockpit.glb")


def main():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    frame = airframe()
    canopy(frame)
    elevator(frame)
    for name, side, first in (("nacelle_left", -1, 0), ("nacelle_right", 1, 2)):
        n = nacelle(name, side, frame)
        for level, hub_z in enumerate(HUBS):
            rotor(first + level, side, hub_z, n)
    # The lights, each an empty where it shines from.
    for name, at in (
        ("light_left", (0.0, -(OUT + 2.24), PIVOT_Z - 0.08)),
        ("light_right", (0.0, OUT + 2.24, PIVOT_Z - 0.08)),
        ("light_tail", (-5.25, 0.0, 0.25)),
        ("beacon", (-2.4, 0.0, -0.62)),
        ("landing_light", (4.6, 0.0, 1.95)),
    ):
        e = bpy.data.objects.new(name, None)
        e.location = at
        bpy.context.scene.collection.objects.link(e)
        e.parent = frame
    export("assets/aircraft/kestrel/exterior.glb")
    clear()
    cockpit()


main()

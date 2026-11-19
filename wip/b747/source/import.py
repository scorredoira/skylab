# Makes wip/b747/exterior.glb from source.glb (a model made in a modelling program),
# in Blender: `Blender -b --python wip/b747/source/import.py`.
#
# The model is turned into the game's body axes (x forward, y right, z down, metres from the
# centre of gravity: crates/games/flight/src/models.rs) and its pieces become the nodes
# exterior.ron moves: each leg of the gear (what its spring raises) and the wheels of each axle,
# each with its origin where it turns. The model has the tyres of one axle of one main bogie only:
# they are copied onto the two axles of each of the four. The rest is the airframe.
import bpy, bmesh, os
from mathutils import Matrix, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.path.join(HERE, "source.glb")
OUT = os.path.join(HERE, "..", "exterior.glb")

# The model: nose towards +y, up +z, the right wing towards +x, a little larger than the real
# aircraft (its span 59.93 to the 747's 59.64 m). The body axes: x = y, y = x, z = -z, scaled,
# from the centre of gravity: 31.4 m behind the nose, and 5.40 m above the ground the main tyres
# stand on, legs extended (aircraft.ron's contacts).
S = 59.64 / 59.9273
CG_Y, CG_Z = 3.83, -4.351
M = Matrix(((0, S, 0, -S * CG_Y), (S, 0, 0, 0), (0, 0, -S, CG_Z), (0, 0, 0, 1)))

# The middle of each bogie (x, y), in aircraft.ron's order after the nose leg, and how far its
# two axles are from it.
BOGIES = {"wing_left": (-0.38, -5.22), "body_left": (-3.61, -1.73), "body_right": (-3.61, 1.73), "wing_right": (-0.38, 5.22)}
AXLES = 0.74
# What the gear is made of, and of it what turns with a wheel.
GEAR = ("747_10", "wheel", "material")
TURNS = ("wheel", "material")
# The faces of the gear's material bigger than this, m², are the wells' linings and the doors.
WELL = 0.35
# Where the belly is at the wells, below the centre of gravity.
BELLY = 3.0

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=SOURCE)

def select(objs):
    bpy.ops.object.select_all(action='DESELECT')
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]

# Everything into body axes, transforms applied, one object.
meshes = [o for o in bpy.context.scene.objects if o.type == 'MESH']
for o in meshes:
    o.matrix_world = M @ o.matrix_world
for o in meshes:
    select([o])
    o.parent = None
    o.matrix_world = o.matrix_world.copy()
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
for o in list(bpy.context.scene.objects):
    if o.type != 'MESH':
        bpy.data.objects.remove(o)
select(meshes)
bpy.ops.object.join()
airframe = bpy.context.view_layer.objects.active
airframe.name = airframe.data.name = "airframe"

def bounds(o):
    vs = [o.matrix_world @ v.co for v in o.data.vertices]
    return Vector((min(v.x for v in vs), min(v.y for v in vs), min(v.z for v in vs))), Vector((max(v.x for v in vs), max(v.y for v in vs), max(v.z for v in vs)))

def cut(source, name, wanted, well=False):
    """The faces of `source` that `wanted(material name, centre)` picks, as a new object; with
    `well`, not the big ones nor those above the belly: the linings of the wells and the doors,
    which stay with the airframe, and what of a leg is inside its well."""
    select([source])
    bpy.ops.object.mode_set(mode='EDIT')
    bm = bmesh.from_edit_mesh(source.data)
    names = [slot.material.name for slot in source.material_slots]
    picked = 0
    for f in bm.faces:
        centre = f.calc_center_median()
        f.select = wanted(names[f.material_index], centre) and not (well and (f.calc_area() > WELL or centre.z < BELLY))
        picked += f.select
    bmesh.update_edit_mesh(source.data)
    if picked == 0:
        bpy.ops.object.mode_set(mode='OBJECT')
        raise SystemExit(f"{name}: nothing to cut")
    bpy.ops.mesh.separate(type='SELECTED')
    bpy.ops.object.mode_set(mode='OBJECT')
    new = [o for o in bpy.context.selected_objects if o != source][0]
    new.name = new.data.name = name
    print(name, picked, "faces", [tuple(round(c, 2) for c in b) for b in bounds(new)])
    return new

def origin(o, at):
    select([o])
    bpy.context.scene.cursor.location = at
    bpy.ops.object.origin_set(type='ORIGIN_CURSOR')

def copy(o, name, by):
    new = o.copy()
    new.data = o.data.copy()
    new.name = new.data.name = name
    bpy.context.scene.collection.objects.link(new)
    new.matrix_world = Matrix.Translation(by) @ new.matrix_world
    select([new])
    bpy.ops.object.transform_apply(location=True, rotation=False, scale=False)
    return new

def middle(o):
    lo, hi = bounds(o)
    return (lo + hi) / 2

# The nose leg: its two tyres on one axle, and the rest, which the pedals steer about the leg.
nose = lambda c: 21.5 < c.x < 27.0 and abs(c.y) < 2.0
wheel_nose = cut(airframe, "wheel_nose", lambda m, c: m in TURNS and nose(c))
leg_nose = cut(airframe, "leg_nose", lambda m, c: m in GEAR and nose(c), well=True)
axle = middle(wheel_nose)
origin(wheel_nose, Vector((axle.x, 0.0, axle.z)))
origin(leg_nose, Vector((axle.x, 0.0, bounds(leg_nose)[0].z)))
print("nose axle", tuple(round(c, 3) for c in axle), "tyre bottom", round(bounds(wheel_nose)[1].z, 3))

# The model's main tyres: one axle's pair, at the rear axle of the left body gear.
pair = cut(airframe, "pair", lambda m, c: m in TURNS and -7.0 < c.x < 3.0)
at = middle(pair)
print("main pair at", tuple(round(c, 3) for c in at), "tyre bottom", round(bounds(pair)[1].z, 3))
main = lambda c: -7.5 < c.x < 3.5 and abs(c.y) < 8.0
for name, (x, y) in BOGIES.items():
    side = (lambda c: c.y < 0) if y < 0 else (lambda c: c.y >= 0)
    place = (lambda c: abs(c.y) > 3.5) if abs(y) > 3.5 else (lambda c: abs(c.y) <= 3.5)
    # The bogie and the strut over it; the doors ahead of it stay.
    near = lambda c: abs(c.x - x) < 1.55
    leg = cut(airframe, f"leg_{name}", lambda m, c: m == "747_10" and main(c) and near(c) and side(c) and place(c), well=True)
    origin(leg, Vector((x, y, bounds(leg)[0].z)))
    for axle_name, dx in (("front", AXLES), ("rear", -AXLES)):
        wheel = copy(pair, f"wheel_{name}_{axle_name}", Vector((x + dx - at.x, y - at.y, 0.0)))
        origin(wheel, Vector((x + dx, y, at.z)))
bpy.data.objects.remove(pair)

# The wells. The model has its gear out: the doors hang open and the skin has a hole at each
# well. What is seen only with the gear out (the doors, the wells' linings: `gear_doors`) is cut
# from the airframe, and each hole is closed with a patch of skin (`doors_closed`), seen only
# with the gear up.
LININGS = ("747_10",)
# The hole of the nose well in the belly, which is round across there, and those of the two body
# wells, where it is flat: from x to x, from y to y.
SQUARE_HOLES = ((23.27, 24.87, -0.65, 0.69),)
FLAT_HOLES = ((-4.02, -1.92, -2.25, -0.33), (-4.02, -1.92, 0.37, 2.29))
def well(c):
    """Whether a point is at a well, and how far below the centre of gravity the skin is there."""
    if 22.8 < c.x < 25.4 and abs(c.y) < 1.1:
        return 2.95
    if -4.6 < c.x < -1.3 and 0.2 < abs(c.y) < 2.8:
        return 3.25
    if -2.2 < c.x < 1.4 and 2.8 <= abs(c.y) < 6.8:
        return 2.3 if abs(c.y) > 5.0 else 3.3
    return None

def hangs(m, c):
    skin = well(c)
    if m in LININGS:
        return nose(c) or main(c)
    return m not in GEAR and skin is not None and c.z > skin

gear_doors = cut(airframe, "gear_doors", hangs)

def close_holes(source, name):
    """A patch over each hole the skin of `source` has at a well: the loops of edges that only
    one face has there, each filled from its middle, with the skin's material and its picture
    carried in from the hole's edge."""
    me = source.data
    bm = bmesh.new()
    bm.from_mesh(me)
    uv = bm.loops.layers.uv.active
    key = lambda co: (round(co.x, 3), round(co.y, 3), round(co.z, 3))
    count, place, owner, paint = {}, {}, {}, {}
    for f in bm.faces:
        for loop in f.loops:
            a, b = key(loop.vert.co), key(loop.link_loop_next.vert.co)
            edge = (min(a, b), max(a, b))
            count[edge] = count.get(edge, 0) + 1
            place[a] = loop.vert.co.copy()
            owner[edge] = (f.material_index, {a: loop[uv].uv.copy(), b: loop.link_loop_next[uv].uv.copy()})
    at_well = lambda k: well(place[k]) is not None and well(place[k]) - 1.3 < place[k].z < well(place[k]) + 0.05
    links = {}
    for (a, b), n in count.items():
        if n == 1 and at_well(a) and at_well(b):
            links.setdefault(a, []).append(b)
            links.setdefault(b, []).append(a)
            # A point of a hole's edge is painted as the face of the skin that ends there.
            material, at = owner[(a, b)]
            paint.setdefault(a, (material, at[a]))
            paint.setdefault(b, (material, at[b]))
    out = bmesh.new()
    out_uv = out.loops.layers.uv.new("UVMap")
    slots, seen, patches = {}, set(), 0

    def triangle(a, b, c):
        # Each a point of the skin (by its key), or a place and where it is in the picture.
        corners = [(place[k], paint[k][1]) if len(k) == 3 else k for k in (a, b, c)]
        # Facing out of the aircraft: down.
        if (corners[1][0] - corners[0][0]).cross(corners[2][0] - corners[0][0]).z < 0:
            corners = [corners[0], corners[2], corners[1]]
        face = out.faces.new([out.verts.new(co) for co, _ in corners])
        face.material_index = slots.setdefault(paint[b][0], len(slots))
        for loop, (_, t) in zip(face.loops, corners):
            loop[out_uv].uv = t

    # The square holes under the fuselage: a strip from the edge behind to the edge ahead, which
    # follows the belly's curve across.
    for x0, x1, y0, y1 in SQUARE_HOLES:
        edge = [k for k in links if x0 - 0.04 < place[k].x < x1 + 0.04 and y0 - 0.04 < place[k].y < y1 + 0.04]
        seen.update(edge)
        behind = sorted((k for k in edge if place[k].x < x0 + 0.06), key=lambda k: place[k].y)
        ahead = sorted((k for k in edge if place[k].x > x1 - 0.06), key=lambda k: place[k].y)
        if len(behind) < 2 or len(ahead) < 2:
            raise SystemExit(f"no hole at {(x0, x1, y0, y1)}: {len(behind)} and {len(ahead)} points")
        # The two sides of the skin's picture meet under the nose, each drawn its own way: the
        # patch is all of the paint at the middle of the edge behind it.
        plain = paint[behind[len(behind) // 2]]
        for k in behind + ahead:
            paint[k] = plain
        i = j = 0
        while i < len(behind) - 1 or j < len(ahead) - 1:
            # The next point across, on the side that is behind in y.
            if j == len(ahead) - 1 or (i < len(behind) - 1 and place[behind[i + 1]].y <= place[ahead[j + 1]].y):
                triangle(behind[i], behind[i + 1], ahead[j])
                i += 1
            else:
                triangle(behind[i], ahead[j + 1], ahead[j])
                j += 1
        patches += 1
        print("  square hole", (x0, x1, y0, y1), len(behind), "and", len(ahead), "points")
    # The flat holes of the body wells: what their edges' points go round, from its middle.
    for x0, x1, y0, y1 in FLAT_HOLES:
        edge = sorted((k for k in links if x0 - 0.04 < place[k].x < x1 + 0.04 and y0 - 0.04 < place[k].y < y1 + 0.04), key=lambda k: (k[0], k[1]))
        seen.update(edge)
        turn = lambda o, a, b: (place[a].x - place[o].x) * (place[b].y - place[o].y) - (place[a].y - place[o].y) * (place[b].x - place[o].x)
        lower, upper = [], []
        for k in edge:
            while len(lower) >= 2 and turn(lower[-2], lower[-1], k) <= 0:
                lower.pop()
            lower.append(k)
        for k in reversed(edge):
            while len(upper) >= 2 and turn(upper[-2], upper[-1], k) <= 0:
                upper.pop()
            upper.append(k)
        ring = lower[:-1] + upper[:-1]
        if len(ring) < 3:
            raise SystemExit(f"no hole at {(x0, x1, y0, y1)}")
        centre = sum((place[k] for k in ring), Vector()) / len(ring)
        centre_uv = sum((paint[k][1] for k in ring), Vector((0.0, 0.0))) / len(ring)
        for i in range(len(ring)):
            triangle((centre, centre_uv), ring[i], ring[(i + 1) % len(ring)])
        patches += 1
        print("  flat hole", (x0, x1, y0, y1), len(edge), "points,", len(ring), "round it")
    for start in links:
        if start in seen or len(links[start]) != 2:
            continue
        ring, before, here = [start], None, start
        while True:
            seen.add(here)
            ahead = [k for k in links[here] if k != before]
            if len(links[here]) != 2 or not ahead:
                ring = None
                break
            before, here = here, ahead[0]
            if here == start:
                break
            if here in seen:
                ring = None
                break
            ring.append(here)
        if ring is None or len(ring) < 3:
            continue
        points = [place[k] for k in ring]
        centre = sum(points, Vector()) / len(points)
        # A hole worth a door: not a sliver between two parts of the model.
        if max((p - centre).length for p in points) < 0.25:
            continue
        centre_uv = sum((paint[k][1] for k in ring), Vector((0.0, 0.0))) / len(ring)
        for i in range(len(ring)):
            triangle((centre, centre_uv), ring[i], ring[(i + 1) % len(ring)])
        patches += 1
        print("  hole of", len(ring), "edges about", tuple(round(c, 2) for c in centre))
    bm.free()
    mesh = bpy.data.meshes.new(name)
    out.to_mesh(mesh)
    out.free()
    for slot, _ in sorted(slots.items(), key=lambda item: item[1]):
        mesh.materials.append(me.materials[slot])
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    print(name, patches, "patches,", len(mesh.polygons), "faces")
    return obj

doors_closed = close_holes(airframe, "doors_closed")

origin(airframe, Vector((0, 0, 0)))

# The lights, at where the model has its lamps.
def empty(name, at):
    e = bpy.data.objects.new(name, None)
    e.location = at
    bpy.context.scene.collection.objects.link(e)

lo, hi = bounds(airframe)
print("airframe", tuple(round(c, 2) for c in lo), tuple(round(c, 2) for c in hi))
empty("light_left", (-13.25, -29.7, -1.15))
empty("light_right", (-13.25, 29.7, -1.15))
empty("light_tail", (lo.x + 0.1, 0.0, -3.0))
empty("beacon", (8.0, 0.0, -4.95))
empty("beacon_belly", (-2.0, 0.0, 3.2))
empty("landing_light_left", (9.3, -4.3, 1.6))
empty("landing_light_right", (9.3, 4.3, 1.6))

bpy.ops.export_scene.gltf(filepath=OUT, export_format='GLB', export_yup=False, export_apply=True, use_selection=False, export_texcoords=True, export_normals=True, export_materials='EXPORT', export_image_format='JPEG', export_jpeg_quality=92)
print("wrote", OUT)

# Makes assets/aircraft/c172/exterior.glb from source.glb (a model made in a modelling program),
# in Blender: `Blender -b --python assets/aircraft/c172/source/import.py`.
#
# The model is turned into the game's body axes (x forward, y right, z down, metres from the
# centre of gravity: crates/games/flight/src/models.rs) and scaled to the real span; its pieces
# become the nodes exterior.ron moves (surfaces about their hinges, the gear's legs, forks and
# wheels, the propeller), each with its origin where it turns. The wheels are taken out of their
# fairings; the cowling is cut from the fuselage at the firewall. The game draws the textures per
# pixel; the end of this file makes them what it draws: the paint cleaned, a normal map, the
# livery painted on.
import bpy, bmesh, math, os, sys
from mathutils import Matrix, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.path.join(HERE, "source.glb")
OUT = os.path.join(HERE, "..", "exterior.glb")

# The model's metres to the real aircraft's (its span 12.6 to the C172's 11.0 m).
S = 0.874
# The model: nose towards -y, up +z, the left wing towards +x. The body axes: x = -y, y = -x,
# z = -z, scaled, and moved so that the main wheels touch the ground where aircraft.ron has them
# (x -0.28, z 1.20 under the centre of gravity).
TX, TZ = -0.092, 1.194
M = Matrix(((0, -S, 0, TX), (-S, 0, 0, 0), (0, 0, -S, TZ), (0, 0, 0, 1)))

def body(p):
    return M @ Vector(p)

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=SOURCE)

def find(part):
    return [o for o in bpy.context.scene.objects if o.type == 'MESH' and part in o.name]

# What of the inside is seen through the windows; the rest (the switches' labels, belts, buttons) goes.
keep_inside = ["Interior_Body", "Seats_MAT", "SeatGear", "Steering_MAT", "Paddle", "Meter_01", "Static_Base"]
for o in list(bpy.context.scene.objects):
    if o.type == 'MESH' and 'Exterior' not in o.name and not any(k in o.name for k in keep_inside):
        bpy.data.objects.remove(o)

# Everything into body axes, transforms applied.
for o in bpy.context.scene.objects:
    if o.type == 'MESH':
        o.matrix_world = M @ o.matrix_world
for o in list(bpy.context.scene.objects):
    if o.type != 'MESH':
        continue
    bpy.ops.object.select_all(action='DESELECT')
    o.select_set(True)
    bpy.context.view_layer.objects.active = o
    o.parent = None
    o.matrix_world = o.matrix_world.copy()
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
for o in list(bpy.context.scene.objects):
    if o.type != 'MESH':
        bpy.data.objects.remove(o)

# The inside, seen only through the windows, made lighter.
for o in list(bpy.context.scene.objects):
    if o.type == 'MESH' and 'Exterior' not in o.name and len(o.data.polygons) > 2000:
        bpy.ops.object.select_all(action='DESELECT')
        o.select_set(True)
        bpy.context.view_layer.objects.active = o
        mod = o.modifiers.new("lighter", 'DECIMATE')
        mod.ratio = 2000 / len(o.data.polygons)
        bpy.ops.object.modifier_apply(modifier="lighter")

def bounds(objs):
    vs = [o.matrix_world @ v.co for o in objs for v in o.data.vertices]
    lo = Vector((min(v.x for v in vs), min(v.y for v in vs), min(v.z for v in vs)))
    hi = Vector((max(v.x for v in vs), max(v.y for v in vs), max(v.z for v in vs)))
    return lo, hi

def loose(objs):
    out = []
    for o in objs:
        bpy.ops.object.select_all(action='DESELECT')
        o.select_set(True)
        bpy.context.view_layer.objects.active = o
        bpy.ops.object.mode_set(mode='EDIT')
        bpy.ops.mesh.select_all(action='SELECT')
        bpy.ops.mesh.separate(type='LOOSE')
        bpy.ops.object.mode_set(mode='OBJECT')
        out += list(bpy.context.selected_objects)
    return out

def piece(name):
    return find(f"Body_MAT_0.{name}_")

# The wheels: in each wheel's fairing, the tyre and the two halves of its hub (all round the axle).
def wheel(objs):
    parts = loose(objs)
    tyre = max(parts, key=lambda p: (lambda b: (b[1] - b[0]).z)(bounds([p])))
    lo, hi = bounds([tyre])
    centre = (lo + hi) / 2
    radius = (hi.z - lo.z) / 2
    turning = [p for p in parts if (lambda b: abs(((b[0] + b[1]) / 2 - centre).x) < radius * 0.3 and abs(((b[0] + b[1]) / 2 - centre).z) < radius * 0.3 and (b[1] - b[0]).z > radius * 1.6)(bounds([p]))]
    rest = [p for p in parts if p not in turning]
    return turning, rest, centre, radius

nose_wheel, nose_rest, nose_hub, nose_r = wheel(piece("021"))
left_wheel, left_rest, left_hub, main_r = wheel(piece("022"))
right_wheel, right_rest, right_hub, _ = wheel(piece("023"))
print("hubs", nose_hub, left_hub, right_hub, "radii", nose_r, main_r)

# Each wheel moved the few millimetres to where aircraft.ron's contact point is, its leg with it.
CONTACTS = {"nose": (1.37, 0.0), "left": (-0.28, -1.265), "right": (-0.28, 1.265)}
def snap(objs, hub, at):
    d = Vector((at[0] - hub.x, at[1] - hub.y, 0.0))
    for o in objs:
        o.matrix_world = Matrix.Translation(d) @ o.matrix_world
    return hub + d
nose_hub = snap(nose_wheel + nose_rest + piece("019"), nose_hub, CONTACTS["nose"])
left_hub = snap(left_wheel + left_rest, left_hub, CONTACTS["left"])
right_hub = snap(right_wheel + right_rest, right_hub, CONTACTS["right"])

# The main legs: one piece for both, cut by side.
legs = loose(piece("020"))
left_leg = [p for p in legs if (lambda b: b[0].y + b[1].y)(bounds([p])) < 0]
right_leg = [p for p in legs if p not in left_leg]

# The cowling: the fuselage forward of the windscreen's foot.
fuselage = find("Cessna_Exterior_Body_MAT_0_Body")[0]
bpy.ops.object.select_all(action='DESELECT')
fuselage.select_set(True)
bpy.context.view_layer.objects.active = fuselage
bpy.ops.object.mode_set(mode='EDIT')
bm = bmesh.from_edit_mesh(fuselage.data)
firewall = body((0, -1.0, 0)).x
for f in bm.faces:
    f.select = all(v.co.x > firewall for v in f.verts)
bmesh.update_edit_mesh(fuselage.data)
bpy.ops.mesh.separate(type='SELECTED')
bpy.ops.object.mode_set(mode='OBJECT')
cowling = [o for o in bpy.context.selected_objects if o != fuselage]

def join(objs, name, origin):
    objs = [o for o in objs if o.name in bpy.data.objects]
    bpy.ops.object.select_all(action='DESELECT')
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    if len(objs) > 1:
        bpy.ops.object.join()
    o = bpy.context.view_layer.objects.active
    o.name = name
    o.data.name = name
    bpy.context.scene.cursor.location = origin
    bpy.ops.object.origin_set(type='ORIGIN_CURSOR')
    return o

def hinge(objs):
    # The leading edge's middle: the hinge of a control surface.
    lo, hi = bounds(objs)
    front = [o.matrix_world @ v.co for o in objs for v in o.data.vertices if (o.matrix_world @ v.co).x > hi.x - 0.03]
    return sum(front, Vector()) / len(front)

def empty(name, at):
    e = bpy.data.objects.new(name, None)
    e.location = at
    bpy.context.scene.collection.objects.link(e)

surfaces = {
    "flap_left": piece("016"), "flap_right": piece("017"),
    "aileron_left": piece("014"), "aileron_right": piece("015"),
    "elevator": piece("012") + piece("013"), "rudder": piece("008"),
}
for name, objs in surfaces.items():
    join(objs, name, hinge(objs))

# The propeller: its blades to the McCauley's 1.905 m, about the hub.
prop = piece("001") + piece("024")
# The model's blades lean some 5 degrees out of the plane square to the shaft, and the spinner
# sits a few millimetres off it: turned about the shaft they would wobble. The blades are turned
# about the hub until both tips are at the same x, and the spinner with them, then centred on it.
def points(objs):
    return [o.matrix_world @ v.co for o in objs for v in o.data.vertices]
def turn(objs, matrix):
    for o in objs:
        o.matrix_world = matrix @ o.matrix_world
lo, hi = bounds(piece("001"))
prop_centre = Vector(((lo.x + hi.x) / 2, 0.0, (lo.z + hi.z) / 2))
def tips():
    ps = points(piece("001"))
    return max(ps, key=lambda p: -p.z), max(ps, key=lambda p: p.z)
top, bottom = tips()
lean = math.atan2(top.x - bottom.x, bottom.z - top.z)
about = lambda m: Matrix.Translation(prop_centre) @ m @ Matrix.Translation(-prop_centre)
best = min((about(Matrix.Rotation(a, 4, 'Y')) for a in (lean, -lean)), key=lambda m: abs((m @ top).x - (m @ bottom).x))
turn(prop, best)
top, bottom = tips()
print("blade tips after squaring: x", round(top.x, 4), round(bottom.x, 4))
lo, hi = bounds(piece("001"))
slo, shi = bounds(piece("024"))
spinner_centre = (slo + shi) / 2
turn(piece("024"), Matrix.Translation(Vector((0.0, prop_centre.y - spinner_centre.y, prop_centre.z - spinner_centre.z))))
print("spinner centre", tuple(round(c, 4) for c in spinner_centre), "hub", tuple(round(c, 4) for c in prop_centre), "spinner objects", len(piece("024")))
blade = 1.905 / 2
k = blade / ((hi.z - lo.z) / 2)
for o in piece("001"):
    o.matrix_world = Matrix.Translation(prop_centre) @ Matrix.Scale(k, 4) @ Matrix.Translation(-prop_centre) @ o.matrix_world
# The spinner is polished metal: a material of its own.
spinner_mat = None
for o in piece("024"):
    for slot in o.material_slots:
        if slot.material and slot.material.name == "Body_MAT":
            if spinner_mat is None:
                spinner_mat = slot.material.copy()
                spinner_mat.name = "Spinner_MAT"
            slot.material = spinner_mat
join(prop, "propeller", prop_centre)
print("propeller hub", prop_centre)
bpy.ops.mesh.primitive_cylinder_add(vertices=64, radius=blade, depth=0.01, location=prop_centre, rotation=(0, 1.5708, 0))
disc = bpy.context.active_object
mat = bpy.data.materials.new("propeller_disc")
mat.use_nodes = True
mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.05, 0.05, 0.05, 1)
disc.data.materials.append(mat)
join([disc], "propeller_disc", prop_centre)

# The gear. The struts' origin is where they leave the fuselage; the forks' at the contact; the
# wheels' at their hubs.
ground = lambda hub, r: Vector((hub.x, hub.y, hub.z + r))
join(piece("019"), "strut_nose", bounds(piece("019"))[0] * 0.5 + bounds(piece("019"))[1] * 0.5)
join(nose_rest, "fork_nose", ground(nose_hub, nose_r))
join(nose_wheel, "wheel_nose", nose_hub)
for side, leg, rest, whl, hub in (("left", left_leg, left_rest, left_wheel, left_hub), ("right", right_leg, right_rest, right_wheel, right_hub)):
    lo, hi = bounds(leg)
    root = Vector(((lo.x + hi.x) / 2, lo.y if side == "right" else hi.y, lo.z))
    join(leg, f"strut_{side}", root)
    join(rest, f"fork_{side}", ground(hub, main_r))
    join(whl, f"wheel_{side}", hub)
    print(side, "strut root", root, "foot", hub - root)

join(cowling, "cowling", Vector((0, 0, 0)))
# The lights at the wing tips' lamps (the left wing's is red), before they join the airframe.
for name, part in (("light_left", "025"), ("light_right", "026")):
    lo, hi = bounds(piece(part))
    empty(name, (lo + hi) / 2)
airframe = piece("002") + piece("004") + piece("003") + piece("005") + piece("010") + piece("011") + piece("025") + piece("026") + piece("009")
join(airframe, "airframe", Vector((0, 0, 0)))
rest = [o for o in bpy.context.scene.objects if o.type == 'MESH' and o.name not in (
    "flap_left", "flap_right", "aileron_left", "aileron_right", "elevator", "rudder", "propeller", "propeller_disc",
    "strut_nose", "fork_nose", "wheel_nose", "strut_left", "fork_left", "wheel_left", "strut_right", "fork_right", "wheel_right",
    "cowling", "airframe")]
join(rest, "fuselage", Vector((0, 0, 0)))

empty("beacon", body((0, 5.965, 2.96)))
empty("light_tail", body((0, 6.62, 1.25)))
empty("landing_light", body((-0.2, -2.25, 0.95)))

for o in bpy.context.scene.objects:
    if o.type == 'MESH':
        print(f"{o.name}: {len(o.data.polygons)} faces, origin {tuple(round(c, 3) for c in o.location)}")
    else:
        print(f"{o.name}: at {tuple(round(c, 3) for c in o.location)}")

# The materials as the game draws them, per pixel (`world_render`'s textured objects): the paint
# is not metal (the model's says it is, everywhere); the spinner is polished metal; the windows a
# light tint; the inside, seen only through them, at a quarter of its pictures' size.
for m in bpy.data.materials:
    if m.use_nodes and "Principled BSDF" in m.node_tree.nodes:
        bsdf = m.node_tree.nodes["Principled BSDF"]
        metal = bsdf.inputs["Metallic"]
        for link in list(metal.links):
            m.node_tree.links.remove(link)
        metal.default_value = 0.0
        if m.name == "Spinner_MAT":
            metal.default_value = 1.0
            for link in list(bsdf.inputs["Roughness"].links):
                m.node_tree.links.remove(link)
            bsdf.inputs["Roughness"].default_value = 0.14
        if m.name == "Glass_MAT":
            bsdf.inputs["Base Color"].default_value = (0.020, 0.028, 0.030, 1.0)
            bsdf.inputs["Alpha"].default_value = 0.30
            bsdf.inputs["Roughness"].default_value = 0.05

def images_of(m):
    return [n.image for n in m.node_tree.nodes if n.type == 'TEX_IMAGE' and n.image] if m.use_nodes else []

outside = {i.name for m in bpy.data.materials if m.name in ("Body_MAT", "Spinner_MAT") for i in images_of(m)}
for m in bpy.data.materials:
    for image in images_of(m):
        if image.name not in outside and max(image.size) > 512:
            image.scale(512, 512)

import numpy as np

def pixels(image):
    w, h = image.size
    a = np.empty(w * h * 4, np.float32)
    image.pixels.foreach_get(a)
    return a.reshape(h, w, 4)

def box(a, r):
    # A box blur of side 2r + 1, both ways, repeating at the edges.
    for axis in (0, 1):
        c = np.cumsum(np.pad(a, [(r + 1, r) if k == axis else (0, 0) for k in range(a.ndim)], mode='wrap'), axis=axis)
        a = (np.take(c, range(2 * r + 1, c.shape[axis]), axis=axis) - np.take(c, range(0, c.shape[axis] - 2 * r - 1), axis=axis)) / (2 * r + 1)
    return a

def srgb_to_linear(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)

def linear_to_srgb(c):
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055)

body_mat = bpy.data.materials["Body_MAT"]
spinner_mat = bpy.data.materials.get("Spinner_MAT")
base_node = body_mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].links[0].from_node
base = pixels(base_node.image)
H, W = base.shape[:2]
def band(d, half, aa):
    return np.clip((half - np.abs(d)) / aa + 0.5, 0.0, 1.0)

smooth = lambda a, b, x: np.clip((x - a) / (b - a), 0.0, 1.0) ** 2 * (3 - 2 * np.clip((x - a) / (b - a), 0.0, 1.0))

# Where on the aircraft each texel of the body's picture is: its triangles rasterised in the
# picture, a texel taking the triangle it is inside (or, within a texel and a half of its edge, the
# nearest), never a far one's guess. Body axes; the fuselage, cowling, tail and the wheels'
# fairings, not the wing.
def triangles(objs):
    # The triangles of the body's material: corners' picture coordinates, places and normals.
    uvs, ps, ns = [], [], []
    for o in objs:
        me = o.data
        slots = [i for i, m in enumerate(me.materials) if m and m.name == "Body_MAT"]
        if not slots:
            continue
        me.calc_loop_triangles()
        t = len(me.loop_triangles)
        loops = np.empty(t * 3, np.int64); me.loop_triangles.foreach_get("loops", loops)
        mats = np.empty(t, np.int64); me.loop_triangles.foreach_get("material_index", mats)
        uv = np.empty(len(me.loops) * 2, np.float32); me.uv_layers[0].data.foreach_get("uv", uv)
        vi = np.empty(len(me.loops), np.int64); me.loops.foreach_get("vertex_index", vi)
        co = np.empty(len(me.vertices) * 3, np.float32); me.vertices.foreach_get("co", co)
        no = np.empty(len(me.vertices) * 3, np.float32); me.vertices.foreach_get("normal", no)
        mw = np.array(o.matrix_world)
        co = co.reshape(-1, 3) @ mw[:3, :3].T + mw[:3, 3]
        no = no.reshape(-1, 3) @ np.linalg.inv(mw[:3, :3])
        keep = np.isin(mats, slots)
        loops = loops.reshape(-1, 3)[keep]
        uvs.append(uv.reshape(-1, 2)[loops]); ps.append(co[vi[loops]]); ns.append(no[vi[loops]])
    return np.concatenate(uvs), np.concatenate(ps), np.concatenate(ns)

uv, pos, nor = triangles([o for o in bpy.context.scene.objects if o.type == 'MESH'])
near = (pos[:, :, 0].max(1) > -6.4) & ((np.abs(pos[:, :, 1]).min(1) < 0.8) | (pos[:, :, 2].max(1) > 0.7)) & (pos[:, :, 2].min(1) < 1.3)
uv, pos, nor = uv[near], pos[near], nor[near]
at = np.zeros((H, W, 3), np.float32)
facing = np.zeros((H, W, 3), np.float32)
nearest = np.full((H, W), 99.0, np.float32)
# How stretched the picture is on each texel: the longest over the shortest of the lengths its
# sides cover on the aircraft.
stretch = np.ones((H, W), np.float32)
for tuv, tp, tn in zip(uv, pos, nor):
    q = tuv * np.array([W, H]) - 0.5
    try:
        jac = np.stack([tp[1] - tp[0], tp[2] - tp[0]], 1) @ np.linalg.inv(np.stack([q[1] - q[0], q[2] - q[0]], 1))
        sv = np.linalg.svd(jac, compute_uv=False)
        ratio = sv[0] / max(sv[1], 1e-9)
    except np.linalg.LinAlgError:
        ratio = 1.0
    x0, y0 = np.floor(q.min(0) - 2).astype(int)
    x1, y1 = np.ceil(q.max(0) + 2).astype(int)
    if x1 - x0 > 600 or y1 - y0 > 600:
        continue
    xs, ys = np.meshgrid(np.arange(x0, x1 + 1), np.arange(y0, y1 + 1))
    pts = np.stack([xs.ravel(), ys.ravel()], -1).astype(np.float64)
    a, b, c = q
    area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    if abs(area) < 1e-9:
        continue
    def edge(p0, p1):
        e = (p1[0] - p0[0]) * (pts[:, 1] - p0[1]) - (p1[1] - p0[1]) * (pts[:, 0] - p0[0])
        return e / area, e / np.hypot(*(p1 - p0)) * np.sign(area)
    w0, d0 = edge(b, c); w1, d1 = edge(c, a); w2, d2 = edge(a, b)
    inside = (d0 > -1.5) & (d1 > -1.5) & (d2 > -1.5)
    if not inside.any():
        continue
    bary = np.clip(np.stack([w0, w1, w2], -1)[inside], 0.0, None)
    bary /= bary.sum(1, keepdims=True)
    px, py = pts[inside, 0].astype(int) % W, pts[inside, 1].astype(int) % H
    off = np.maximum(0.0, -np.minimum(np.minimum(d0, d1), d2)[inside])
    better = off < nearest[py, px]
    nearest[py[better], px[better]] = off[better]
    at[py[better], px[better]] = (bary @ tp)[better]
    facing[py[better], px[better]] = (bary @ tn)[better]
    stretch[py[better], px[better]] = ratio
placed = nearest < 99.0
# The cowling: forward of the firewall.
cowl = placed & (at[..., 0] > body((0, -1.0, 0)).x)


# The white paint made clean: the picture's soft smears and blotches (left by how it was painted
# on the model) flattened, its fine print kept: panel lines, rivets, seams, hatches, and the
# stains that are clearly darker (the exhaust's). The cowling is made smooth (its picture's rows
# of rivets come out as dashes, stretched across it) and its seams drawn again, straight: the
# split of its upper and lower halves along the side with its fasteners, its joint at the
# firewall. What is not white paint (tyres, propeller, lenses) is left as it is.
lum = base[..., :3] @ np.array([0.2126, 0.7152, 0.0722], np.float32)
low = box(lum, 16)
paint_white = np.median(lum[low > 0.6])
deviation = low - paint_white
clean_low = paint_white + deviation * smooth(0.05, 0.14, -deviation)
fine = lum - low
# The fine print is thin and sharp: what stands out from a few texels round it. A smear is wide.
sharp = np.abs(lum - box(lum, 2))
keep = np.maximum(smooth(0.03, 0.07, sharp), smooth(0.05, 0.10, np.abs(fine)) * smooth(0.012, 0.03, sharp))
keep = np.where(cowl, 0.0, keep)
# The panels behind it, under the windscreen, have its rows of rivets drawn as dashes: fainter.
keep = np.where(placed & ~cowl & (at[..., 0] > 0.2), keep * 0.4, keep)
# Where the picture is stretched the rivets come out as dashes: only the lines stay there.
keep *= np.where(placed, smooth(2.4, 1.6, stretch) + (1 - smooth(2.4, 1.6, stretch)) * smooth(0.12, 0.2, sharp), 1.0)
keep = np.maximum(keep, np.maximum(np.roll(keep, 1, 0), np.roll(keep, -1, 0)))
keep = np.maximum(keep, np.maximum(np.roll(keep, 1, 1), np.roll(keep, -1, 1)))
clean = clean_low + fine * keep
COWL_SPLIT, FIREWALL, FASTENERS = -0.06, body((0, -1.0, 0)).x + 0.02, 0.11
cx, cz = at[..., 0], at[..., 2]
side_on = smooth(0.3, 0.5, np.abs(facing[..., 1]) / np.maximum(np.linalg.norm(facing, axis=-1), 1e-6))
seam = np.maximum(band(cz - COWL_SPLIT, 0.0016, 0.0012) * side_on, band(cx - FIREWALL, 0.0016, 0.0012))
fastener = np.hypot(cx - np.round(cx / FASTENERS) * FASTENERS, cz - (COWL_SPLIT - 0.012)) < 0.0045
seam = np.maximum(seam, fastener * side_on * (cx > FIREWALL + 0.05))
clean = np.where(cowl, clean * (1.0 - 0.35 * seam), clean)
whiteness = smooth(0.45, 0.6, low)
target = lum + (clean - lum) * whiteness
base[..., :3] *= (np.maximum(target, 1e-4) / np.maximum(lum, 1e-4))[..., None]
base[..., :3] = np.clip(base[..., :3], 0.0, 1.0)
lum = base[..., :3] @ np.array([0.2126, 0.7152, 0.0722], np.float32)

# The paint's roughness: the picture's grime kept, but faint, so that the sky it mirrors does not
# come out in blotches.
mr_node = body_mat.node_tree.nodes["Principled BSDF"].inputs["Roughness"].links[0].from_node
while mr_node.type != 'TEX_IMAGE':
    mr_node = mr_node.inputs[0].links[0].from_node
mr = pixels(mr_node.image)
rough = mr[..., 1]
broad = box(rough, 24)
rough = np.clip(np.mean(rough) + (broad - np.mean(rough)) * 0.4 + (rough - broad) * 0.3, 0.22, 1.0)
mr[..., 1] = np.where(whiteness > 0.5, rough, mr[..., 1])
# The wheels' fairings are glossier, moulded and polished: the sky and the ground run over them.
fairing = placed & (at[..., 2] > 0.72) & (whiteness > 0.5)
mr[..., 1] = np.where(fairing, 0.13 + 0.25 * (mr[..., 1] - np.mean(rough)), mr[..., 1])
mr_image = bpy.data.images.new("Body_metallic_roughness", W, H, alpha=True)
mr_image.colorspace_settings.name = 'Non-Color'
mr_image.pixels.foreach_set(mr.astype(np.float32).ravel())
mr_node.image = mr_image

# The normal map: the panel lines, rivets and seams the base colour shows (darker than what is
# round them) pressed into the skin. Its rows go up the picture, as glTF's +y.
height = np.clip(lum - box(lum, 3), -0.10, 0.03)
gx = (np.roll(height, -1, 1) - np.roll(height, 1, 1)) * 0.5
gy = (np.roll(height, -1, 0) - np.roll(height, 1, 0)) * 0.5
STRENGTH = 3.0
normal = np.stack([-gx * STRENGTH, -gy * STRENGTH, np.ones_like(gx)], -1)
normal /= np.linalg.norm(normal, axis=-1, keepdims=True)
normal_image = bpy.data.images.new("Body_normal", W, H, alpha=True)
normal_image.colorspace_settings.name = 'Non-Color'
normal_image.pixels.foreach_set(np.concatenate([normal * 0.5 + 0.5, np.ones((H, W, 1), np.float32)], -1).astype(np.float32).ravel())
for mat in (body_mat, spinner_mat):
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    tex = nodes.new('ShaderNodeTexImage')
    tex.image = normal_image
    nmap = nodes.new('ShaderNodeNormalMap')
    links.new(tex.outputs["Color"], nmap.inputs["Color"])
    links.new(nmap.outputs["Normal"], nodes["Principled BSDF"].inputs["Normal"])

# The livery, painted on the white skin in the aircraft's axes: a navy cheatline with a red
# pinstripe under it from the cowling to the tail, sweeping up the tail cone onto the fin, a
# second band on the fin, and the registration on the tail cone (also a decal of its own, sharp
# close up: below). Each texel is painted by where on the aircraft it is, so the stripes run
# straight across the pieces of the picture. The paint keeps the picture's dirt, panel lines and
# rivets.
NAVY = np.array([0.012, 0.030, 0.105])
RED = np.array([0.45, 0.018, 0.022])
REGISTRATION = "EC-SKY"
GLYPHS = {
    "E": [[(4, 6), (0, 6), (0, 0), (4, 0)], [(0, 3), (3, 3)]],
    "C": [[(4, 5), (3, 6), (1, 6), (0, 5), (0, 1), (1, 0), (3, 0), (4, 1)]],
    "-": [[(0.8, 3), (3.2, 3)]],
    "S": [[(4, 5), (3, 6), (1, 6), (0, 5), (0, 4), (1, 3), (3, 3), (4, 2), (4, 1), (3, 0), (1, 0), (0, 1)]],
    "K": [[(0, 0), (0, 6)], [(4, 6), (0, 2.4)], [(1.5, 3.8), (4, 0)]],
    "Y": [[(0, 6), (2, 3), (4, 6)], [(2, 3), (2, 0)]],
}
# The registration's letters: their height, the line they stand on (down), where the text begins
# and ends along the tail cone (x).
LETTERS = (0.30, 0.44, -3.85, -2.2)

def stripe_centre(x):
    # Down (z) of the middle of the cheatline at x: level under the windows, rising aft.
    t = np.maximum(-1.4 - x, 0.0)
    return 0.15 + 0.02 * np.clip((x + 1.4) / 3.4, 0.0, 1.0) - 0.085 * t ** 1.45

def ink(u, v, aa=0.12):
    # How much a point (u along the text, v up, in the glyphs' units: 6 high) is inside the letters.
    best = np.full(u.shape, 99.0)
    for i, ch in enumerate(REGISTRATION):
        lu = u - i * 5.6
        for line in GLYPHS[ch]:
            for (ax, ay), (bx, by) in zip(line, line[1:]):
                dx, dy = bx - ax, by - ay
                t = np.clip(((lu - ax) * dx + (v - ay) * dy) / (dx * dx + dy * dy), 0.0, 1.0)
                best = np.minimum(best, np.hypot(lu - ax - t * dx, v - ay - t * dy))
    return np.clip((0.62 - best) / aa + 0.5, 0.0, 1.0)

def letter_place(x, y, z):
    # A point of the tail cone in the letters' units: along the text (from tail to nose on the
    # right, from nose to tail on the left, so that both read), and up.
    h, base_z, first, last = LETTERS
    return np.where(y > 0, x - first, last - x) / h * 6.0, (base_z - z) / h * 6.0

def livery(p, n):
    x, y, z = p[:, 0], p[:, 1], p[:, 2]
    # Up to the front of the cowling, its end cut on a slant, raked back.
    zc = stripe_centre(x)
    side = np.clip((np.abs(n[:, 1]) - 0.2) / 0.25, 0.0, 1.0) * (np.abs(y) < 0.75) * (z < 0.72) * np.clip((1.68 + (z - zc) * 0.9 - x) / 0.006, 0.0, 1.0) * (x > -6.3)
    aa = 0.004
    navy = band(z - zc, 0.062, aa)
    red = band(z - (zc + 0.105), 0.021, aa)
    fin = (x < -4.55) * band(z - (zc - 0.20), 0.028, aa)
    u, v = letter_place(x, y, z)
    letters = ink(u, v) * (v > -1) * (v < 7) * (u > -1) * (u < 34)
    colour = np.zeros((len(x), 3))
    alpha = np.zeros(len(x))
    for a, c in ((navy, NAVY), (red, RED), (fin, NAVY), (letters, NAVY)):
        a = a * side
        colour = colour * (1 - a[:, None]) + c * a[:, None]
        alpha = alpha + a * (1 - alpha)
    # The paints laid one over the other (premultiplied), then their colour where any is.
    return colour / np.maximum(alpha[:, None], 1e-4), alpha

paint = np.zeros((H, W, 3), np.float32)
cover = np.zeros((H, W), np.float32)
paint[placed], cover[placed] = livery(at[placed], facing[placed])
lin = srgb_to_linear(base[..., :3])
white = np.percentile(lin[..., 1][cover > 0.0], 90)
detail = np.clip(lin / max(white, 1e-3), 0.0, 1.15)
lin = lin * (1 - cover[..., None]) + paint * detail * cover[..., None]
out = base.copy()
out[..., :3] = linear_to_srgb(lin)
livery_image = bpy.data.images.new("Body_livery", W, H, alpha=True)
livery_image.pixels.foreach_set(out.astype(np.float32).ravel())
if os.environ.get("LIVERY_PNG"):
    # To look at the picture painted: LIVERY_PNG=<file.png> Blender -b --python import.py
    livery_image.filepath_raw = os.environ["LIVERY_PNG"]; livery_image.file_format = 'PNG'; livery_image.save()
for mat in (body_mat, spinner_mat):
    mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].links[0].from_node.image = livery_image
print("livery: %d texels painted" % int((cover > 0.5).sum()))

# Ambient occlusion, baked (Cycles): how much of the sky each texel of the body sees, the shade in
# every crease (a wheel in its fairing, the strut's foot, the wing's root, under the cowling).
# glTF's occlusion texture; the game darkens the sky's light and its mirror by it, not the sun's.
scene = bpy.context.scene
scene.render.engine = 'CYCLES'
scene.cycles.device = 'CPU'
scene.cycles.samples = 64
scene.world = scene.world or bpy.data.worlds.new("world")
scene.world.light_settings.distance = 0.45
occlusion = bpy.data.images.new("Body_occlusion", 1024, 1024, alpha=False)
occlusion.colorspace_settings.name = 'Non-Color'
bpy.data.objects["propeller_disc"].hide_render = True
baking = [o for o in scene.objects if o.type == 'MESH' and any(m and m.name in ("Body_MAT", "Spinner_MAT") for m in o.data.materials)]
group = bpy.data.node_groups.new("glTF Material Output", 'ShaderNodeTree')
group.interface.new_socket("Occlusion", in_out='INPUT', socket_type='NodeSocketFloat')
for mat in (body_mat, spinner_mat):
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    tex = nodes.new('ShaderNodeTexImage')
    tex.image = occlusion
    nodes.active = tex
    out_node = nodes.new('ShaderNodeGroup')
    out_node.node_tree = group
    links.new(tex.outputs["Color"], out_node.inputs["Occlusion"])
bpy.ops.object.select_all(action='DESELECT')
for o in baking:
    o.select_set(True)
bpy.context.view_layer.objects.active = baking[0]
bpy.ops.object.bake(type='AO', margin=6, use_clear=True)
ao = pixels(occlusion)
# A little of it: the sky reaches into a crease more than a white sky dome says.
ao[..., :3] = 0.25 + 0.75 * ao[..., :3]
occlusion.pixels.foreach_set(ao.astype(np.float32).ravel())
bpy.data.objects["propeller_disc"].hide_render = False
print("occlusion baked on %d objects" % len(baking))

# The registration as a decal of its own, sharp close up: the faces of the tail cone round it,
# copied a couple of millimetres out, with a picture of the letters at about a millimetre a texel
# (alpha masked; the body's picture has them too, for afar). It stays in the fuselage's node.
DECAL = (-6.0, 40.0, -4.0, 10.0)   # the picture's reach in the letters' units: along, up
decal_size = (2048, 1024)          # the right side's letters in the lower half, the left's in the upper
decal = np.zeros((decal_size[1], decal_size[0], 4), np.float32)
for half in (0, 1):
    rows = decal_size[1] // 2
    gu, gv = np.meshgrid((np.arange(decal_size[0]) + 0.5) / decal_size[0], (np.arange(rows) + 0.5) / rows)
    u = DECAL[0] + gu * (DECAL[1] - DECAL[0])
    v = DECAL[2] + gv * (DECAL[3] - DECAL[2])
    texel = (DECAL[1] - DECAL[0]) / decal_size[0]
    a = ink(u, v, aa=texel * 1.5)
    block = decal[half * rows:(half + 1) * rows]
    block[..., :3] = np.where(a[..., None] > 0.02, linear_to_srgb(NAVY), linear_to_srgb(np.full(3, paint_white ** 2.2)))
    block[..., 3] = a
decal_image = bpy.data.images.new("Registration", decal_size[0], decal_size[1], alpha=True)
decal_image.pixels.foreach_set(decal.ravel())
reg_mat = bpy.data.materials.new("Registration_MAT")
reg_mat.use_nodes = True
nodes, links = reg_mat.node_tree.nodes, reg_mat.node_tree.links
bsdf = nodes["Principled BSDF"]
tex = nodes.new('ShaderNodeTexImage')
tex.image = decal_image
cut = nodes.new('ShaderNodeMath')
cut.operation = 'GREATER_THAN'
cut.inputs[1].default_value = 0.5
links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
links.new(tex.outputs["Alpha"], cut.inputs[0])
links.new(cut.outputs[0], bsdf.inputs["Alpha"])
bsdf.inputs["Roughness"].default_value = 0.3
bsdf.inputs["Metallic"].default_value = 0.0
h, base_z, first, last = LETTERS
decal_faces = 0
for o in [o for o in scene.objects if o.type == 'MESH' and o.name in ("fuselage", "airframe")]:
    me = o.data
    body_slots = [i for i, m in enumerate(me.materials) if m and m.name == "Body_MAT"]
    me.materials.append(reg_mat)
    slot = len(me.materials) - 1
    bm = bmesh.new()
    bm.from_mesh(me)
    uvl = bm.loops.layers.uv[0]
    mw = o.matrix_world
    def inside(f):
        if f.material_index not in body_slots:
            return False
        n = (mw.to_3x3() @ f.normal).normalized()
        if abs(n.y) < 0.4:
            return False
        for vert in f.verts:
            p = mw @ vert.co
            u, v = letter_place(np.array([p.x]), np.array([p.y]), np.array([p.z]))
            if not (DECAL[0] < u[0] < DECAL[1] and DECAL[2] < v[0] < DECAL[3]):
                return False
        return True
    faces = [f for f in bm.faces if inside(f)]
    if not faces:
        bm.free()
        continue
    copied = bmesh.ops.duplicate(bm, geom=faces)
    new_faces = [g for g in copied["geom"] if isinstance(g, bmesh.types.BMFace)]
    new_verts = [g for g in copied["geom"] if isinstance(g, bmesh.types.BMVert)]
    bm.normal_update()
    for vert in new_verts:
        vert.co += vert.normal * 0.002
    for f in new_faces:
        f.material_index = slot
        for loop in f.loops:
            p = mw @ loop.vert.co
            u, v = letter_place(np.array([p.x]), np.array([p.y]), np.array([p.z]))
            right = p.y > 0
            loop[uvl].uv = ((u[0] - DECAL[0]) / (DECAL[1] - DECAL[0]), ((v[0] - DECAL[2]) / (DECAL[3] - DECAL[2])) * 0.5 + (0.0 if right else 0.5))
    decal_faces += len(new_faces)
    bm.to_mesh(me)
    bm.free()
print("registration decal: %d faces" % decal_faces)

bpy.ops.export_scene.gltf(filepath=OUT, export_format='GLB', export_yup=False, export_apply=True, use_selection=False, export_texcoords=True, export_normals=True, export_materials='EXPORT', export_image_format='JPEG', export_jpeg_quality=92)
print("wrote", OUT)

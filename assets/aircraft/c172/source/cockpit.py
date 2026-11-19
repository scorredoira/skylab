# Makes assets/aircraft/c172/cockpit.glb, in Blender:
# `Blender -b --python assets/aircraft/c172/source/cockpit.py`, from
# - panel.glb: the G1000 panel and what is on it, its glare shield, the pedestal, the magnetic compass, the yokes,
#   pedals, levers and knobs: made for the game, each material named for what it is made of
#   (`crates/games/flight/src/cockpit/file.rs`), the nodes cockpit.ron moves;
# - source.glb: the model's inside with its pictures (the lining with the windows' frames and the
#   doors, the floor, the seats on their rails, the belts) and its windows'
#   glass, moved to the aircraft's axes as import.py moves its outside, so both fit.
#
# The model's own panel (dials that do not work) and its glare shield are left out: the G1000 and
# its straight glare shield stand where they were.
# The game draws the model's materials with their pictures, per pixel: the colours all at COLOUR
# pixels a side and the normal maps at RELIEF.
import bpy, bmesh, os
from mathutils import Matrix, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "cockpit.glb")
COLOUR, RELIEF = 2048, 1024

# The model to body axes (import.py's), and body axes (x forward, y right, z down) to Blender's
# as the glTF importer leaves panel.glb (x, up, right): the file is written back the same way.
S = 0.874
TX, TZ = -0.092, 1.194
M = Matrix(((0, -S, 0, TX), (-S, 0, 0, 0), (0, 0, -S, TZ), (0, 0, 0, 1)))
B = Matrix(((1, 0, 0, 0), (0, 0, -1, 0), (0, 1, 0, 0), (0, 0, 0, 1)))

def body(p):
    return B.inverted() @ p

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=os.path.join(HERE, "panel.glb"))
ours = set(bpy.context.scene.objects)
bpy.ops.import_scene.gltf(filepath=os.path.join(HERE, "source.glb"))

# What is taken of the model, and the node each piece becomes.
TAKE = {"Interior_Body": "interior", "Seats_MAT": "interior", "SeatGear": "interior", "SeatBelt": "interior", "Exterior_Glass": "glass"}
pieces = {}
for o in list(set(bpy.context.scene.objects) - ours):
    name = next((node for part, node in TAKE.items() if part in o.name), None) if o.type == 'MESH' else None
    if name:
        o.parent = None
        o.matrix_world = B @ M @ o.matrix_world
        pieces.setdefault(name, []).append(o)
for o in list(set(bpy.context.scene.objects) - ours):
    if not any(o in group for group in pieces.values()):
        bpy.data.objects.remove(o)

def apply(o):
    bpy.ops.object.select_all(action='DESELECT')
    o.select_set(True)
    bpy.context.view_layer.objects.active = o
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

for group in pieces.values():
    for o in group:
        apply(o)

# The islands of a mesh: the vertices of each connected piece.
def islands(bm):
    seen = set()
    for v in bm.verts:
        if v in seen:
            continue
        stack, island = [v], []
        seen.add(v)
        while stack:
            a = stack.pop()
            island.append(a)
            for e in a.link_edges:
                b = e.other_vert(a)
                if b not in seen:
                    seen.add(b)
                    stack.append(b)
        yield island

def bounds(island):
    points = [body(v.co) for v in island]
    return Vector([min(p[i] for p in points) for i in range(3)]), Vector([max(p[i] for p in points) for i in range(3)])

# The lining: the model's panel board goes (a flat sheet with the dials' holes at x 0.58 to 0.60)
# and its glare shield, an arch that comes down at its ends (an older 172's, over round dials):
# the G1000's panel is square under a straight glare shield, panel.glb's.
for o in pieces["interior"]:
    if "Interior_Body" not in o.name:
        continue
    bm = bmesh.new()
    bm.from_mesh(o.data)
    drop = set()
    for island in islands(bm):
        low, high = bounds(island)
        board = low.x > 0.55 and high.x < 0.63 and low.z > -0.45 and high.z < -0.05
        shield = low.x > 0.44 and high.x < 0.85 and low.z > -0.50 and high.z < -0.20 and high.y - low.y > 0.9
        if board or shield:
            drop |= {f for v in island for f in v.link_faces}
    bmesh.ops.delete(bm, geom=list(drop), context='FACES')
    # The floor and the firewall under the panel: the model's picture there is a blur of stains.
    # They are the game's carpet (charcoal) and dark trim, by its own materials.
    slots = {}
    for name, colour, rough in (("carpet", (0.085, 0.085, 0.09), 0.95), ("vinyl", (0.06, 0.06, 0.065), 0.8)):
        material = bpy.data.materials.new(name)
        material.use_nodes = True
        bsdf = material.node_tree.nodes["Principled BSDF"]
        bsdf.inputs["Base Color"].default_value = (*colour, 1.0)
        bsdf.inputs["Roughness"].default_value = rough
        bsdf.inputs["Metallic"].default_value = 0.0
        o.data.materials.append(material)
        slots[name] = len(o.data.materials) - 1
    for f in bm.faces:
        centre, normal = body(f.calc_center_median()), body(f.normal)
        if centre.z > 0.36 and normal.z < -0.6 and centre.x > -1.3:
            f.material_index = slots["carpet"]
        elif centre.x > 0.70 and centre.z > -0.22 and normal.x < -0.6:
            f.material_index = slots["vinyl"]
    bm.to_mesh(o.data)
    bm.free()

# The belts have as many triangles as the lining: a seventh of them is plenty.
for o in pieces["interior"]:
    if "SeatBelt" in o.name:
        o.modifiers.new("lighter", 'DECIMATE').ratio = 0.15
        bpy.ops.object.select_all(action='DESELECT')
        o.select_set(True)
        bpy.context.view_layer.objects.active = o
        bpy.ops.object.modifier_apply(modifier="lighter")

# The windows' glass: the model has both faces of each pane; the inner one is enough (the game
# draws it over the world). Of each pair, the larger (the outer) goes.
for o in pieces["glass"]:
    bm = bmesh.new()
    bm.from_mesh(o.data)
    panes = [(island, *bounds(island)) for island in islands(bm)]
    drop = set()
    for k, (island, low, high) in enumerate(panes):
        centre = (low + high) / 2
        for other, lo, hi in panes[:k] + panes[k + 1:]:
            c = (lo + hi) / 2
            if (c - centre).length < 0.05 and (hi - lo).length > (high - low).length:
                drop |= {f for v in island for f in v.link_faces}
    bmesh.ops.delete(bm, geom=list(drop), context='FACES')
    bm.to_mesh(o.data)
    bm.free()


# The magnetic compass hung from the old cabin's windscreen frame: moved to this one's, 21 cm aft
# and 12 cm lower. The glare shield was made square, 1.02 m wide from the panel (x 0.588) to x 1.0:
# it is given this windscreen's shape seen from above, its front edge a centimetre into the glass
# (FOOT: where the glass is at the shield's height, x for each y), so that it ends at the glass
# all round and the cowling shows over it.
COMPASS = Vector((-0.21, 0.0, 0.12))
SHIELD = (0.588, 1.0, 1.02)
FOOT = ((0.0, 0.819), (0.2, 0.812), (0.3, 0.798), (0.4, 0.754), (0.45, 0.712), (0.5, 0.642), (0.53, 0.57), (0.56, 0.50))

def foot(y):
    y = min(abs(y), FOOT[-1][0])
    for (y0, x0), (y1, x1) in zip(FOOT, FOOT[1:]):
        if y <= y1:
            return x0 + (x1 - x0) * (y - y0) / (y1 - y0)
cabin = bpy.data.objects["cabin"]
leather = [k for k, m in enumerate(cabin.data.materials) if m.name.split(".")[0] == "leather"]
bm = bmesh.new()
bm.from_mesh(cabin.data)
inverse = cabin.matrix_world.inverted()
for island in islands(bm):
    low, high = bounds([type("P", (), {"co": cabin.matrix_world @ v.co}) for v in island])
    if low.x >= 0.35 and high.z <= -0.6:
        for v in island:
            v.co = inverse @ (B @ (body(cabin.matrix_world @ v.co) + COMPASS))
    elif high.y - low.y > 0.9 and low.x > 0.55 and high.z < -0.2 and all(f.material_index in leather for v in island for f in v.link_faces):
        for v in island:
            p = body(cabin.matrix_world @ v.co)
            back, front, wider = SHIELD
            p.y *= wider
            p.x = back + (p.x - back) * max(foot(p.y) + 0.01 - back, 0.004) / (front - back)
            v.co = inverse @ (B @ p)
bm.to_mesh(cabin.data)
bm.free()

def join(objs, name):
    bpy.ops.object.select_all(action='DESELECT')
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    if len(objs) > 1:
        bpy.ops.object.join()
    o = bpy.context.view_layer.objects.active
    o.name = name
    o.data.name = name
    return o

interior = join(pieces["interior"], "interior")
glass = join(pieces["glass"], "glass")
for o in (interior, glass):
    for attribute in list(o.data.color_attributes):
        o.data.color_attributes.remove(attribute)

# The materials as the game draws them (`cockpit::Material::Textured`): the colour's picture and
# the normal map, nothing else; how rough each is, a number.
ROUGH = {"Interior_Body_MAT": 0.62, "Seats_MAT": 0.9, "SeatGear_MAT": 0.5, "SeatBelt_MAT": 0.85}
for m in bpy.data.materials:
    if m.name not in ROUGH:
        continue
    tree = m.node_tree
    bsdf = tree.nodes["Principled BSDF"]
    for socket in ("Metallic", "Roughness", "Specular IOR Level", "Emission Color", "Alpha"):
        if socket in bsdf.inputs:
            for link in list(bsdf.inputs[socket].links):
                tree.links.remove(link)
    bsdf.inputs["Metallic"].default_value = 0.0
    bsdf.inputs["Roughness"].default_value = ROUGH[m.name]
    bsdf.inputs["Alpha"].default_value = 1.0

    def picture(socket):
        node = bsdf.inputs[socket].links[0].from_node if bsdf.inputs[socket].links else None
        while node and node.type != 'TEX_IMAGE':
            link = next((i.links[0] for i in node.inputs if i.links), None)
            node = link.from_node if link else None
        return node.image if node else None

    colour, relief = picture("Base Color"), picture("Normal")
    if colour and tuple(colour.size) != (COLOUR, COLOUR):
        colour.scale(COLOUR, COLOUR)
    if relief and tuple(relief.size) != (RELIEF, RELIEF):
        relief.scale(RELIEF, RELIEF)
    print(f"{m.name}: colour {colour.name if colour else None}, relief {relief.name if relief else None}")

bpy.ops.export_scene.gltf(filepath=OUT, export_format='GLB', export_apply=True, export_texcoords=True, export_normals=True, export_materials='EXPORT', export_vertex_color='ACTIVE', export_all_vertex_colors=False, export_image_format='JPEG', export_jpeg_quality=90, export_animations=False, export_cameras=False, export_lights=False)
print("wrote", OUT)

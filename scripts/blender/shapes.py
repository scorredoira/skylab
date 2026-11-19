# What the models' scripts share: a mesh built up of shapes (skins over sections, shapes turned
# round an axis, tubes along a path, rounded boxes), each face facing out as it is made, and the
# export as the game reads it. Coordinates are the game's body axes (x forward, y right, z down),
# written as they are.

import math
import os

import bpy

# Where each object made here is, in the model's space: a child is placed from its parent's.
ORIGINS = {}


def sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def add(a, b):
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def scale(a, k):
    return (a[0] * k, a[1] * k, a[2] * k)


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def length(a):
    return math.sqrt(dot(a, a))


def unit(a):
    n = length(a)
    return scale(a, 1 / n) if n > 1e-12 else (0.0, 0.0, 0.0)


def middle(points):
    n = len(points)
    return (sum(p[0] for p in points) / n, sum(p[1] for p in points) / n, sum(p[2] for p in points) / n)


def catmull(keys, x):
    """A value through key points (x increasing), smoothly (Catmull-Rom)."""
    last = len(keys) - 1
    i = next((i for i in range(last) if x <= keys[i + 1][0]), last - 1)
    p0, p1, p2, p3 = keys[max(i - 1, 0)][1], keys[i][1], keys[i + 1][1], keys[min(i + 2, last)][1]
    t = min(1.0, max(0.0, (x - keys[i][0]) / (keys[i + 1][0] - keys[i][0])))
    return 0.5 * (2 * p1 + (p2 - p0) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t * t + (3 * p1 - p0 - 3 * p2 + p3) * t ** 3)


def superellipse(a, e):
    """The point at angle a of a superellipse of exponent e (2 a circle, more a rounder box)."""
    c, s = math.cos(a), math.sin(a)
    return (math.copysign(abs(c) ** (2 / e), c), math.copysign(abs(s) ** (2 / e), s))


def aerofoil(thickness, n):
    """A symmetric aerofoil's outline (a NACA four-digit section's thickness) as a closed loop of
    2 n points: (share of the chord from the leading edge, half thickness over the chord, signed)."""

    def half(x):
        return 5 * thickness * (0.2969 * math.sqrt(x) - 0.126 * x - 0.3516 * x * x + 0.2843 * x ** 3 - 0.1036 * x ** 4)

    def station(k):
        return 0.5 * (1 - math.cos(math.pi * k / n))

    return [(station(k), -half(station(k))) for k in range(n)] + [(station(k), half(station(k))) for k in range(n, 0, -1)]


class Builder:
    """A mesh built up: points, faces and each face's material."""

    def __init__(self):
        self.points = []
        self.faces = []
        self.materials = []
        self.slots = {}
        # Each face's corners' places in a picture (u across, v down), for those that show one.
        self.uvs = []

    def slot(self, material):
        if material[0] not in self.slots:
            self.slots[material[0]] = (len(self.slots), material)
        return self.slots[material[0]][0]

    def face(self, corners, material, away_from=None, towards=None, uv=None):
        """A face of these corners, its normal away from a point, or along a direction; `uv`,
        each corner's place in the picture it shows."""
        if uv is None:
            corners = [c for i, c in enumerate(corners) if length(sub(c, corners[i - 1])) > 1e-7]
        if len(corners) < 3:
            return
        normal = (0.0, 0.0, 0.0)
        for i in range(1, len(corners) - 1):
            normal = add(normal, cross(sub(corners[i], corners[0]), sub(corners[i + 1], corners[0])))
        if length(normal) < 1e-12:
            return
        want = towards if towards is not None else sub(middle(corners), away_from)
        if dot(normal, want) < 0:
            corners = corners[::-1]
            uv = uv[::-1] if uv is not None else None
        self.uvs.append(uv)
        first = len(self.points)
        self.points.extend(corners)
        self.faces.append(tuple(range(first, first + len(corners))))
        self.materials.append(self.slot(material))

    def box(self, lo, hi, material, round=0.0):
        """A box between two corners, its edges rounded by `round`."""
        lo, hi = tuple(min(a, b) for a, b in zip(lo, hi)), tuple(max(a, b) for a, b in zip(lo, hi))
        cy, cz = (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2

        def ring(x, inset):
            hy, hz = (hi[1] - lo[1]) / 2 - inset, (hi[2] - lo[2]) / 2 - inset
            r = max(0.0, min(round, hy, hz))
            if r <= 0:
                return [(x, cy + hy, cz + hz), (x, cy - hy, cz + hz), (x, cy - hy, cz - hz), (x, cy + hy, cz - hz)]
            points = []
            for corner, (sy, sz) in enumerate(((1, 1), (-1, 1), (-1, -1), (1, -1))):
                for i in range(5):
                    a = math.pi / 2 * (corner + i / 4)
                    points.append((x, cy + sy * (hy - r) + r * math.cos(a), cz + sz * (hz - r) + r * math.sin(a)))
            return points

        if round > 0 and hi[0] - lo[0] > 2 * round:
            rings = [ring(lo[0], round * 0.6), ring(lo[0] + round * 0.6, 0.0), ring(hi[0] - round * 0.6, 0.0), ring(hi[0], round * 0.6)]
        else:
            rings = [ring(lo[0], 0.0), ring(hi[0], 0.0)]
        loft(self, rings, material, caps=True)

    def object(self, name, origin=(0.0, 0.0, 0.0), parent=None):
        """The mesh as an object named `name`, its origin at `origin` (the model's space)."""
        mesh = bpy.data.meshes.new(name)
        mesh.from_pydata([sub(p, origin) for p in self.points], [], self.faces)
        for _, (_, (label, colour, metallic, roughness)) in sorted(self.slots.items(), key=lambda item: item[1][0]):
            mesh.materials.append(material(label, colour, metallic, roughness))
        mesh.polygons.foreach_set("material_index", self.materials)
        mesh.polygons.foreach_set("use_smooth", [True] * len(self.faces))
        if any(uv is not None for uv in self.uvs):
            # Blender's v goes up; the picture's, down.
            layer = mesh.uv_layers.new(name="UVMap")
            for polygon, uv in zip(mesh.polygons, self.uvs):
                for loop, (u, v) in zip(polygon.loop_indices, uv or [(0.0, 0.0)] * polygon.loop_total):
                    layer.data[loop].uv = (u, 1.0 - v)
        mesh.update()
        # Points of faces that meet are one; edges sharper than 40° stay sharp.
        merge(mesh)
        mesh.set_sharp_from_angle(angle=math.radians(40))
        obj = bpy.data.objects.new(name, mesh)
        bpy.context.scene.collection.objects.link(obj)
        ORIGINS[name] = origin
        if parent is not None:
            obj.parent = parent
            obj.location = sub(origin, ORIGINS[parent.name])
        else:
            obj.location = origin
        return obj


def merge(mesh):
    import bmesh

    bm = bmesh.new()
    bm.from_mesh(mesh)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
    bm.to_mesh(mesh)
    bm.free()


MATERIALS = {}


def material(label, colour, metallic, roughness):
    if label in MATERIALS:
        return MATERIALS[label]
    m = bpy.data.materials.new(label)
    m.use_nodes = True
    shader = m.node_tree.nodes.get("Principled BSDF")
    shader.inputs["Base Color"].default_value = (*colour, 1.0)
    shader.inputs["Metallic"].default_value = metallic
    shader.inputs["Roughness"].default_value = roughness
    MATERIALS[label] = m
    return m


def loft(b, rings, material, caps=False, closed=True, away_from=None, colour=None):
    """A skin over rings of as many points each: closed loops (a body: its faces away from the
    rings' middles, its ends capped with `caps`), or open rows (a sheet: its faces away from
    `away_from`). `colour(ring, point)` gives a face its own material, or leaves it out (None)."""
    count = len(rings[0])
    centres = [middle(r) for r in rings]
    for i in range(len(rings) - 1):
        for k in range(count if closed else count - 1):
            corners = [rings[i][k], rings[i + 1][k], rings[i + 1][(k + 1) % count], rings[i][(k + 1) % count]]
            paint = colour(i, k) if colour else material
            if paint is None:
                continue
            b.face(corners, paint, away_from=away_from if away_from is not None else middle([centres[i], centres[i + 1]]))
    if caps and closed:
        for ring, centre, other in ((rings[0], centres[0], centres[1]), (rings[-1], centres[-1], centres[-2])):
            for k in range(count):
                b.face([centre, ring[k], ring[(k + 1) % count]], material, towards=sub(centre, other))


def revolve(b, profile, material, centre=(0.0, 0.0, 0.0), axis="z", sides=24, closed=False, flip=False, sign=1):
    """A profile of (radius, place along the axis) turned round an axis through `centre`. A
    closed profile is a body (a ring), its faces away from the profile's middle; an open one
    faces away from the axis where it runs along it, and for what is flat across it (a disc) the
    way the axis points, or the other way with `flip`."""

    def at(r, a, t):
        c, s = r * math.cos(t), r * math.sin(t)
        if axis == "x":
            return (centre[0] + sign * a, centre[1] + c, centre[2] + s)
        if axis == "y":
            return (centre[0] + c, centre[1] + sign * a, centre[2] + s)
        return (centre[0] + c, centre[1] + s, centre[2] + sign * a)

    mid_r = sum(p[0] for p in profile) / len(profile)
    mid_a = sum(p[1] for p in profile) / len(profile)
    count = len(profile)
    for i in range(sides):
        t0, t1 = math.tau * i / sides, math.tau * (i + 1) / sides
        tm = (t0 + t1) / 2
        for k in range(count if closed else count - 1):
            (r0, a0), (r1, a1) = profile[k], profile[(k + 1) % count]
            corners = [at(r0, a0, t0), at(r1, a1, t0), at(r1, a1, t1), at(r0, a0, t1)]
            if closed:
                b.face(corners, material, away_from=at(mid_r, mid_a, tm))
            elif abs(a1 - a0) > 1e-9 or r0 + r1 < 1e-9:
                # Away from the axis, at the face's own place along it.
                b.face(corners, material, away_from=at(0.0, (a0 + a1) / 2 - (r1 - r0) * 0.0, tm), )
            else:
                along = sub(at(0.0, 1.0, 0.0), at(0.0, 0.0, 0.0))
                b.face(corners, material, towards=scale(along, -1.0 if flip else 1.0))
    # A body's ends, where its profile starts or ends off the axis, are left open: close them
    # with a point on the axis in the profile.


def tube(b, path, radius, material, sides=8, cap=True, smooth_path=False):
    """A round bar along a path of points; `smooth_path` bends it through them."""
    if smooth_path and len(path) > 2:
        fine = []
        for i in range(len(path) - 1):
            p0, p1, p2, p3 = path[max(i - 1, 0)], path[i], path[i + 1], path[min(i + 2, len(path) - 1)]
            for j in range(8):
                t = j / 8
                fine.append(tuple(0.5 * (2 * p1[c] + (p2[c] - p0[c]) * t + (2 * p0[c] - 5 * p1[c] + 4 * p2[c] - p3[c]) * t * t + (3 * p1[c] - p0[c] - 3 * p2[c] + p3[c]) * t ** 3) for c in range(3)))
        fine.append(path[-1])
        path = fine
    # A frame carried along the path without twisting.
    tangent = unit(sub(path[1], path[0]))
    side = unit(cross(tangent, (0.0, 0.0, 1.0) if abs(tangent[2]) < 0.9 else (1.0, 0.0, 0.0)))
    rings = []
    for i, p in enumerate(path):
        ahead = unit(sub(path[min(i + 1, len(path) - 1)], path[max(i - 1, 0)]))
        side = unit(sub(side, scale(ahead, dot(side, ahead))))
        up = cross(ahead, side)
        rings.append([add(p, add(scale(side, radius * math.cos(math.tau * k / sides)), scale(up, radius * math.sin(math.tau * k / sides)))) for k in range(sides)])
    loft(b, rings, material, caps=cap)


def clear():
    """An empty scene, for another model."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    MATERIALS.clear()
    ORIGINS.clear()


def export(path):
    """Writes every object as a .glb, in the axes they were made in."""
    path = os.path.abspath(path)
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.export_scene.gltf(filepath=path, export_format="GLB", export_yup=False, export_apply=True, export_animations=False, export_cameras=False, export_lights=False)
    print(f"{path}: {os.path.getsize(path)} bytes")

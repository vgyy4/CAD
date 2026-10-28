#!/usr/bin/env python3
"""judgegen: mechanical OCCT -> Rust translation judges (oracle + replay test + API doc).

Usage:
  judgegen.py <Package> <module> <out-dir> [--libs TKMath TKernel ...] [--names names.tsv]
              [--occt DIR] [--src DIR] [--cases N] [--max-bytes N] [--rust-mod NAME] [--stub] [--keep-build] [--rust-only]
              [--no-checks] [--rustfmt-config FILE]

  e.g.  judgegen.py ElCLib elclib_gen /tmp/out --occt $OCCT --names names-elclib.tsv

Needs python3 (standard library only), clang++ (for the JSON AST) and a C++ compiler (`c++`, or $CXX) with an OCCT
8.0.1 install. What it does:
  1. dumps the clang JSON AST of every header of the package (as tools/cxx2rs.py does) and collects the public
     classes, constructors, methods (static / const / virtual), free functions and enums (also enums nested in a class)
     with parameter and return types;
  2. keeps what it can drive mechanically: f64 / i32 / bool, enums, the gp value types (Pnt Vec Dir XYZ Ax1 Ax2 Ax3
     Lin Circ Elips Hypr Parab Pln Cylinder Cone Sphere Torus Mat Trsf Quaternion and the 2D forms), TColgp /
     TColStd arrays of those, out-parameters (non-const references; a scalar one that the call also reads is found by
     *probing* and becomes an in-out parameter) and objects of the package's classes built by a constructor it can drive
     (their state is read back through the zero-argument const getters: the *snapshot*). Everything else (operators,
     handles of other kinds, streams, abstract classes, callbacks, pointers, enums with explicit values...) is listed as
     "not generated" in the API doc, with the reason, for a hand-written family. Also driven:
       * `Handle(Geom_*)` / `Handle(Geom2d_*)` curves and surfaces (any class of the Geom hierarchy, e.g. Geom_Curve,
         Geom_BoundedCurve, Geom_Circle, Geom_Surface, Geom_Plane ...) as arguments and results. An argument is a printed
         *recipe* (the grammar of the reviewed judges tests/geom.rs / tests/geom2d.rs: `kind ctor numbers ...`, recursive for
         trimmed / offset curves and swept / trimmed / offset surfaces), rebuilt on the Rust side by builders copied from those
         judges (`mod h3`, `mod h2` of the generated test); a class argument gets a recipe of the kinds the class can have and
         arrives as `Arc<dyn Curve>` / `Arc<Circle>` ... A result is snapshotted through its evaluation: 1 (0 for a null handle),
         bounds, periodicity, continuity, EvalD0 and EvalD1 at five parameters (surfaces: bounds, closedness / periodicity,
         continuity, D0 / D1 at four points), `x` where an evaluation raised;
       * adaptors: `Adaptor3d_Curve`, `Adaptor2d_Curve2d`, `Adaptor3d_Surface` (also as `Handle`) and `GeomAdaptor_Curve`,
         `GeomAdaptor_Surface`, `Geom2dAdaptor_Curve` arguments, built from a printed description (use_range, bounds, recipe)
         on the matching Geom curve / surface; results are snapshotted through the adaptor interface;
       * `Geom_Curve::ResD1..3` / `Geom_Surface::ResD1..3` (also as `NCollection_Array1/2` of them, e.g. GeomGridEval::CurveD1)
         and `Geom2dGridEval::CurveD1..3` as results; `Bnd_Box` / `Bnd_Box2d` arguments (a non-const reference is filled by the
         call and printed afterwards: void flag, the corners, the gap);
       * "make" classes (gce_Make*, GC_Make*): constructor + the zero-argument getters IsDone / Status / Value of the snapshot;
         a package that only holds deprecated `using` aliases (GCE2d) says so and lists them;
     The shipped OCCT libraries are built with -DNo_Exception (every `Raise_if` check is compiled out). To get the
     documented behaviour the sources of the package (`--src`, default <occt>/../occt-src) are compiled into the oracle
     WITHOUT that define, so an out-of-range index or a zero divisor raises instead of returning garbage: a `throws`
     line means "the documented check fires" and the Rust API must return `Err` (`--no-checks` links the library as built).
  3. writes <out>/oracle/<module>.cpp: one family per constructor / method / overload, >= 16 distinct cases each from a
     fixed xorshift seed (one seed per family) plus edge values (0, negatives, tiny / huge, coincident / parallel /
     opposite gp inputs, tolerances 0 ... 1, out-of-range indices), each call in try/catch printing `throws` for
     Standard_Failure, f64 as 16-hex bit patterns, deterministic; every case is run 6 times with random stack and heap
     fill and dropped when it is not reproducible, not finite or crashes (this finds uninitialised reads); the C
     library's transcendental functions are interposed, so a family is `EXACT` (bit for bit) when none of its cases calls
     one and otherwise only keeps cases that are stable under a 1-ulp perturbation of every such call;
  4. writes <out>/tests/<module>.rs (replays every record on the Rust API by the naming rule below; the tolerance rules,
     tally line and failure message of tests/convert.rs), <out>/docs/api-<module>.md (every Rust signature the harness
     expects, conventions, coverage, the "not generated" list) and <out>/docs/api-<module>.names.tsv (the effective names);
  5. builds and runs the oracle twice (identical output required) into <out>/tests/<module>.txt. If the output would
     exceed --max-bytes (1.5 MB) the families are split in parts <module>_1 ... <module>_N, each a standalone judge.
  --stub additionally writes <out>/src/<rust-mod>.rs, a signature-only Rust module (`unimplemented!()`) to check that
  the generated test compiles for a package that is not translated yet. --keep-build keeps <out>/.build so that
  --rust-only can rewrite the test and the docs (e.g. after editing names.tsv) without running the oracle again.

Naming rule (documented again in the generated API doc; names.tsv overrides it):
  * the Rust name of a C++ method is snake_case of the C++ name (`SquareDistance` -> `square_distance`);
    constructors are `new`; static methods of the class that carries the package name and free functions
    become functions of the Rust module `<package lower case>`; other static methods are associated fns.
  * overloads (same class, same name) get a suffix: if their parameter counts differ, `_<count>`; among
    overloads with the same count the `_<types>` of the shortest list of in-parameters whose type differs (gp_Lin ->
    `lin`, double -> `real`, int -> `int`, an array of points -> `arr_pnt`); a const and non-const overload with the
    same parameters: the non-const one gets `_mut`; a remaining clash gets `_b`, `_c`...
  * names.tsv: `Class::Method(sig)<TAB>rust_name` or `Class::Method/N<TAB>rust_name` (N = C++ parameter
    count), a constructor is `Class::Class(sig)`, a free function / package static `Package::Method(sig)`;
    `sig` is the comma separated list of the parameter type words (`real`, `int`, `bool`, `pnt`, `lin2d`,
    `arr_pnt`, an enum or class name without its package prefix in snake case; an out-parameter has a trailing `&`).
    `Enum::Constant` entries rename enum variants (default: the constant without the `<Package>_` prefix, CamelCase),
    `enum <Enum>` entries rename an enum type. `name@0,2` passes the C++ const-reference parameters 0 and 2 by value
    (a `.clone()`) because the Rust API takes e.g. `Pnt` where the rule expects `&Pnt`. The Rust name `-` marks an item the reviewed API deliberately does not have (it is listed under "not generated").
    A Rust name that starts with `=` is an expression template for
    an API whose shape differs (`={s}.transforms_xyz(&mut {0})`: `{s}` is the receiver, `{0}`.. the arguments the harness passes).
    Lines starting with `#` are comments.
"""
import argparse
import collections
import glob
import json
import os
import re
import shutil
import subprocess
import sys

KEYWORDS = {
    'as', 'break', 'const', 'continue', 'crate', 'else', 'enum', 'extern', 'false', 'fn', 'for', 'if', 'impl', 'in',
    'let', 'loop', 'match', 'mod', 'move', 'mut', 'pub', 'ref', 'return', 'self', 'Self', 'static', 'struct', 'super',
    'trait', 'true', 'type', 'unsafe', 'use', 'where', 'while', 'async', 'await', 'dyn', 'abstract', 'become', 'box',
    'do', 'final', 'macro', 'override', 'priv', 'typeof', 'unsized', 'virtual', 'yield', 'try',
}

# --------------------------------------------------------------------------------------------------------------
# gp value types: word -> (C++ class, Rust type, number of raw tokens or None if variable)
GP = collections.OrderedDict([
    ('pnt', ('gp_Pnt', 'Pnt', 3)), ('vec', ('gp_Vec', 'GpVec', 3)), ('dir', ('gp_Dir', 'Dir', 3)),
    ('xyz', ('gp_XYZ', 'XYZ', 3)), ('ax1', ('gp_Ax1', 'Ax1', 6)), ('ax2', ('gp_Ax2', 'Ax2', 9)),
    ('ax3', ('gp_Ax3', 'Ax3', 10)), ('lin', ('gp_Lin', 'Lin', 6)), ('circ', ('gp_Circ', 'Circ', 10)),
    ('elips', ('gp_Elips', 'Elips', 11)), ('hypr', ('gp_Hypr', 'Hypr', 11)), ('parab', ('gp_Parab', 'Parab', 10)),
    ('pln', ('gp_Pln', 'Pln', 10)), ('cylinder', ('gp_Cylinder', 'Cylinder', 11)), ('cone', ('gp_Cone', 'Cone', 12)),
    ('sphere', ('gp_Sphere', 'Sphere', 11)), ('torus', ('gp_Torus', 'Torus', 12)), ('mat', ('gp_Mat', 'Mat', 9)),
    ('trsf', ('gp_Trsf', 'Trsf', None)), ('quat', ('gp_Quaternion', 'Quaternion', 4)),
    ('pnt2d', ('gp_Pnt2d', 'Pnt2d', 2)), ('vec2d', ('gp_Vec2d', 'Vec2d', 2)), ('dir2d', ('gp_Dir2d', 'Dir2d', 2)),
    ('xy', ('gp_XY', 'XY', 2)), ('ax2d', ('gp_Ax2d', 'Ax2d', 4)), ('ax22d', ('gp_Ax22d', 'Ax22d', 6)),
    ('lin2d', ('gp_Lin2d', 'Lin2d', 4)), ('circ2d', ('gp_Circ2d', 'Circ2d', 7)),
    ('elips2d', ('gp_Elips2d', 'Elips2d', 8)), ('hypr2d', ('gp_Hypr2d', 'Hypr2d', 8)),
    ('parab2d', ('gp_Parab2d', 'Parab2d', 7)), ('mat2d', ('gp_Mat2d', 'Mat2d', 4)),
    ('trsf2d', ('gp_Trsf2d', 'Trsf2d', None)),
])
GP_BY_CLASS = {v[0]: k for k, v in GP.items()}
# element words usable in arrays
ARR_ELEM = {'Pnt': 'pnt', 'Vec': 'vec', 'Dir': 'dir', 'XYZ': 'xyz', 'Pnt2d': 'pnt2d', 'Vec2d': 'vec2d',
            'Dir2d': 'dir2d', 'XY': 'xy', 'Real': 'f', 'Integer': 'i'}
CPP_ELEM = {'gp_Pnt': 'pnt', 'gp_Vec': 'vec', 'gp_Dir': 'dir', 'gp_XYZ': 'xyz', 'gp_Pnt2d': 'pnt2d',
            'gp_Vec2d': 'vec2d', 'gp_Dir2d': 'dir2d', 'gp_XY': 'xy', 'double': 'f', 'Standard_Real': 'f',
            'int': 'i', 'Standard_Integer': 'i'}
ELEM_CPP = {'f': 'double', 'i': 'int'}
ELEM_CPP.update({w: GP[w][0] for w in ARR_ELEM.values() if w in GP})
SCALARS = {'double': 'f', 'Standard_Real': 'f', 'int': 'i', 'Standard_Integer': 'i', 'bool': 'b',
           'Standard_Boolean': 'b'}


def snake(name):
    s = re.sub(r'(?<=[a-z0-9])([A-Z])', r'_\1', name)
    s = re.sub(r'(?<=[A-Z])([A-Z][a-z])', r'_\1', s)
    return s.lower()


def camel(name):
    return ''.join(p[:1].upper() + p[1:] for p in name.split('_') if p)


def rust_ident(n):
    return n + '_' if n in KEYWORDS else n



# --------------------------------------------------------------------------------------------------------------
# handles of Geom_* / Geom2d_* curves and surfaces and the adaptors built from them. A handle argument is a printed
# *recipe* (the grammar of the reviewed judges tests/geom.rs and tests/geom2d.rs); the Rust harness copies their builders.
# class -> (family, Rust type inside Arc<..>, recipe kinds the class can have, concrete Rust struct or None)
_C3 = {'Line': [0], 'Circle': [1], 'Ellipse': [2], 'Hyperbola': [3], 'Parabola': [4], 'BezierCurve': [5], 'BSplineCurve': [6],
       'TrimmedCurve': [7], 'OffsetCurve': [8]}
_S3 = {'Plane': [0], 'CylindricalSurface': [1], 'ConicalSurface': [2], 'SphericalSurface': [3], 'ToroidalSurface': [4],
       'BezierSurface': [5], 'BSplineSurface': [6], 'SurfaceOfLinearExtrusion': [7], 'SurfaceOfRevolution': [8],
       'RectangularTrimmedSurface': [9], 'OffsetSurface': [10]}
H_INFO = {}
for _n, _k in _C3.items():
    H_INFO['Geom_' + _n] = ('c3', 'occt_port::geom::' + _n, _k, _n)
    H_INFO['Geom2d_' + _n] = ('c2', 'occt_port::geom2d::' + _n, _k, _n)
H_INFO['Geom_Curve'] = ('c3', 'dyn occt_port::geom::Curve', list(range(9)), None)
H_INFO['Geom_BoundedCurve'] = ('c3', 'dyn occt_port::geom::BoundedCurve', [5, 6, 7], 'bounded')
H_INFO['Geom_Conic'] = ('c3', 'dyn occt_port::geom::Conic', [1, 2, 3, 4], 'conic')
H_INFO['Geom2d_Curve'] = ('c2', 'dyn occt_port::geom2d::Curve2d', list(range(9)), None)
H_INFO['Geom2d_BoundedCurve'] = ('c2', 'dyn occt_port::geom2d::BoundedCurve2d', [5, 6, 7], 'bounded')
H_INFO['Geom2d_Conic'] = ('c2', 'dyn occt_port::geom2d::Conic2d', [1, 2, 3, 4], 'conic')
for _n, _k in _S3.items():
    H_INFO['Geom_' + _n] = ('s3', 'occt_port::geom::' + _n, _k, _n)
H_INFO['Geom_Surface'] = ('s3', 'dyn occt_port::geom::Surface', list(range(11)), None)
H_INFO['Geom_ElementarySurface'] = ('s3', 'dyn occt_port::geom::ElementarySurface', [0, 1, 2, 3, 4], 'elementary')
H_INFO['Geom_SweptSurface'] = ('s3', 'dyn occt_port::geom::SweptSurface', [7, 8], 'swept')
H_INFO['Geom_BoundedSurface'] = ('s3', 'dyn occt_port::geom::BoundedSurface', [5, 6, 9], 'bounded')
# adaptor class -> (family, Rust type; a trait object for the abstract bases, the struct otherwise, concrete)
AD_INFO = {
    'Adaptor3d_Curve': ('ac3', 'dyn occt_port::adaptor3d::Curve3dAdaptor', False),
    'GeomAdaptor_Curve': ('ac3', 'occt_port::geomadaptor::Curve', True),
    'Adaptor2d_Curve2d': ('ac2', 'dyn occt_port::adaptor2d::Curve2dAdaptor', False),
    'Geom2dAdaptor_Curve': ('ac2', 'occt_port::geom2dadaptor::Curve', True),
    'Adaptor3d_Surface': ('as3', 'dyn occt_port::adaptor3d::SurfaceAdaptor', False),
    'GeomAdaptor_Surface': ('as3', 'occt_port::geomadaptor::Surface', True),
}

# Result structs (output only), also as the elements of NCollection arrays: C++ name -> (Rust type, [(C++ field, Rust field, word)])
STRUCTS = collections.OrderedDict()


def _structs():
    c3 = [('Point', 'point', 'pnt'), ('D1', 'd1', 'vec'), ('D2', 'd2', 'vec'), ('D3', 'd3', 'vec')]
    c2 = [('Point', 'point', 'pnt2d'), ('D1', 'd1', 'vec2d'), ('D2', 'd2', 'vec2d'), ('D3', 'd3', 'vec2d')]
    sf = [('Point', 'point', 'pnt'), ('D1U', 'd1u', 'vec'), ('D1V', 'd1v', 'vec'), ('D2U', 'd2u', 'vec'), ('D2V', 'd2v', 'vec'),
          ('D2UV', 'd2uv', 'vec'), ('D3U', 'd3u', 'vec'), ('D3V', 'd3v', 'vec'), ('D3UUV', 'd3uuv', 'vec'), ('D3UVV', 'd3uvv', 'vec')]
    for d, nf in ((1, 2), (2, 3), (3, 4)):
        STRUCTS['Geom_Curve::ResD%d' % d] = ('occt_port::geom::ResD%d' % d, c3[:nf])
        STRUCTS['Geom2d_Curve::ResD%d' % d] = ('occt_port::geom2d::ResD%d' % d, c2[:nf])
        STRUCTS['Geom2dGridEval::CurveD%d' % d] = ('occt_port::geom2dgrideval::CurveD%d' % d, c2[:nf])
    for d, nf in ((1, 3), (2, 6), (3, 10)):
        STRUCTS['Geom_Surface::ResD%d' % d] = ('occt_port::geom::SurfResD%d' % d, sf[:nf])


_structs()
STRUCT_ALIAS = {'GeomGridEval::CurveD%d' % d: 'Geom_Curve::ResD%d' % d for d in (1, 2, 3)}
STRUCT_ALIAS.update({'GeomGridEval::SurfD%d' % d: 'Geom_Surface::ResD%d' % d for d in (1, 2, 3)})


def struct_of(name):
    n = STRUCT_ALIAS.get(name, name)
    return n if n in STRUCTS else None

BX_RS = {'Bnd_Box': 'occt_port::bnd::BndBox', 'Bnd_Box2d': 'occt_port::bnd::BndBox2d'}
DOMAINS = {'MathUtils::Domain1D': ('occt_port::mathutils::Domain1D', ['min', 'max'], ['Min', 'Max']),
           'MathUtils::Domain2D': ('occt_port::mathutils::Domain2D', ['umin', 'umax', 'vmin', 'vmax'], ['UMin', 'UMax', 'VMin', 'VMax'])}
HANDLE_RX = re.compile(r'^(?:occ::|opencascade::)?handle<\s*([\w:]+)\s*>$|^Handle\((\w+)\)$')


# --------------------------------------------------------------------------------------------------------------
# clang AST streaming (as tools/cxx2rs.py, but tracking `includedFrom` correctly)

def run(cmd, **kw):
    return subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, **kw)


def stream_decls(path, pat):
    """Top-level declarations of a clang JSON dump whose file name (base name) matches `pat`."""
    file_re = re.compile(r'"file": "([^"]*)"')
    cur = start = None
    buf = None
    skip = in_loc = after_inc = False
    with open(path) as f:
        for line in f:
            if '"file"' in line:
                m = file_re.search(line)
                if m:
                    if after_inc:
                        after_inc = False
                    else:
                        cur = m.group(1)
                        if in_loc:
                            start = cur
            elif '"includedFrom"' in line:
                after_inc = True
            if buf is None:
                if line == '    {\n':
                    buf, start, in_loc, skip = [line], cur, False, False
                continue
            if not skip:
                buf.append(line)
            if line.startswith('      "loc"'):
                in_loc = True
            elif line.startswith('      "range"') and in_loc:
                in_loc = False
                if not (start and pat.search(start.rsplit('/', 1)[-1])):
                    skip, buf = True, []
            if line in ('    },\n', '    }\n'):
                if not skip:
                    yield json.loads(''.join(buf).rstrip().rstrip(','))
                buf = None


def comment_text(node):
    """All text of the FullComment children of a declaration."""
    out = []

    def walk(n):
        if isinstance(n, dict):
            if 'text' in n and isinstance(n['text'], str):
                out.append(n['text'])
            for c in n.get('inner', []):
                walk(c)
    for c in node.get('inner', []):
        if c.get('kind') == 'FullComment':
            walk(c)
    return ' '.join(out)


# --------------------------------------------------------------------------------------------------------------
# types

def split_type(qt):
    s = qt.strip()
    s = re.sub(r'\b(struct|class|enum|typename)\s+', '', s)
    t = {'ref': False, 'rref': False, 'ptr': False, 'const': False}
    if s.endswith('&&'):
        t['rref'] = True
        s = s[:-2].strip()
    elif s.endswith('&'):
        t['ref'] = True
        s = s[:-1].strip()
    if s.endswith('*'):
        t['ptr'] = True
        s = s[:-1].strip()
    if s.startswith('const '):
        t['const'] = True
        s = s[6:].strip()
    if s.endswith(' const'):
        t['const'] = True
        s = s[:-6].strip()
    t['base'] = s
    return t


class Ctx:
    """Package-wide knowledge needed to classify types."""

    def __init__(self, pkg, occt):
        self.pkg = pkg
        self.occt = occt
        self.enums = {}          # full name -> [constants] (None if not drivable)
        self.classes = {}        # full name -> Cls
        self.drivable = set()    # classes that can be built (fixpoint)
        self.used_enums = set()
        self.scope = None        # class whose members are being read (nested enums are written unqualified there)
        self.saw_handle = False  # some signature mentions a handle / adaptor that can be driven
        self.used_structs = set()   # result structs that a signature returns
        self.saw_bx = False         # a signature takes a Bnd_Box / Bnd_Box2d
        self.structs = {}           # namespace-level plain structs of the package: name -> [(field, type)]
        self.alias_map = {}         # namespace-level `using A = B;` of the package: 'Pkg::A' -> 'B'
        self.used_ns = []           # package structs that a signature returns (in dependency order)
        self.saw_dom = False        # a signature takes a MathUtils::Domain1D / Domain2D

    def enum(self, name):
        if name in self.enums:
            return self.enums[name]
        res = None
        hdr = os.path.join(self.occt, 'include', 'opencascade', name + '.hxx')
        if re.match(r'^[A-Z][A-Za-z0-9]*_[A-Za-z0-9_]+$', name) and os.path.exists(hdr):
            txt = open(hdr, errors='replace').read()
            txt = re.sub(r'//[^\n]*', '', txt)
            txt = re.sub(r'/\*.*?\*/', '', txt, flags=re.S)
            m = re.search(r'enum\s+' + re.escape(name) + r'\s*\{([^}]*)\}', txt)
            if m and '=' not in m.group(1):
                res = [c.strip() for c in m.group(1).split(',') if c.strip()]
        self.enums[name] = res
        return res

    def field_kind(self, qt):
        k, why, info = self.classify(qt)
        return k

    def struct_printable(self, name, seen=()):
        """A namespace struct whose fields all are printable (scalars, enums, gp values, nested structs, arrays of structs)."""
        if name in seen:
            return False
        for fname, qt in self.structs[name]:
            ma = re.match(r'^NCollection_(?:DynamicArray|Array1|Vector)<\s*([\w:]+)\s*>$', qt.strip())
            if ma:
                en = ma.group(1)
                if en not in self.structs and (pkg_q := self.pkg + '::' + en) in self.structs:
                    en = pkg_q
                if en in self.structs and self.struct_printable(en, seen + (name,)):
                    continue
                return False
            t = split_type(qt)
            if t['ptr'] or t['rref']:
                return False
            if t['base'] in self.structs:
                if not self.struct_printable(t['base'], seen + (name,)):
                    return False
                continue
            k = self.classify(qt)[0]
            if k is None or k[0] not in ('f', 'i', 'b', 'enum', 'gp'):
                return False
        return True

    def mark_ns(self, name):
        """Registers a printable struct and the structs it contains (contained ones first)."""
        for fname, qt in self.structs[name]:
            ma = re.match(r'^NCollection_(?:DynamicArray|Array1|Vector)<\s*([\w:]+)\s*>$', qt.strip())
            sub = ma.group(1) if ma else split_type(qt)['base']
            if sub not in self.structs and (self.pkg + '::' + sub) in self.structs:
                sub = self.pkg + '::' + sub
            if sub in self.structs and sub not in self.used_ns:
                self.mark_ns(sub)
        if name not in self.used_ns:
            self.used_ns.append(name)

    def classify(self, qt, desugared=None):
        """-> (kind tuple or None, reason). Kinds: ('f',) ('i',) ('b',) ('enum', name) ('gp', word)
        ('arr', elem) ('arr2', elem) ('cls', name) ('void',)."""
        t = split_type(qt)
        d = split_type(desugared) if desugared else t
        base, dbase = t['base'], d['base']
        for _ in range(3):
            if base in self.alias_map:
                base = split_type(self.alias_map[base])['base']
            if dbase in self.alias_map:
                dbase = split_type(self.alias_map[dbase])['base']
        info = dict(t)
        if t['rref']:
            return None, 'rvalue reference', info
        if t['ptr']:
            return None, 'pointer parameter', info
        if base == 'void':
            return ('void',), None, info
        for b in (base, dbase):
            if b in SCALARS:
                return (SCALARS[b],), None, info
        for b in (base, dbase):
            if b in GP_BY_CLASS:
                return ('gp', GP_BY_CLASS[b]), None, info
        for b in (base, dbase):
            m = re.match(r'^(?:TColgp|TColStd)_Array([12])Of(\w+)$', b)
            if m and m.group(2) in ARR_ELEM:
                e = ARR_ELEM[m.group(2)]
                if m.group(1) == '1' or e in ('f', 'pnt'):
                    return ('arr' if m.group(1) == '1' else 'arr2', e), None, info
            m = re.match(r'^NCollection_Array([12])<\s*([\w:]+)\s*>$', b)
            if m and m.group(2) in CPP_ELEM:
                e = CPP_ELEM[m.group(2)]
                if m.group(1) == '1' or e in ('f', 'pnt'):
                    return ('arr' if m.group(1) == '1' else 'arr2', e), None, info
        if base in self.classes or dbase in self.classes:
            n = base if base in self.classes else dbase
            return ('cls', n), None, info
        for b in (base, dbase):
            if b in DOMAINS:
                self.saw_dom = True
                return ('dom', b), None, info
        for b in (base, dbase):
            if b in self.structs and self.struct_printable(b):
                if b not in self.used_ns:
                    self.mark_ns(b)
                return ('ns', b), None, info
        for b in (base, dbase):
            if b in ('size_t', 'std::size_t', 'Standard_Size', 'unsigned long', 'unsigned long long'):
                return ('u',), None, info
        for b in (base, dbase):
            if b in ('Bnd_Box', 'Bnd_Box2d'):
                self.saw_bx = True
                return ('bx', b), None, info
        for b in (base, dbase):
            hm = HANDLE_RX.match(b)
            if hm:
                hn = hm.group(1) or hm.group(2)
                if hn in H_INFO:
                    self.saw_handle = True
                    info['handle'] = True
                    return ('h', hn), None, info
                if hn in AD_INFO:
                    self.saw_handle = True
                    info['handle'] = True
                    return ('ad', hn, True), None, info
            if b in AD_INFO and not (t['ptr']):
                self.saw_handle = True
                info['handle'] = False
                return ('ad', b, False), None, info
        for b in (base, dbase):
            if self.scope and (self.scope + '::' + b) in self.enums:
                b = self.scope + '::' + b
            if self.enum(b) is not None:
                self.used_enums.add(b)
                return ('enum', b), None, info
        for b in (base, dbase):
            sn = struct_of(b)
            if sn:
                self.used_structs.add(sn)
                self.saw_handle = self.saw_handle or not sn.startswith('Geom2dGridEval')
                return ('st', sn), None, info
            ma = re.match(r'^NCollection_Array([12])<\s*([\w:]+)\s*>$', b)
            if ma and struct_of(ma.group(2)):
                self.used_structs.add(struct_of(ma.group(2)))
                self.saw_handle = self.saw_handle or not struct_of(ma.group(2)).startswith('Geom2dGridEval')
                return ('sarr', struct_of(ma.group(2)), int(ma.group(1))), None, info
        if re.search(r'handle<|Handle\(', base):
            return None, 'handle type ' + base, info
        return None, 'unsupported type ' + base, info


# --------------------------------------------------------------------------------------------------------------
# model

class Param:
    def __init__(self, name, qt, desugared, ctx, idx):
        self.name = name or 'p%d' % idx
        self.qt = qt
        kind, why, info = ctx.classify(qt, desugared)
        self.kind, self.why, self.info = kind, why, info
        self.mode = 'in'
        self.mutref = bool(kind and kind[0] == 'bx' and not info['const'] and info['ref'])   # a Bnd_Box that the call fills
        if kind and kind[0] in ('st', 'sarr', 'u', 'ns'):
            self.kind, self.why = None, 'result struct / unsigned value as argument'
            kind = None
        if kind and not info['const'] and (info['ref']):
            if kind[0] == 'bx':
                pass
            elif kind[0] in ('f', 'i', 'b') or kind[0] == 'gp':
                self.mode = 'out'
            else:
                self.kind, self.why = None, 'non-const reference to ' + qt
        self.hint = param_hint(self.name, kind)

    @property
    def token(self):
        return kind_token(self.kind)


def param_hint(name, kind):
    n = name.lower()
    if kind and kind[0] == 'i':
        if re.search(r'degree|order|deriv|^n$|^the_?n$|^n[a-z]?$', n):
            return 'DEG'
        return 'IDX' if re.search(r'index|ind|^i$|^j$|nb|num|row|col|rank|mult|^k', n) else 'GEN'
    if kind and kind[0] == 'f':
        if re.search(r'tol|eps|prec|resolution|toler', n):
            return 'TOL'
        if re.search(r'ang|teta|theta|phi|rot', n):
            return 'ANG'
        if re.search(r'radius|focal|major|minor|scale|dist|length|size|height|width|semi|factor|^r$|^r[a-z]$', n):
            return 'POS'
        if re.search(r'^u|^v$|^t$|param|^the_?[uvt]|[uvt]$|abscissa', n):
            return 'PAR'
        if re.search(r'alpha|beta|ratio|weight', n):
            return 'UNIT'
    return 'GEN'


def kind_token(k):
    if k is None:
        return '?'
    t = k[0]
    if t == 'f':
        return 'real'
    if t == 'i':
        return 'int'
    if t == 'b':
        return 'bool'
    if t == 'void':
        return 'void'
    if t == 'enum':
        return snake(strip_pkg(k[1]))
    if t == 'gp':
        return k[1]
    if t in ('arr', 'arr2'):
        return t + '_' + k[1]
    if t == 'cls':
        return snake(strip_pkg(k[1]))
    if t in ('h', 'ad'):
        return snake(k[1])
    if t in ('st', 'sarr'):
        return snake(k[1].replace('::', '_'))
    if t in ('bx', 'dom'):
        return snake(k[1].replace('::', '_'))
    if t == 'ns':
        return snake(k[1].replace('::', '_'))
    if t == 'u':
        return 'usize'
    return '?'


def strip_pkg(n):
    if '::' in n:
        return n.split('::')[-1]
    return n.split('_', 1)[1] if '_' in n else n


class Fn:
    """One constructor, method, static method or free function."""

    def __init__(self, cls, node, ctx, kind):
        self.cls = cls                      # class record (Cls) or None
        self.kind = kind                    # 'ctor' | 'method' | 'static' | 'free'
        self.cname = node['name']
        qt = node['type']['qualType']
        i = qt.find('(')
        self.ret_qt = qt[:i].strip() if kind != 'ctor' else ''
        self.const = bool(re.search(r'\)\s*const\b', qt))
        self.virtual = bool(node.get('virtual'))
        self.pure = bool(node.get('pure'))
        self.raises = bool(re.search(r'\b[Rr]aises?\b|Standard_\w*(Error|Failure)\b|\b[Tt]hrows?\b', comment_text(node)))
        self.params = []
        for p in node.get('inner', []):
            if p.get('kind') == 'ParmVarDecl':
                t = p.get('type', {})
                self.params.append(Param(p.get('name'), t.get('qualType', '?'), t.get('desugaredQualType'), ctx,
                                         len(self.params)))
        if kind == 'ctor':
            self.ret = ('void',)
            self.ret_why = None
        else:
            r_kind, why, info = ctx.classify(self.ret_qt, node['type'].get('desugaredQualType', '').split('(')[0]
                                             if node['type'].get('desugaredQualType') else None)
            self.ret, self.ret_why = r_kind, why
            self.ret_info = info
            if r_kind and r_kind[0] == 'cls':
                self.ret, self.ret_why = None, 'returns an object of a package class'
        self.reason = None
        self.rust = None
        self.fam = None
        self.skip = False       # names.tsv value `-`: the reviewed API has no counterpart
        self.byval = set()      # indexes of parameters the reviewed Rust API takes by value although C++ takes a const reference
        self.family = None
        self.node_id = node.get('id')

    def sig_tokens(self):
        # an out-parameter (non-const reference) is marked with a trailing `&`
        return [p.token + ('&' if p.mode == 'out' else '') for p in self.params]

    def sig(self):
        return ','.join(self.sig_tokens())

    def undrivable(self):
        for p in self.params:
            if p.kind is None:
                return 'parameter %s: %s' % (p.name, p.why)
            if p.kind[0] == 'void':
                return 'void parameter'
        if self.ret is None:
            return 'return type: ' + str(self.ret_why)
        return None


class Cls:
    def __init__(self, node, pkg):
        self.node = node
        self.name = node['name']
        self.short = strip_pkg(self.name)
        self.is_struct = node.get('tagUsed') == 'struct'
        self.bases = [b['type']['qualType'] for b in node.get('bases', []) if b.get('access') == 'public']
        self.fns = []           # declared here
        self.abstract = False
        self.dtor_public = True
        self.skipped = []       # (text, reason)
        self.ctors = []
        self.in_pkg_name = (self.name == pkg)


EXTRA_INC = []     # -I<package source dir>: public headers may include the package's private .pxx files


def collect(pkg, headers_dir, occt, tmpdir):
    """Dump the AST of all headers of the package and build the model."""
    inc = os.path.join(occt, 'include', 'opencascade')
    hdrs = sorted(f for f in os.listdir(inc) if re.match(r'^' + re.escape(pkg) + r'(_[A-Za-z0-9_]+)?\.hxx$', f))
    if not hdrs:
        sys.exit('no headers for package ' + pkg)
    probe = os.path.join(tmpdir, 'probe.cxx')
    with open(probe, 'w') as f:
        for h in hdrs:
            f.write('#include <%s>\n' % h)
    jpath = os.path.join(tmpdir, 'ast.json')
    with open(jpath, 'w') as out:
        r = subprocess.run(['clang++', '-std=c++17', '-fsyntax-only', '-I' + inc] + EXTRA_INC + ['-Xclang', '-ast-dump=json', probe],
                           stdout=out, stderr=subprocess.PIPE, text=True)
    if os.path.getsize(jpath) < 1000:
        sys.exit('clang failed:\n' + r.stderr[-2000:])
    pat = re.compile(r'^' + re.escape(pkg) + r'(_[A-Za-z0-9_]+)?\.(hxx|lxx|pxx)$')
    decls = list(stream_decls(jpath, pat))
    os.remove(jpath)
    return hdrs, decls


class Model:
    def __init__(self, pkg, ctx):
        self.pkg = pkg
        self.ctx = ctx
        self.classes = collections.OrderedDict()   # name -> Cls
        self.enums = collections.OrderedDict()     # name -> [constants]
        self.free = []                              # free Fn
        self.skipped = []                           # (what, reason) not-API items we still list
        self.names = {}
        self.total_api = 0
        self.aliases = collections.OrderedDict()   # deprecated `using Pkg_X = Other_Y;` names
        self.structs = collections.OrderedDict()   # namespace-level plain structs: qualified name -> [(field, C++ type)]


def build_model(pkg, decls, ctx, headers):
    m = Model(pkg, ctx)
    recs = collections.OrderedDict()
    for d in decls:
        k = d.get('kind')
        n = d.get('name', '')
        if k == 'CXXRecordDecl' and d.get('completeDefinition') and n.startswith(pkg) and \
                (n == pkg or n.startswith(pkg + '_')):
            recs[n] = d
        elif k == 'NamespaceDecl' and n == pkg:
            for ch in d.get('inner', []):
                ck, cn = ch.get('kind'), ch.get('name', '')
                if ck == 'TypeAliasDecl' and cn:
                    ctx.alias_map[pkg + '::' + cn] = ch.get('type', {}).get('qualType', '')
                if ck == 'EnumDecl' and cn:
                    consts = [x['name'] for x in ch.get('inner', []) if x.get('kind') == 'EnumConstantDecl']
                    explicit = any(i.get('kind') in ('ConstantExpr', 'IntegerLiteral', 'BinaryOperator') for x in ch.get('inner', [])
                                   if x.get('kind') == 'EnumConstantDecl' for i in x.get('inner', []))
                    m.enums[pkg + '::' + cn] = consts
                    ctx.enums[pkg + '::' + cn] = None if explicit else consts
                elif ck == 'CXXRecordDecl' and cn and ch.get('completeDefinition') and ch.get('tagUsed') == 'struct':
                    flds = [(x['name'], x.get('type', {}).get('qualType', '?')) for x in ch.get('inner', [])
                            if x.get('kind') == 'FieldDecl' and x.get('name')]
                    if flds:
                        m.structs[pkg + '::' + cn] = flds
                        ctx.structs[pkg + '::' + cn] = flds
        elif k in ('TypeAliasDecl', 'TypedefDecl') and n.startswith(pkg + '_'):
            m.aliases[n] = d.get('type', {}).get('qualType', '?')
        elif k == 'EnumDecl' and n and (n.startswith(pkg + '_') or n == pkg):
            consts = [c['name'] for c in d.get('inner', []) if c.get('kind') == 'EnumConstantDecl']
            explicit = any(i.get('kind') in ('ConstantExpr', 'IntegerLiteral', 'BinaryOperator') for c in d.get('inner', [])
                           if c.get('kind') == 'EnumConstantDecl' for i in c.get('inner', []))
            m.enums[n] = consts
            ctx.enums[n] = None if explicit else consts
    for n, d in recs.items():
        ctx.classes[n] = Cls(d, pkg)
        acc = 'public' if d.get('tagUsed') == 'struct' else 'private'
        for ch in d.get('inner', []):
            if ch.get('kind') == 'AccessSpecDecl':
                acc = ch.get('access', acc)
            elif ch.get('kind') == 'EnumDecl' and ch.get('name') and acc == 'public':
                consts = [x['name'] for x in ch.get('inner', []) if x.get('kind') == 'EnumConstantDecl']
                explicit = any(i.get('kind') in ('ConstantExpr', 'IntegerLiteral', 'BinaryOperator')
                               for x in ch.get('inner', []) if x.get('kind') == 'EnumConstantDecl'
                               for i in x.get('inner', []))
                m.enums[n + '::' + ch['name']] = consts
                ctx.enums[n + '::' + ch['name']] = None if explicit else consts
    # a class named like a gp word is handled as a value word, not as an object
    for n, c in ctx.classes.items():
        if n in GP_BY_CLASS:
            c.is_gp = True
        else:
            c.is_gp = False
    for n, c in ctx.classes.items():
        ctx.scope = n
        d = c.node
        access = 'public' if c.is_struct else 'private'
        has_ctor = False
        for ch in d.get('inner', []):
            k = ch.get('kind')
            if k == 'AccessSpecDecl':
                access = ch.get('access', access)
                continue
            if k == 'EnumDecl':
                continue
            if k == 'CXXDestructorDecl':
                if access != 'public' and not ch.get('isImplicit'):
                    c.dtor_public = False
                continue
            if k not in ('CXXMethodDecl', 'CXXConstructorDecl'):
                continue
            if ch.get('isImplicit') or ch.get('explicitlyDeleted'):
                continue
            nm = ch.get('name', '')
            if nm in ('operator new', 'operator delete', 'operator new[]', 'operator delete[]'):
                continue
            if k == 'CXXConstructorDecl':
                has_ctor = True
                if access != 'public':
                    continue
                qt = ch['type']['qualType']
                ps = [p for p in ch.get('inner', []) if p.get('kind') == 'ParmVarDecl']
                if len(ps) >= 1 and re.match(r'^(const )?' + re.escape(n) + r' ?&&?$', ps[0]['type']['qualType']) \
                        and all(p.get('inner') for p in ps[1:]):
                    continue    # copy / move constructor
                fn = Fn(c, ch, ctx, 'ctor')
                c.ctors.append(fn)
                continue
            if access != 'public':
                continue
            if nm.startswith('operator') and nm != 'operator()':
                c.skipped.append((nm, 'operator (Rust traits are for the translator to choose)'))
                continue
            kind = 'static' if ch.get('storageClass') == 'static' else 'method'
            fn = Fn(c, ch, ctx, kind)
            if nm == 'operator()' and not (fn.ret and fn.ret[0] == 'u') and not any(
                    q.kind and q.kind[0] in ('h', 'ad') for q in fn.params):
                # a call operator is only a callable of the API for the hashers (a handle argument / a hash value);
                # element access like gp_Mat::operator()(row, col) stays a Rust trait of the translator's choice
                c.skipped.append((nm, 'operator (Rust traits are for the translator to choose)'))
                continue
            if fn.pure:
                c.abstract = True
            c.fns.append(fn)
        c.has_ctor = has_ctor
    ctx.scope = None
    # free functions
    for d in decls:
        if d.get('kind') == 'FunctionDecl' and d.get('name', '').startswith(('', )) and d.get('name') and \
                not d['name'].startswith('operator') and not d.get('isImplicit') and d.get('storageClass') != 'static':
            if d.get('previousDecl') and d.get('inner') and any(i.get('kind') == 'CompoundStmt' for i in d['inner']):
                continue
            fn = Fn(None, d, ctx, 'free')
            m.free.append(fn)
    return m


def finish_model(m):
    """Abstractness, inheritance, drivable classes, overload names."""
    ctx = m.ctx
    classes = ctx.classes
    # unresolved pure virtuals
    memo = {}

    def pure_set(c):
        if c.name in memo:
            return memo[c.name]
        memo[c.name] = set()
        s = set()
        for b in c.bases:
            bc = classes.get(split_type(b)['base'])
            if bc:
                s |= pure_set(bc)
        own = {(f.cname, f.sig()) for f in c.fns if not f.pure}
        s -= own
        s |= {(f.cname, f.sig()) for f in c.fns if f.pure}
        memo[c.name] = s
        return s
    for c in classes.values():
        c.abstract = bool(pure_set(c))
    # a class without any declared constructor has an implicit default one
    for c in classes.values():
        if not c.has_ctor and not c.abstract and not c.is_gp:
            c.implicit_default = True
        else:
            c.implicit_default = False
    # drivable classes: fixpoint over constructors
    drivable = set()
    changed = True
    while changed:
        changed = False
        for c in classes.values():
            if c.name in drivable or c.abstract or not c.dtor_public or c.is_gp:
                continue
            ok = c.implicit_default
            for f in c.ctors:
                if all(p.kind is not None and p.mode == 'in' and p.kind[0] != 'void' and
                       (p.kind[0] != 'cls' or p.kind[1] in drivable) for p in f.params):
                    ok = True
            if ok:
                drivable.add(c.name)
                changed = True
    ctx.drivable = drivable
    # which constructors of a drivable class can be used
    for c in classes.values():
        c.drv_ctors = []
        if c.name in drivable:
            if c.implicit_default:
                c.drv_ctors.append(None)
            for f in c.ctors:
                if all(p.kind is not None and p.mode == 'in' and p.kind[0] != 'void' and
                       (p.kind[0] != 'cls' or p.kind[1] in drivable) for p in f.params):
                    c.drv_ctors.append(f)
    # inheritance: methods of in-package bases are also callable on the derived class
    for c in classes.values():
        c.all_fns = list(c.fns)
        seen = {(f.cname, f.sig(), f.const, f.kind) for f in c.fns}

        def up(cc):
            for b in cc.bases:
                bc = classes.get(split_type(b)['base'])
                if bc:
                    for f in bc.fns:
                        key = (f.cname, f.sig(), f.const, f.kind)
                        if key not in seen:
                            seen.add(key)
                            c.all_fns.append(f)
                    up(bc)
        up(c)
    return m


# --------------------------------------------------------------------------------------------------------------
# names

def load_names(path):
    t = {}
    if path:
        for line in open(path):
            line = line.rstrip('\n')
            if not line.strip() or line.lstrip().startswith('#'):
                continue
            k, _, v = line.partition('\t')
            if v.strip():
                t[k.strip()] = v.strip()
    return t


def overload_names(group, base, names, keyfn):
    """group: list of Fn with the same class and C++ name -> {id(fn): rust name}."""
    res = {}
    by_arity = collections.defaultdict(list)
    for f in group:
        by_arity[len(f.params)].append(f)
    used = collections.Counter()
    for f in group:
        parts = []
        if len(group) > 1:
            if len(by_arity) > 1:
                parts.append(str(len(f.params)))
            peers = by_arity[len(f.params)]
            if len(peers) > 1:
                ins = [i for i in range(len(f.params)) if f.params[i].mode == 'in']
                alld = [i for i in range(len(f.params)) if len({p.params[i].token for p in peers}) > 1]
                diff = [i for i in alld if i in ins] or alld
                chosen = []
                for i in diff:
                    chosen.append(i)
                    if len({tuple(p.params[j].token for j in chosen) for p in peers}) == len(peers):
                        break
                if chosen:
                    parts.append('_'.join(f.params[i].token for i in chosen))
                else:
                    # identical parameter types: const / non-const pair
                    if not f.const and any(p.const for p in peers if p is not f):
                        parts.append('mut')
        n = base + ('_' + '_'.join(parts) if parts else '')
        res[id(f)] = n
    # explicit names (several overloads may deliberately share one Rust name: a generic or trait based API)
    explicit = set()
    for f in group:
        k1 = keyfn(f) + '(' + f.sig() + ')'
        k2 = keyfn(f) + '/%d' % len(f.params)
        if k1 in names:
            res[id(f)] = names[k1]
            explicit.add(id(f))
        elif k2 in names:
            res[id(f)] = names[k2]
            explicit.add(id(f))
    # clashes between rule-made names
    cnt = collections.Counter(res.values())
    seen = collections.Counter()
    for f in group:
        n = res[id(f)]
        if cnt[n] > 1 and id(f) not in explicit:
            seen[n] += 1
            if seen[n] > 1:
                res[id(f)] = n + '_' + chr(ord('a') + seen[n] - 1)
    return res


def base_name(cname):
    """Rust base name of a C++ method: snake case; a call operator is `call` (a hasher's hash and equality are `call_1` / `call_2`)."""
    return 'call' if cname == 'operator()' else snake(cname)


def assign_names(m, names):
    """Rust names of every constructor / method / function: f.rust (with names.tsv) and f.fam (the default rule, used
    for the family names so that they do not depend on names.tsv)."""
    pkg = m.pkg
    ctx = m.ctx

    def do(grp, base, keyfn):
        r0 = overload_names(grp, base, {}, keyfn)
        r1 = overload_names(grp, base, names, keyfn)
        for f in grp:
            f.fam = rust_ident(r0[id(f)])
            n = r1[id(f)]
            if '@' in n and not n.startswith('='):
                n, _, spec = n.partition('@')
                f.byval = {int(x) for x in spec.split(',') if x.strip().isdigit()}
            f.skip = n == '-'
            f.rust = n if n.startswith(('=', '-')) else rust_ident(n)
    for c in ctx.classes.values():
        groups = collections.defaultdict(list)
        for f in c.ctors:
            groups[('ctor',)].append(f)
        for f in c.fns:
            groups[(f.cname, f.kind == 'static')].append(f)
        for key, grp in groups.items():
            if key == ('ctor',):
                do(grp, 'new', lambda f, c=c: '%s::%s' % (c.name, c.name))
            else:
                do(grp, rust_ident(base_name(grp[0].cname)), lambda f, c=c: '%s::%s' % (c.name, f.cname))
    groups = collections.defaultdict(list)
    for f in m.free:
        groups[f.cname].append(f)
    for key, grp in groups.items():
        do(grp, rust_ident(snake(key)), lambda f: '%s::%s' % (pkg, f.cname))
    # statics of the class named like the package are module functions; they use the package key in names.tsv
    pc = ctx.classes.get(pkg)
    if pc is not None:
        groups = collections.defaultdict(list)
        for f in pc.fns:
            if f.kind == 'static':
                groups[f.cname].append(f)
        for key, grp in groups.items():
            do(grp, rust_ident(snake(key)), lambda f: '%s::%s' % (pkg, f.cname))


def enum_short(e, names):
    if 'enum ' + e in names:
        return names['enum ' + e]
    return e.split('::')[-1] if '::' in e else strip_pkg(e)


def enum_variant(m, e, const, names):
    k = '%s::%s' % (e, const)
    if k in names:
        return names[k]
    n = const
    short = e.split('::')[-1]
    for pre in (short.split('_')[0] + '_', e.split('_')[0] + '_'):
        if n.startswith(pre) and len(n) > len(pre):
            n = n[len(pre):]
            break
    return camel(n) if '_' in n or n.islower() else n[:1].upper() + n[1:]


# --------------------------------------------------------------------------------------------------------------
# planning: which families are generated

SNAPSHOT_SKIP = {'HashCode', 'DynamicType', 'This', 'GetRefCount', 'IsInstance', 'IsKind', 'Delete', 'Dump',
                 'DumpJson', 'InitFromJson', 'IsEqual', 'IsDifferent'}


def word_of(k):
    t = k[0]
    if t in ('f', 'i', 'b'):
        return t
    if t == 'enum':
        return 'e_' + k[1]
    if t == 'gp':
        return k[1]
    if t in ('arr', 'arr2'):
        return t + '_' + k[1]
    if t == 'cls':
        return 'obj_' + k[1]
    if t == 'h':
        return 'h_' + k[1]
    if t == 'ad':
        return 'ad_' + k[1]
    if t == 'bx':
        return 'bx_' + k[1]
    if t == 'dom':
        return 'dom_' + k[1].replace('::', '_')
    raise ValueError(k)


class Plan:
    def __init__(self, name, fn, kind):
        self.name = name
        self.fn = fn
        self.kind = kind            # 'ctor_gp' | 'ctor_obj' | 'call'
        self.recv = None            # None | ('gp', word) | ('cls', Cls)
        self.cls = None             # declaring class
        self.getters = []
        self.mutates = False
        self.reason = None
        self.lines = 0
        self.throws = 0
        self.libm = False
        self.words = []             # schema words in order
        self.can_throw = False
        self.cands = []             # indexes of non-const-reference scalar parameters (out or in-out, decided by probing)
        self.inout = set()


def plan_words(plan):
    f = plan.fn
    ws = []
    if plan.kind == 'ctor_obj':
        return ['obj_' + plan.cls.name]
    if plan.recv:
        ws.append(word_of_recv(plan.recv))
    for j, p in enumerate(f.params if f else []):
        if p.mode == 'in' or j in plan.inout:
            ws.append(word_of(p.kind))
    return ws


def snapshot_getters(c):
    out = []
    seen = set()
    for f in c.all_fns:
        if f.kind != 'method' or not f.const or f.params or f.cname in SNAPSHOT_SKIP or f.cname in seen:
            continue
        if f.ret is None or f.ret[0] in ('void', 'cls') or f.undrivable():
            continue
        if f.ret[0] in ('arr2',):
            continue
        seen.add(f.cname)
        out.append(f)
    return out[:24]


def fam_name(m, f, c):
    base = f.fam
    if f.kind == 'free' or c is None or c.name == m.pkg or (f.kind == 'static' and c.name == m.pkg):
        return base
    return snake(c.short) + '_' + base


def plan_all(m):
    """-> (plans, notgen) where notgen is a list of (description, reason, fn or None)."""
    ctx = m.ctx
    plans, notgen = [], []
    names_used = set()

    def uniq(n):
        k, i = n, 2
        while n in names_used:
            n = '%s_%d' % (k, i)
            i += 1
        names_used.add(n)
        return n

    def check_params(f):
        if f.skip:
            return 'no counterpart in the reviewed Rust API (names.tsv maps it to `-`)'
        r = f.undrivable()
        if r:
            return r
        for p in f.params:
            if p.kind[0] == 'cls' and p.kind[1] not in ctx.drivable:
                return 'parameter %s: class %s has no constructor that can be driven' % (p.name, p.kind[1])
            if p.mode == 'out' and p.kind[0] == 'cls':
                return 'out parameter of class type'
        return None

    def describe(f, c):
        ps = ', '.join(p.qt + ' ' + p.name for p in f.params)
        pre = (c.name + '::' if c else '')
        if f.kind == 'ctor':
            return '%s%s(%s)' % (pre, c.name, ps)
        return '%s %s%s(%s)%s' % (f.ret_qt, pre, f.cname, ps, ' const' if f.const else '')

    for c in ctx.classes.values():
        # a class with only static members is a namespace: no constructor family
        if not c.is_gp and not any(f.kind == 'method' for f in c.all_fns):
            c.namespace_like = True
        else:
            c.namespace_like = False
        # constructors
        if c.namespace_like:
            pass
        elif c.is_gp:
            for f in c.ctors:
                r = check_params(f)
                if r:
                    notgen.append((describe(f, c), r, f))
                    continue
                p = Plan(uniq(snake(c.short) + '_' + f.fam), f, 'ctor_gp')
                p.cls, p.recv = c, None
                p.words = [word_of(q.kind) for q in f.params if q.mode == 'in']
                plans.append(p)
            if not c.ctors:
                pass
        else:
            if c.name in ctx.drivable:
                p = Plan(uniq(snake(c.short) + '_new'), None, 'ctor_obj')
                p.cls = c
                p.recv = ('cls', c)
                p.words = ['obj_' + c.name]
                p.getters = snapshot_getters(c)
                plans.append(p)
                for f in c.ctors:
                    if f not in c.drv_ctors:
                        notgen.append((describe(f, c), check_params(f) or 'constructor not drivable', f))
                    else:
                        f.covered_by = p
            else:
                for f in c.ctors:
                    notgen.append((describe(f, c), 'class cannot be built (%s)' % (
                        'abstract' if c.abstract else 'no constructor that can be driven'), f))
        # methods
        for f in c.fns:
            r = check_params(f)
            if r:
                notgen.append((describe(f, c), r, f))
                continue
            if f.kind == 'static':
                p = Plan(uniq(fam_name(m, f, c)), f, 'call')
                p.cls = c
            else:
                recvc = c
                if c.is_gp:
                    recv = ('gp', GP_BY_CLASS[c.name])
                else:
                    if c.name not in ctx.drivable:
                        recvc = None
                        for c2 in ctx.classes.values():
                            if c2.name in ctx.drivable and any(g is f for g in c2.all_fns):
                                recvc = c2
                                break
                    if recvc is None:
                        notgen.append((describe(f, c), 'no instantiable class carries this method', f))
                        continue
                    recv = ('cls', recvc)
                p = Plan(uniq(fam_name(m, f, c)), f, 'call')
                p.cls = c
                p.recv = recv
                p.mutates = not f.const
                if recv[0] == 'cls' and not f.const:
                    p.getters = snapshot_getters(recv[1])
            p.words = ([word_of_recv(p.recv)] if p.recv else []) + [word_of(q.kind) for q in f.params if q.mode == 'in']
            plans.append(p)
        for txt, why in c.skipped:
            notgen.append((c.name + '::' + txt, why, None))
    for f in m.free:
        r = check_params(f)
        if r:
            notgen.append((describe(f, None), r, f))
            continue
        p = Plan(uniq(fam_name(m, f, None)), f, 'call')
        p.words = [word_of(q.kind) for q in f.params if q.mode == 'in']
        plans.append(p)
    for p in plans:
        if p.fn is not None and p.kind == 'call':
            p.cands = [j for j, q in enumerate(p.fn.params) if q.mode == 'out' and q.kind[0] in ('f', 'i', 'b')]
    for e, consts in m.enums.items():
        if ctx.enums.get(e) is None:
            notgen.append(('enum ' + e, 'explicit enumerator values (ordinals would not match)', None))
    return plans, notgen


def word_of_recv(recv):
    if recv[0] == 'gp':
        return recv[1]
    return 'obj_' + recv[1].name


# --------------------------------------------------------------------------------------------------------------
# C++ prelude of the generated oracle (harness as in oracle/convert.cpp + generic value generators)

CPP_PRELUDE = r'''// GENERATED by tools/judgegen.py -- reference calls of OCCT 8.0.1 for the package @@PKG@@. Do not edit by hand.
// Output: one record per line, `<family> <inputs...> | <outputs...>`; floats are 16-hex-digit f64 bit patterns, ints and
// bools decimal, an enum is its ordinal. Every input is printed on the left of ` | ` in the order of the family's schema
// (`begin(name, schema)`; the oracle aborts if the printed words differ from the schema). Words: f i b (scalars),
// e_<Enum> (ordinal), the gp words (pnt vec dir xyz ax1 ax2 ax3 lin circ elips hypr parab pln cylinder cone sphere torus mat
// trsf quat pnt2d vec2d dir2d xy ax2d ax22d lin2d circ2d elips2d hypr2d parab2d mat2d trsf2d; layouts in the test), arr_<e> /
// arr2_<e> (count / rows cols then the elements) and obj_<Class> (constructor index, then its arguments). A call that
// raises Standard_Failure prints `throws`. Every call is run 6 times with differently filled stack and heap (a case whose
// results differ, that is not finite or that crashes in a child process is dropped); a case that calls the C library's
// transcendental functions is kept only when moving every such result by an ulp changes the outputs by at most 2 ulps
// (the test allows 8); a family none of whose cases calls them is compared bit for bit.
// @@BUILDNOTE@@
#include <algorithm>
#include <array>
#include <cfloat>
#include <climits>
#include <cmath>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <map>
#include <memory>
#include <new>
#include <utility>
#include <set>
#include <string>
#include <vector>
#include <malloc.h>
#include <unistd.h>
#include <sys/wait.h>
#include <dlfcn.h>
#include <sys/personality.h>
@@INCLUDES@@
#include <NCollection_Array1.hxx>
#include <NCollection_Array2.hxx>
#include <Standard_Failure.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Ax22d.hxx>
#include <gp_Ax2d.hxx>
#include <gp_Ax3.hxx>
#include <gp_Circ.hxx>
#include <gp_Circ2d.hxx>
#include <gp_Cone.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Dir.hxx>
#include <gp_Dir2d.hxx>
#include <gp_Elips.hxx>
#include <gp_Elips2d.hxx>
#include <gp_Hypr.hxx>
#include <gp_Hypr2d.hxx>
#include <gp_Lin.hxx>
#include <gp_Lin2d.hxx>
#include <gp_Mat.hxx>
#include <gp_Mat2d.hxx>
#include <gp_Parab.hxx>
#include <gp_Parab2d.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Quaternion.hxx>
#include <gp_Sphere.hxx>
#include <gp_Torus.hxx>
#include <gp_Trsf.hxx>
#include <gp_Trsf2d.hxx>
#include <gp_Vec.hxx>
#include <gp_Vec2d.hxx>
#include <gp_XY.hxx>
#include <gp_XYZ.hxx>
typedef std::vector<double> Vd;

// ---------------------------------------------------------------------------------
// deterministic random numbers (fixed seed)
static uint64_t state = 0x9E3779B97F4A7C15ull;
static double rnd(double lo, double hi) {
  state ^= state << 13; state ^= state >> 7; state ^= state << 17;
  return lo + (hi - lo) * (double)(state >> 11) / 9007199254740992.0;
}
static int rint(int lo, int hi) { int v = lo + (int)std::floor(rnd(0, 1) * (hi - lo + 1)); return v > hi ? hi : v; }
static bool coin(double p) { return rnd(0, 1) < p; }
template <typename T, size_t N> static T pick(const T (&a)[N]) { return a[rint(0, (int)N - 1)]; }

struct Discard {};
static int g_run_k = 0;
static bool g_call_phase = false, g_call_threw = false;   // a Standard_Failure raised by the call itself (not by building an input)
// every family starts from its own seed, so families do not depend on each other or on which ones are skipped
static void seed_for(const char* n) {
  uint64_t h = 0xcbf29ce484222325ull;
  for (; *n; ++n) { h ^= (unsigned char)*n; h *= 0x100000001b3ull; }
  state = h ? h : 1;
  for (int i = 0; i < 8; ++i) rnd(0, 1);
}

// ---------------------------------------------------------------------------------
// record buffer
static std::string buf, cur_name, cur_schema;
static size_t mark = 0;
static int ntok = 0;
static bool outphase = false;
static std::vector<std::string> got_words;
static std::set<std::string> seen;
static std::map<std::string, int> fam_lines, fam_throws, fam_dropped;
static std::map<std::string, bool> fam_libm;

// A heap / stack / code address that reaches the output differs from run to run (ASLR): such a case is dropped. An output is
// pointer-like when an integer does not fit 32 bits or a double is a denormal with a mantissa of more than 32 bits.
static bool pointer_like(double v) {
  uint64_t bits; std::memcpy(&bits, &v, 8);
  return ((bits >> 52) & 0x7ff) == 0 && (bits & 0xfffffffffffffull) >= (1ull << 32);
}
static void hx(double v) {
  uint64_t bits; std::memcpy(&bits, &v, 8); char t[24]; std::snprintf(t, sizeof t, " %016llx", (unsigned long long)bits);
  buf += t; ++ntok;
}
static void h(double v) {
  if (!std::isfinite(v)) {
    if (!outphase) { std::fprintf(stderr, "non-finite input in %s: %s\n", cur_name.c_str(), buf.c_str()); std::abort(); }
    throw Discard();
  }
  if (outphase && pointer_like(v)) throw Discard();
  hx(v);
}
static void in(long v) {
  if (outphase && (v > 0x7fffffffL || v < -0x80000000L)) throw Discard();
  buf += " " + std::to_string(v); ++ntok;
}
static void ob(bool v) { in(v ? 1 : 0); }
static void ox() { buf += " x"; ++ntok; }   // a getter of an object's snapshot that raised
static int g_nested = 0;   // > 0 while the sub-words of an obj_<Class> word are printed (only the obj word is recorded)
static void word(const std::string& w, int start, int expect) {
  if (g_nested) return;
  got_words.push_back(w);
  if (expect >= 0 && ntok - start != expect) {
    std::fprintf(stderr, "word %s printed %d tokens, expected %d in %s\n", w.c_str(), ntok - start, expect, cur_name.c_str());
    std::abort();
  }
}
static std::vector<std::string> split_words(const std::string& s) {
  std::vector<std::string> out; size_t pos = 0;
  while (pos < s.size()) {
    size_t e = s.find(' ', pos); if (e == std::string::npos) e = s.size();
    if (e > pos) out.push_back(s.substr(pos, e - pos));
    pos = e + 1;
  }
  return out;
}
static void begin(const std::string& name, const std::string& schema) {
  buf = name; cur_name = name; cur_schema = schema; ntok = 0; outphase = false; got_words.clear();
}
static void bar() {
  if (got_words != split_words(cur_schema)) {
    std::fprintf(stderr, "schema mismatch in %s: printed words differ from '%s'\n", cur_name.c_str(), cur_schema.c_str());
    std::abort();
  }
  buf += " |"; mark = buf.size(); outphase = true;
}
static void finish() {
  if (!seen.insert(buf).second) { buf.clear(); outphase = false; return; }   // exact duplicate: dropped
  fam_lines[cur_name]++;
  if (g_call_threw && buf.size() >= 8 && buf.compare(buf.size() - 7, 7, " throws") == 0) fam_throws[cur_name]++;
  buf += "\n"; std::fputs(buf.c_str(), stdout); buf.clear(); outphase = false;
}

// ---------------------------------------------------------------------------------
// The C library's transcendental functions, interposed so that a case can be re-run with every result moved by
// 0 or 1 ulp (a different libm gives such differences). With g_perturb off the functions are the C library's;
// g_libm counts the calls (a family none of whose cases calls them is compared bit for bit).
static bool g_perturb = false;
static uint64_t g_salt = 0;
static unsigned long g_libm = 0;
static double pert(double r, double x, uint64_t k) {
  ++g_libm;
  if (!g_perturb || !std::isfinite(r)) return r;
  uint64_t b; std::memcpy(&b, &x, 8);
  uint64_t z = (b ^ (g_salt * 0x9E3779B97F4A7C15ull) ^ (k * 0xD1B54A32D192ED03ull)) * 0xBF58476D1CE4E5B9ull;
  z ^= z >> 31; z *= 0x94D049BB133111EBull; z ^= z >> 29;
  switch (z & 3) {
    case 0: return std::nextafter(r, HUGE_VAL);
    case 1: return std::nextafter(r, -HUGE_VAL);
    default: return r;
  }
}
#define REAL1(name) static double (*real_##name)(double) = nullptr; if (!real_##name) real_##name = (double (*)(double))dlsym(RTLD_NEXT, #name)
#define REAL2(name) static double (*real_##name)(double, double) = nullptr; if (!real_##name) real_##name = (double (*)(double, double))dlsym(RTLD_NEXT, #name)
#define F1(name, k) double name(double x) noexcept { REAL1(name); return pert(real_##name(x), x, k); }
extern "C" {
F1(sin, 1) F1(cos, 2) F1(tan, 3) F1(asin, 4) F1(acos, 5) F1(atan, 6) F1(sinh, 9) F1(cosh, 10) F1(tanh, 11)
F1(asinh, 12) F1(acosh, 13) F1(atanh, 14) F1(exp, 15) F1(exp2, 16) F1(expm1, 17) F1(log, 18) F1(log2, 19)
F1(log10, 20) F1(log1p, 21) F1(cbrt, 22)
double atan2(double y, double x) noexcept { REAL2(atan2); return pert(real_atan2(y, x), y + 3.0 * x, 7); }
double pow(double x, double y) noexcept { REAL2(pow); return pert(real_pow(x, y), x + 5.0 * y, 8); }
double hypot(double x, double y) noexcept { REAL2(hypot); return pert(real_hypot(x, y), x + 7.0 * y, 23); }
void sincos(double x, double* s, double* c) noexcept {
  REAL1(sin); static double (*real_cos2)(double) = nullptr; if (!real_cos2) real_cos2 = (double (*)(double))dlsym(RTLD_NEXT, "cos");
  *s = pert(real_sin(x), x, 1); *c = pert(real_cos2(x), x, 2);
}
}

// ---------------------------------------------------------------------------------
// run a call 6 times with differently filled stack and heap, keep the line when all runs agree. The stack below the
// caller is filled with pseudo-random words (different for every run), so that a variable that is read without being
// written gets a different value in every run.
__attribute__((noinline)) static void poison_stack(int k) {
  volatile uint64_t a[8192];
  uint64_t z = 0x9E3779B97F4A7C15ull * (uint64_t)(k + 1);
  for (size_t i = 0; i < 8192; ++i) {
    z += 0x9E3779B97F4A7C15ull;
    uint64_t r = z;
    r = (r ^ (r >> 30)) * 0xBF58476D1CE4E5B9ull;
    r = (r ^ (r >> 27)) * 0x94D049BB133111EBull;
    a[i] = k == 4 ? 0ull : (k == 5 ? ~0ull : (r ^ (r >> 31)));
  }
  asm volatile("" ::: "memory");
}
static const int POISON_RUNS = 6;
// Objects are built in heap blocks whose surroundings hold pseudo-random bytes that differ in every run: a call that reads
// outside the object (an index whose range check is compiled out of the library) gives results that differ between the
// runs, and the case is dropped.
template <class T> struct Padded {
  char* raw; T* p;
  explicit Padded(const T& v) {
    const size_t PAD = 256;
    static uint64_t counter = 0;
    raw = (char*)std::malloc(sizeof(T) + 2 * PAD + 16);
    uint64_t z = 0x9E3779B97F4A7C15ull * (uint64_t)(g_run_k + 1) + 0xD1B54A32D192ED03ull * (++counter);
    for (size_t i = 0; i < sizeof(T) + 2 * PAD; ++i) {
      z ^= z << 13; z ^= z >> 7; z ^= z << 17;
      raw[i] = g_run_k == 4 ? 0 : (g_run_k == 5 ? (char)0xFF : (char)(z >> 24));
    }
    p = new (raw + PAD) T(v);
  }
  ~Padded() { p->~T(); std::free(raw); }
  Padded(const Padded&) = delete;
  Padded& operator=(const Padded&) = delete;
  T& operator*() const { return *p; }
  T* operator->() const { return p; }
};
static std::string run_once(const std::function<void()>& body, int poison, bool& discard) {
  std::string left = buf.substr(0, mark);
  g_call_phase = false;
  g_run_k = poison;
  mallopt(M_PERTURB, poison == 4 ? 0x01 : (poison == 5 ? 0xFE : ((0x37 + 0x4B * poison) & 0xff)));
  poison_stack(poison);
  discard = false;
  try {
    body();
  } catch (Standard_Failure&) {
    if (g_call_phase) g_call_threw = true;
    buf.resize(mark); buf += " throws";
  } catch (Discard&) {
    discard = true;
  } catch (std::exception&) {
    discard = true;
  }
  std::string res = buf;
  buf = left;
  return res;
}
static bool is_float_token(const std::string& t) {
  if (t.size() != 16) return false;
  for (char c : t) if (!std::isxdigit((unsigned char)c)) return false;
  return true;
}
static double tok_double(const std::string& t) { uint64_t b = std::strtoull(t.c_str(), nullptr, 16); double d; std::memcpy(&d, &b, 8); return d; }
// the outputs of two lines agree within `ulps` units of eps times the largest absolute value (at least 1) of the line
static bool lines_close(const std::string& x, const std::string& y, double ulps) {
  size_t px = x.find(" |"), py = y.find(" |");
  if (px == std::string::npos || py == std::string::npos || x.substr(0, px) != y.substr(0, py)) return false;
  std::vector<std::string> a = split_words(x.substr(px + 2)), b = split_words(y.substr(py + 2));
  if (a.size() != b.size()) return false;
  double scale = 1.0;
  for (size_t i = 0; i < a.size(); ++i) if (is_float_token(a[i])) scale = std::max(scale, std::fabs(tok_double(a[i])));
  for (size_t i = 0; i < a.size(); ++i) {
    if (is_float_token(a[i]) != is_float_token(b[i])) return false;
    if (is_float_token(a[i])) {
      double u = tok_double(a[i]), v = tok_double(b[i]);
      if (std::isfinite(scale) ? std::fabs(u - v) > ulps * DBL_EPSILON * scale : u != v) return false;
    } else if (a[i] != b[i]) return false;
  }
  return true;
}
// Everything that executes the library runs in a forked child: a call that reads uninitialised memory or writes outside its
// objects may crash (or corrupt the heap and crash later), and must not take the oracle with it. The child sends the final
// record (and two flags) through a pipe; a child that crashes, hangs or exits abnormally gives a dropped case.
static bool emit_core(const std::function<void()>& body, std::string& r1, bool& libm, bool& call_threw) {
  size_t keep_ntok = (size_t)ntok;
  for (int k = 0; k < POISON_RUNS; ++k) {
    bool dk = false;
    g_call_threw = false;
    unsigned long c0 = g_libm;
    std::string rk = run_once(body, k, dk);
    if (k == 0) { libm = g_libm != c0; call_threw = g_call_threw; }
    ntok = (int)keep_ntok;
    if (dk) return false;
    if (k == 0) r1 = rk;
    else if (rk != r1) return false;
  }
  if (libm) {
    for (uint64_t salt = 1; salt <= 24; ++salt) {
      g_perturb = true; g_salt = salt;
      bool dp = false;
      std::string rp = run_once(body, 0, dp);
      g_perturb = false;
      ntok = (int)keep_ntok;
      if (dp || !lines_close(r1, rp, 2.0)) return false;
    }
  }
  return true;
}
static bool write_all(int fd, const std::string& msg) {
  size_t off = 0;
  while (off < msg.size()) { ssize_t n = write(fd, msg.data() + off, msg.size() - off); if (n <= 0) return false; off += (size_t)n; }
  return true;
}
static std::map<std::string, int> fam_hangs;
static bool emit(const std::function<void()>& body) {
  fflush(stdout); fflush(stderr);
  int fd[2];
  if (pipe(fd)) { std::fprintf(stderr, "pipe failed\n"); std::abort(); }
  pid_t p = fork();
  if (p == 0) {
    close(fd[0]);
    signal(SIGSEGV, SIG_DFL); signal(SIGABRT, SIG_DFL); signal(SIGFPE, SIG_DFL); signal(SIGBUS, SIG_DFL);
    alarm(20);
    std::string r1; bool libm = false, threw = false;
    bool ok = emit_core(body, r1, libm, threw);
    std::string msg = std::string(ok ? "1" : "0") + (libm ? "1" : "0") + (threw ? "1" : "0") + r1;
    _exit(write_all(fd[1], msg) ? 0 : 3);
  }
  close(fd[1]);
  std::string msg; char tmp[65536]; ssize_t n;
  while ((n = read(fd[0], tmp, sizeof tmp)) > 0) msg.append(tmp, (size_t)n);
  close(fd[0]);
  int st = 0; waitpid(p, &st, 0);
  if (WIFSIGNALED(st) && WTERMSIG(st) == SIGALRM) fam_hangs[cur_name]++;   // a call that does not return: the family gives up after a few
  if (!(WIFEXITED(st) && WEXITSTATUS(st) == 0) || msg.size() < 3 || msg[0] != '1') { fam_dropped[cur_name]++; buf.clear(); return false; }
  if (msg[1] == '1') fam_libm[cur_name] = true;
  g_call_threw = msg[2] == '1';
  buf = msg.substr(3);
  finish();
  return true;
}

// =================================================================================
// raw inputs: what is printed on the left of ` | ` and what the call is rebuilt from
struct CV; struct SV;
struct Raw { std::string w, pat; Vd v; std::shared_ptr<CV> cv; std::shared_ptr<SV> sv; };
struct RB {
  std::string pat; Vd v;
  void f(double x) { pat += 'f'; v.push_back(x); }
  void i(long x) { pat += 'i'; v.push_back((double)x); }
};
struct Bag {
  std::vector<Raw> r;
  const Raw* last(const std::string& w) const { for (size_t i = r.size(); i-- > 0;) if (r[i].w == w) return &r[i]; return nullptr; }
};
struct Cursor {
  const Bag& b; size_t i = 0;
  explicit Cursor(const Bag& bag) : b(bag) {}
  const Raw& next() { return b.r.at(i++); }
};
static bool variable_word(const std::string& w) { return w == "trsf" || w == "trsf2d" || w.compare(0, 3, "arr") == 0 || w.compare(0, 4, "obj_") == 0 || w.compare(0, 2, "h_") == 0 || w.compare(0, 3, "ad_") == 0; }
static void pr(const Raw& r) {   // print the raw tokens of one schema word
  int s = ntok;
  for (size_t k = 0; k < r.pat.size(); ++k) { if (r.pat[k] == 'f') h(r.v[k]); else in((long)r.v[k]); }
  if (!g_nested) word(r.w, s, variable_word(r.w) ? -1 : (int)r.pat.size());
}
static void put(Bag& b, const Raw& r) { pr(r); b.r.push_back(r); }
static std::vector<std::string> fam_order;
static void reg(const char* n) { if (std::find(fam_order.begin(), fam_order.end(), n) == fam_order.end()) fam_order.push_back(n); }

static int g_att = 0, g_cases = @@CASES@@;
static bool edge() { return g_att % 7 == 0; }

// scalar hints
enum { H_GEN, H_TOL, H_ANG, H_POS, H_PAR, H_IDX, H_DEG, H_UNIT };
static double nice() { return 0.5 * rint(-8, 8); }
static double crd() { return coin(0.4) ? nice() : rnd(-6, 6); }
static double gf(int hint) {
  static const double EG[] = {0.0, 1.0, -1.0, 1e-12, -1e-9, 1e9, 0.5, M_PI, -M_PI, -0.0};
  static const double ET[] = {0.0, 1e-12, 1e-7, 1e-3, 1.0, 0.1};
  static const double EA[] = {0.0, M_PI / 2, M_PI, 2 * M_PI, -M_PI / 2, 1e-9, -1.0, 3 * M_PI};
  static const double EP[] = {0.0, 1e-9, 1.0, 1e6, 0.5, 2.0, -1.0};
  static const double EU[] = {0.0, 1.0, 0.5, 1e-9, 2.0, -0.5};
  double r = rnd(0, 1);
  switch (hint) {
    case H_TOL:
      if (edge() || r < 0.5) return pick(ET);
      return r < 0.7 ? 1e-7 : rnd(0, 0.5);
    case H_ANG:
      if (edge()) return pick(EA);
      if (r < 0.25) return (M_PI / 4) * rint(-8, 8);
      if (r < 0.32) return rnd(-1e-6, 1e-6);
      return rnd(-7, 7);
    case H_POS:
      if (edge()) return pick(EP);
      if (r < 0.3) return 0.5 * rint(1, 12);
      if (r < 0.8) return rnd(0.05, 8);
      if (r < 0.86) return rnd(1e-6, 1e-3);
      if (r < 0.92) return rnd(1e3, 1e5);
      return -rnd(0.1, 3);
    case H_PAR:
      if (edge()) return pick(EA);
      if (r < 0.3) return nice();
      if (r < 0.4) return (M_PI / 4) * rint(-8, 8);
      return rnd(-6, 6);
    case H_UNIT:
      if (edge()) return pick(EU);
      return r < 0.3 ? 0.25 * rint(0, 4) : rnd(0, 1);
    default:
      if (edge()) return pick(EG);
      if (r < 0.08) return 0.0;
      if (r < 0.16) return coin(0.5) ? 1.0 : -1.0;
      if (r < 0.22) return (coin(0.5) ? 1 : -1) * rnd(1e-12, 1e-8);
      if (r < 0.27) return (coin(0.5) ? 1 : -1) * rnd(1e6, 1e9);
      if (r < 0.55) return nice();
      return rnd(-8, 8);
  }
}
static long gi(int hint) {
  switch (hint) {
    case H_DEG: { static const long D[] = {0, 1, 2, 3, 4, 5, 6, -1, 7}; return edge() ? pick(D) : rint(0, 6); }
    case H_IDX: { static const long D[] = {-1, 0, 1, 2, 3, 4, 5, 6, 10}; return edge() ? pick(D) : rint(0, 4); }
    default: { static const long D[] = {-2, -1, 0, 1, 2, 3, 4, 5, 8, 100}; return coin(0.6) ? rint(0, 5) : pick(D); }
  }
}
static Raw g_f(int hint) { Raw r{"f", "f", {}}; r.v.push_back(gf(hint)); return r; }
static Raw g_i(int hint) { Raw r{"i", "i", {}}; r.v.push_back((double)gi(hint)); return r; }
static Raw g_b() { Raw r{"b", "i", {}}; r.v.push_back(coin(0.5) ? 1 : 0); return r; }
static Raw g_enum(const std::string& w, int n) { Raw r{w, "i", {}}; r.v.push_back((double)rint(0, n - 1)); return r; }

// ----- coordinates, directions, frames (like = the previous value of the same word in the call, for coincident /
// parallel / opposite inputs)
static void ap_pnt3(RB& b, const double* like) {
  if (like && coin(0.15)) { for (int k = 0; k < 3; ++k) b.f(like[k]); return; }
  for (int k = 0; k < 3; ++k) b.f(edge() && coin(0.5) ? 0.0 : crd());
}
static void ap_pnt2(RB& b, const double* like) {
  if (like && coin(0.15)) { for (int k = 0; k < 2; ++k) b.f(like[k]); return; }
  for (int k = 0; k < 2; ++k) b.f(edge() && coin(0.5) ? 0.0 : crd());
}
static void ap_vec(RB& b, int n) {
  for (int k = 0; k < n; ++k) b.f(edge() && coin(0.4) ? 0.0 : (coin(0.2) ? gf(H_GEN) : crd()));
}
static void ap_dir(RB& b, int n, const double* like) {   // a raw (un-normalised) direction, nonzero except rarely
  if (like) {
    double r = rnd(0, 1);
    if (r < 0.10) { for (int k = 0; k < n; ++k) b.f(like[k]); return; }
    if (r < 0.17) { for (int k = 0; k < n; ++k) b.f(-like[k]); return; }
    if (r < 0.22) { for (int k = 0; k < n; ++k) b.f(2.5 * like[k]); return; }
  }
  if (coin(0.02)) { for (int k = 0; k < n; ++k) b.f(0.0); return; }
  for (;;) {
    double t[3] = {0, 0, 0}, nn = 0;
    double r = rnd(0, 1);
    for (int k = 0; k < n; ++k) {
      t[k] = r < 0.3 ? (double)rint(-1, 1) : (r < 0.55 ? (double)rint(-2, 2) : rnd(-3, 3));
      nn += t[k] * t[k];
    }
    if (nn > 0.04) { for (int k = 0; k < n; ++k) b.f(t[k]); return; }
  }
}
static bool near_par3(const double* a, const double* c) {
  double x = a[1] * c[2] - a[2] * c[1], y = a[2] * c[0] - a[0] * c[2], z = a[0] * c[1] - a[1] * c[0];
  double na = a[0] * a[0] + a[1] * a[1] + a[2] * a[2], nc = c[0] * c[0] + c[1] * c[1] + c[2] * c[2];
  return x * x + y * y + z * z < 1e-6 * na * nc;
}
static bool near_par2(const double* a, const double* c) { double x = a[0] * c[1] - a[1] * c[0]; return x * x < 1e-6 * (a[0] * a[0] + a[1] * a[1]) * (c[0] * c[0] + c[1] * c[1]); }
// p3, n3, vx3 (vx not parallel to n except in 3% of the cases)
static void ap_frame3(RB& b, const double* like) {
  ap_pnt3(b, like);
  ap_dir(b, 3, like ? like + 3 : nullptr);
  size_t o = b.v.size() - 3;
  for (int t = 0; t < 30; ++t) {
    RB tmp; ap_dir(tmp, 3, like ? like + 6 : nullptr);
    if (t == 29 || coin(0.03) || !near_par3(&b.v[o], tmp.v.data())) { for (double x : tmp.v) b.f(x); return; }
  }
}
// p2, vx2, vy2 (vy not parallel to vx except in 3% of the cases)
static void ap_frame2(RB& b, const double* like) {
  ap_pnt2(b, like);
  ap_dir(b, 2, like ? like + 2 : nullptr);
  size_t o = b.v.size() - 2;
  for (int t = 0; t < 30; ++t) {
    RB tmp; ap_dir(tmp, 2, like ? like + 4 : nullptr);
    if (t == 29 || coin(0.03) || !near_par2(&b.v[o], tmp.v.data())) { for (double x : tmp.v) b.f(x); return; }
  }
}
static void ap_ax2d(RB& b, const double* like) { ap_pnt2(b, like); ap_dir(b, 2, like ? like + 2 : nullptr); }
static void ap_ax1(RB& b, const double* like) { ap_pnt3(b, like); ap_dir(b, 3, like ? like + 3 : nullptr); }
static void ap_ax3(RB& b, const double* like) { ap_frame3(b, like); b.i(coin(0.7) ? 0 : rint(1, 2)); }
static double ap_rad(RB& b) { double r = gf(H_POS); if (r < 0 && coin(0.7)) r = -r; b.f(r); return r; }
static void ap_trsf(RB& b) {
  static const int K[] = {0, 2, 3, 4, 5, 6};
  int k = pick(K);
  b.i(k);
  switch (k) {
    case 0: ap_ax1(b, nullptr); b.f(gf(H_ANG)); break;
    case 2: ap_pnt3(b, nullptr); b.f(coin(0.5) ? rnd(0.2, 3) : -rnd(0.2, 3)); break;
    case 3: ap_vec(b, 3); break;
    case 4: ap_ax1(b, nullptr); break;
    case 5: ap_pnt3(b, nullptr); break;
    default: ap_pnt3(b, nullptr); ap_dir(b, 3, nullptr); break;
  }
}
static void ap_trsf2d(RB& b) {
  static const int K[] = {0, 2, 3, 5, 6};
  int k = pick(K);
  b.i(k);
  switch (k) {
    case 0: ap_pnt2(b, nullptr); b.f(gf(H_ANG)); break;
    case 2: ap_pnt2(b, nullptr); b.f(coin(0.5) ? rnd(0.2, 3) : -rnd(0.2, 3)); break;
    case 3: ap_vec(b, 2); break;
    case 5: ap_pnt2(b, nullptr); break;
    default: ap_ax2d(b, nullptr); break;
  }
}
static Raw g_gp(const std::string& w, const Raw* like) {
  RB b; const double* L = like ? like->v.data() : nullptr;
  if (w == "pnt") ap_pnt3(b, L);
  else if (w == "vec" || w == "xyz") ap_vec(b, 3);
  else if (w == "dir") ap_dir(b, 3, L);
  else if (w == "ax1" || w == "lin") ap_ax1(b, L);
  else if (w == "ax2") ap_frame3(b, L);
  else if (w == "ax3" || w == "pln") ap_ax3(b, L);
  else if (w == "circ") { ap_frame3(b, L); ap_rad(b); }
  else if (w == "elips" || w == "hypr") { ap_frame3(b, L); double a = ap_rad(b); b.f(coin(0.8) ? (w == "elips" ? a * rnd(0.1, 1.0) : gf(H_POS)) : gf(H_POS)); }
  else if (w == "parab") { ap_frame3(b, L); ap_rad(b); }
  else if (w == "cylinder" || w == "sphere") { ap_ax3(b, L); ap_rad(b); }
  else if (w == "cone") { ap_ax3(b, L); b.f(coin(0.8) ? rnd(0.05, 1.5) * (coin(0.2) ? -1 : 1) : gf(H_ANG)); ap_rad(b); }
  else if (w == "torus") { ap_ax3(b, L); double a = ap_rad(b); b.f(coin(0.8) ? std::fabs(a) * rnd(0.05, 0.95) : gf(H_POS)); }
  else if (w == "mat") ap_vec(b, 9);
  else if (w == "quat") ap_vec(b, 4);
  else if (w == "trsf") ap_trsf(b);
  else if (w == "pnt2d") ap_pnt2(b, L);
  else if (w == "vec2d" || w == "xy") ap_vec(b, 2);
  else if (w == "dir2d") ap_dir(b, 2, L);
  else if (w == "ax2d" || w == "lin2d") ap_ax2d(b, L);
  else if (w == "ax22d") ap_frame2(b, L);
  else if (w == "circ2d") { ap_frame2(b, L); ap_rad(b); }
  else if (w == "elips2d" || w == "hypr2d") { ap_frame2(b, L); double a = ap_rad(b); b.f(coin(0.8) ? (w == "elips2d" ? a * rnd(0.1, 1.0) : gf(H_POS)) : gf(H_POS)); }
  else if (w == "parab2d") { ap_frame2(b, L); ap_rad(b); }
  else if (w == "mat2d") ap_vec(b, 4);
  else if (w == "trsf2d") ap_trsf2d(b);
  else { std::fprintf(stderr, "unknown gp word %s\n", w.c_str()); std::abort(); }
  if (w == "trsf" || w == "trsf2d") { /* variable length */ }
  return Raw{w, b.pat, b.v};
}
// arrays: count (or rows cols) then the elements
static int elem_stride(const std::string& e) {
  if (e == "f" || e == "i") return 1;
  return (int)g_gp(e, nullptr).v.size();
}
static Raw g_arr(const std::string& e, int dims) {
  Raw r{(dims == 1 ? "arr_" : "arr2_") + e, "", {}};
  int rows = coin(0.15) ? 1 : rint(1, 6), cols = dims == 2 ? rint(1, 4) : 1;
  if (dims == 1) { r.pat = "i"; r.v.push_back(rows); } else { r.pat = "ii"; r.v.push_back(rows); r.v.push_back(cols); }
  for (int k = 0; k < rows * cols; ++k) {
    if (e == "f") { r.pat += 'f'; r.v.push_back(gf(H_GEN)); }
    else if (e == "i") { r.pat += 'i'; r.v.push_back((double)gi(H_GEN)); }
    else { Raw x = g_gp(e, nullptr); r.pat += x.pat; for (double d : x.v) r.v.push_back(d); }
  }
  return r;
}

// ----- rebuilding objects from the raw inputs (may raise Standard_Failure: a documented / unchecked-free throw)
static gp_Pnt mkp_pnt(const double* v) { return gp_Pnt(v[0], v[1], v[2]); }
static gp_Vec mkp_vec(const double* v) { return gp_Vec(v[0], v[1], v[2]); }
static gp_XYZ mkp_xyz(const double* v) { return gp_XYZ(v[0], v[1], v[2]); }
static gp_Dir mkp_dir(const double* v) { return gp_Dir(v[0], v[1], v[2]); }
static gp_Ax1 mkp_ax1(const double* v) { return gp_Ax1(mkp_pnt(v), mkp_dir(v + 3)); }
static gp_Ax2 mkp_ax2(const double* v) { return gp_Ax2(mkp_pnt(v), mkp_dir(v + 3), mkp_dir(v + 6)); }
static gp_Ax3 mkp_ax3(const double* v) {
  gp_Ax3 a(mkp_ax2(v)); int ind = (int)v[9];
  if (ind == 1) a.XReverse(); else if (ind == 2) a.YReverse();
  return a;
}
static gp_Lin mkp_lin(const double* v) { return gp_Lin(mkp_ax1(v)); }
static gp_Circ mkp_circ(const double* v) { return gp_Circ(mkp_ax2(v), v[9]); }
static gp_Elips mkp_elips(const double* v) { return gp_Elips(mkp_ax2(v), v[9], v[10]); }
static gp_Hypr mkp_hypr(const double* v) { return gp_Hypr(mkp_ax2(v), v[9], v[10]); }
static gp_Parab mkp_parab(const double* v) { return gp_Parab(mkp_ax2(v), v[9]); }
static gp_Pln mkp_pln(const double* v) { return gp_Pln(mkp_ax3(v)); }
static gp_Cylinder mkp_cylinder(const double* v) { return gp_Cylinder(mkp_ax3(v), v[10]); }
static gp_Cone mkp_cone(const double* v) { return gp_Cone(mkp_ax3(v), v[10], v[11]); }
static gp_Sphere mkp_sphere(const double* v) { return gp_Sphere(mkp_ax3(v), v[10]); }
static gp_Torus mkp_torus(const double* v) { return gp_Torus(mkp_ax3(v), v[10], v[11]); }
static gp_Mat mkp_mat(const double* v) { return gp_Mat(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8]); }
static gp_Quaternion mkp_quat(const double* v) { return gp_Quaternion(v[0], v[1], v[2], v[3]); }
static gp_Trsf mkp_trsf(const double* v) {
  gp_Trsf t; int k = (int)v[0];
  switch (k) {
    case 0: t.SetRotation(mkp_ax1(v + 1), v[7]); break;
    case 2: t.SetScale(mkp_pnt(v + 1), v[4]); break;
    case 3: t.SetTranslation(mkp_vec(v + 1)); break;
    case 4: t.SetMirror(mkp_ax1(v + 1)); break;
    case 5: t.SetMirror(mkp_pnt(v + 1)); break;
    default: t.SetMirror(gp_Ax2(mkp_pnt(v + 1), mkp_dir(v + 4))); break;
  }
  return t;
}
static gp_Pnt2d mkp_pnt2d(const double* v) { return gp_Pnt2d(v[0], v[1]); }
static gp_Vec2d mkp_vec2d(const double* v) { return gp_Vec2d(v[0], v[1]); }
static gp_XY mkp_xy(const double* v) { return gp_XY(v[0], v[1]); }
static gp_Dir2d mkp_dir2d(const double* v) { return gp_Dir2d(v[0], v[1]); }
static gp_Ax2d mkp_ax2d(const double* v) { return gp_Ax2d(mkp_pnt2d(v), mkp_dir2d(v + 2)); }
static gp_Ax22d mkp_ax22d(const double* v) { return gp_Ax22d(mkp_pnt2d(v), mkp_dir2d(v + 2), mkp_dir2d(v + 4)); }
static gp_Lin2d mkp_lin2d(const double* v) { return gp_Lin2d(mkp_ax2d(v)); }
static gp_Circ2d mkp_circ2d(const double* v) { return gp_Circ2d(mkp_ax22d(v), v[6]); }
static gp_Elips2d mkp_elips2d(const double* v) { return gp_Elips2d(mkp_ax22d(v), v[6], v[7]); }
static gp_Hypr2d mkp_hypr2d(const double* v) { return gp_Hypr2d(mkp_ax22d(v), v[6], v[7]); }
static gp_Parab2d mkp_parab2d(const double* v) { return gp_Parab2d(mkp_ax22d(v), v[6]); }
static gp_Mat2d mkp_mat2d(const double* v) { gp_Mat2d m; m.SetValue(1, 1, v[0]); m.SetValue(1, 2, v[1]); m.SetValue(2, 1, v[2]); m.SetValue(2, 2, v[3]); return m; }
static gp_Trsf2d mkp_trsf2d(const double* v) {
  gp_Trsf2d t; int k = (int)v[0];
  switch (k) {
    case 0: t.SetRotation(mkp_pnt2d(v + 1), v[3]); break;
    case 2: t.SetScale(mkp_pnt2d(v + 1), v[3]); break;
    case 3: t.SetTranslation(mkp_vec2d(v + 1)); break;
    case 5: t.SetMirror(mkp_pnt2d(v + 1)); break;
    default: t.SetMirror(mkp_ax2d(v + 1)); break;
  }
  return t;
}
static double mkp_f(const double* v) { return v[0]; }
static int mkp_i(const double* v) { return (int)v[0]; }
#define MK(W, T) static T mk_##W(const Raw& r) { return mkp_##W(r.v.data()); }
MK(pnt, gp_Pnt) MK(vec, gp_Vec) MK(xyz, gp_XYZ) MK(dir, gp_Dir) MK(ax1, gp_Ax1) MK(ax2, gp_Ax2) MK(ax3, gp_Ax3)
MK(lin, gp_Lin) MK(circ, gp_Circ) MK(elips, gp_Elips) MK(hypr, gp_Hypr) MK(parab, gp_Parab) MK(pln, gp_Pln)
MK(cylinder, gp_Cylinder) MK(cone, gp_Cone) MK(sphere, gp_Sphere) MK(torus, gp_Torus) MK(mat, gp_Mat) MK(trsf, gp_Trsf)
MK(quat, gp_Quaternion) MK(pnt2d, gp_Pnt2d) MK(vec2d, gp_Vec2d) MK(xy, gp_XY) MK(dir2d, gp_Dir2d) MK(ax2d, gp_Ax2d)
MK(ax22d, gp_Ax22d) MK(lin2d, gp_Lin2d) MK(circ2d, gp_Circ2d) MK(elips2d, gp_Elips2d) MK(hypr2d, gp_Hypr2d)
MK(parab2d, gp_Parab2d) MK(mat2d, gp_Mat2d) MK(trsf2d, gp_Trsf2d) MK(f, double) MK(i, int)
static bool mk_b(const Raw& r) { return r.v.at(0) != 0; }
template <typename T, T (*F)(const double*)> static NCollection_Array1<T> mk_arr1(const Raw& r, int stride) {
  int n = (int)r.v.at(0); NCollection_Array1<T> a(1, n);
  for (int k = 0; k < n; ++k) a.SetValue(1 + k, F(&r.v.at(1 + (size_t)k * stride)));
  return a;
}
template <typename T, T (*F)(const double*)> static NCollection_Array2<T> mk_arr2(const Raw& r, int stride) {
  int nr = (int)r.v.at(0), nc = (int)r.v.at(1); NCollection_Array2<T> a(1, nr, 1, nc);
  for (int i = 0; i < nr; ++i) for (int j = 0; j < nc; ++j) a.SetValue(1 + i, 1 + j, F(&r.v.at(2 + ((size_t)i * nc + j) * stride)));
  return a;
}
static void o_f(double v) { h(v); }
static void o_i(long v) { in(v); }
// an unsigned 64 bit value (a hash): printed as `h` and 16 hex digits (not a float token)
static void o_u(unsigned long long v) { char t[28]; std::snprintf(t, sizeof t, " h%016llx", v); buf += t; ++ntok; }
static void o_b(bool v) { ob(v); }
static void o_xyz(const gp_XYZ& p) { h(p.X()); h(p.Y()); h(p.Z()); }
static void o_pnt(const gp_Pnt& p) { h(p.X()); h(p.Y()); h(p.Z()); }
static void o_vec(const gp_Vec& p) { h(p.X()); h(p.Y()); h(p.Z()); }
static void o_dir(const gp_Dir& p) { h(p.X()); h(p.Y()); h(p.Z()); }
static void o_ax1(const gp_Ax1& a) { o_pnt(a.Location()); o_dir(a.Direction()); }
static void o_ax2(const gp_Ax2& a) { o_pnt(a.Location()); o_dir(a.Direction()); o_dir(a.XDirection()); o_dir(a.YDirection()); }
static void o_ax3(const gp_Ax3& a) { o_pnt(a.Location()); o_dir(a.Direction()); o_dir(a.XDirection()); o_dir(a.YDirection()); }
static void o_lin(const gp_Lin& c) { o_ax1(c.Position()); }
static void o_circ(const gp_Circ& c) { o_ax2(c.Position()); h(c.Radius()); }
static void o_elips(const gp_Elips& c) { o_ax2(c.Position()); h(c.MajorRadius()); h(c.MinorRadius()); }
static void o_hypr(const gp_Hypr& c) { o_ax2(c.Position()); h(c.MajorRadius()); h(c.MinorRadius()); }
static void o_parab(const gp_Parab& c) { o_ax2(c.Position()); h(c.Focal()); }
static void o_pln(const gp_Pln& c) { o_ax3(c.Position()); }
static void o_cylinder(const gp_Cylinder& c) { o_ax3(c.Position()); h(c.Radius()); }
static void o_cone(const gp_Cone& c) { o_ax3(c.Position()); h(c.SemiAngle()); h(c.RefRadius()); }
static void o_sphere(const gp_Sphere& c) { o_ax3(c.Position()); h(c.Radius()); }
static void o_torus(const gp_Torus& c) { o_ax3(c.Position()); h(c.MajorRadius()); h(c.MinorRadius()); }
static void o_mat(const gp_Mat& m) { for (int i = 1; i <= 3; ++i) for (int j = 1; j <= 3; ++j) h(m.Value(i, j)); }
static void o_trsf(const gp_Trsf& m) { for (int i = 1; i <= 3; ++i) for (int j = 1; j <= 4; ++j) h(m.Value(i, j)); }
static void o_quat(const gp_Quaternion& q) { h(q.X()); h(q.Y()); h(q.Z()); h(q.W()); }
static void o_pnt2d(const gp_Pnt2d& p) { h(p.X()); h(p.Y()); }
static void o_vec2d(const gp_Vec2d& p) { h(p.X()); h(p.Y()); }
static void o_xy(const gp_XY& p) { h(p.X()); h(p.Y()); }
static void o_dir2d(const gp_Dir2d& p) { h(p.X()); h(p.Y()); }
static void o_ax2d(const gp_Ax2d& a) { o_pnt2d(a.Location()); o_dir2d(a.Direction()); }
static void o_ax22d(const gp_Ax22d& a) { o_pnt2d(a.Location()); o_dir2d(a.XDirection()); o_dir2d(a.YDirection()); }
static void o_lin2d(const gp_Lin2d& c) { o_ax2d(c.Position()); }
static void o_circ2d(const gp_Circ2d& c) { o_ax22d(c.Position()); h(c.Radius()); }
static void o_elips2d(const gp_Elips2d& c) { o_ax22d(c.Axis()); h(c.MajorRadius()); h(c.MinorRadius()); }
static void o_hypr2d(const gp_Hypr2d& c) { o_ax22d(c.Axis()); h(c.MajorRadius()); h(c.MinorRadius()); }
static void o_parab2d(const gp_Parab2d& c) { o_ax22d(c.Axis()); h(c.Focal()); }
static void o_mat2d(const gp_Mat2d& m) { for (int i = 1; i <= 2; ++i) for (int j = 1; j <= 2; ++j) h(m.Value(i, j)); }
static void o_trsf2d(const gp_Trsf2d& m) { for (int i = 1; i <= 2; ++i) for (int j = 1; j <= 3; ++j) h(m.Value(i, j)); }
#define ARR_OUT(W, PRINT) template <class A> static void o_arr_##W(const A& a) { in(a.Lower()); in(a.Upper()); for (int i = a.Lower(); i <= a.Upper(); ++i) PRINT(a(i)); }
ARR_OUT(f, o_f) ARR_OUT(i, o_i) ARR_OUT(pnt, o_pnt) ARR_OUT(vec, o_vec) ARR_OUT(dir, o_dir) ARR_OUT(xyz, o_xyz)
ARR_OUT(pnt2d, o_pnt2d) ARR_OUT(vec2d, o_vec2d) ARR_OUT(dir2d, o_dir2d) ARR_OUT(xy, o_xy)
#define ARR2_OUT(W, PRINT) template <class A> static void o_arr2_##W(const A& a) { in(a.LowerRow()); in(a.UpperRow()); in(a.LowerCol()); in(a.UpperCol()); for (int i = a.LowerRow(); i <= a.UpperRow(); ++i) for (int j = a.LowerCol(); j <= a.UpperCol(); ++j) PRINT(a(i, j)); }
ARR2_OUT(f, o_f) ARR2_OUT(pnt, o_pnt)

@@HANDLES@@
static const char* g_only_fam = nullptr;
static std::set<std::string> g_skip;
static bool fam_wanted(const char* n) {
  if (g_skip.count(n)) return false;
  if (!g_only_fam) return true;
  std::string o = std::string(",") + g_only_fam + ",", k = std::string(",") + n + ",";
  return o.find(k) != std::string::npos;
}
static const int MAXATT = 400;
// in-out probing: a non-const-reference scalar parameter is an in-out one if the call's outputs depend on its initial value
static bool g_probe = false;
static int g_probe_j = -1;
static std::set<std::string> g_inout, g_probe_hits;
static bool inout_flag(const char* n, int j) { return g_probe || g_inout.count(std::string(n) + ":" + std::to_string(j)) > 0; }
static bool probe_diff(const std::function<void()>& body, int j) {
  fflush(stdout); fflush(stderr);
  pid_t p = fork();
  if (p == 0) {
    signal(SIGSEGV, SIG_DFL); signal(SIGABRT, SIG_DFL); signal(SIGFPE, SIG_DFL); signal(SIGBUS, SIG_DFL);
    alarm(20);
    g_probe_j = -1;
    bool d1 = false, d2 = false;
    std::string a = run_once(body, 0, d1);
    g_probe_j = j;
    std::string b = run_once(body, 0, d2);
    g_probe_j = -1;
    _exit(!d1 && !d2 && a != b ? 2 : 0);
  }
  int st = 0; waitpid(p, &st, 0);
  return WIFEXITED(st) && WEXITSTATUS(st) == 2;
}
'''

CPP_MAIN = r'''
int main(int argc, char** argv) {
  // The oracle runs with address space randomization off, so that its decisions (which calls crash, which read freed or
  // uninitialised memory) do not depend on where the allocator puts things: two runs print the same bytes. A third run with
  // randomization on (JG_ASLR=1) is compared line by line by the driver: a line that differs there depends on an address.
  if (!std::getenv("JG_ASLR") && !std::getenv("JG_REEXEC")) {
    int pers = personality(0xffffffff);
    if (pers != -1 && !(pers & ADDR_NO_RANDOMIZE) && personality((unsigned long)pers | ADDR_NO_RANDOMIZE) != -1) {
      setenv("JG_REEXEC", "1", 1);
      execv("/proc/self/exe", argv);
    }
  }
  (void)argc;
  signal(SIGPIPE, SIG_IGN);
  const char* c = std::getenv("JG_CASES");
  if (c) g_cases = std::atoi(c);
  const char* s = std::getenv("JG_SKIP");
  std::string skip = std::string(s ? s : "") + "," + "@@SKIP@@";
  for (size_t p = 0; p < skip.size();) {
    size_t e = skip.find(',', p); if (e == std::string::npos) e = skip.size();
    if (e > p) g_skip.insert(skip.substr(p, e - p));
    p = e + 1;
  }
  g_only_fam = std::getenv("JG_ONLY");
  {
    std::string io = std::string(std::getenv("JG_INOUT") ? std::getenv("JG_INOUT") : "") + "," + "@@INOUT@@";
    for (size_t p = 0; p < io.size();) {
      size_t e = io.find(',', p); if (e == std::string::npos) e = io.size();
      if (e > p) g_inout.insert(io.substr(p, e - p));
      p = e + 1;
    }
  }
  g_probe = std::getenv("JG_PROBE") != nullptr;
  std::setvbuf(stdout, nullptr, _IOFBF, 1 << 20);
  @@CALLS@@
  std::fflush(stdout);
  if (g_probe) {
    FILE* f = std::fopen(std::getenv("JG_PROBE"), "w");
    for (const auto& h : g_probe_hits) std::fprintf(f, "%s\n", h.c_str());
    std::fclose(f);
    return 0;
  }
  const char* meta = std::getenv("JG_META");
  if (meta) {
    FILE* f = std::fopen(meta, "w");
    for (const auto& n : fam_order) std::fprintf(f, "%s\t%d\t%d\t%d\t%d\n", n.c_str(), fam_lines[n], fam_libm[n] ? 1 : 0, fam_throws[n], fam_dropped[n]);
    std::fclose(f);
  }
  return 0;
}
'''


CPP_HANDLES = r'''
// =================================================================================
// handles: curve and surface recipes (the grammar of the reviewed judges tests/geom.rs and tests/geom2d.rs; docs/api-geom.md)
//   crv (3D):  kind 0 line  1 circle  2 ellipse  3 hyperbola  4 parabola:  kind ctor v0..v8 [r | a b | f]
//       line: ctor (0 Ax1, 1 gp_Lin, 2 point + direction) then o d (6 floats); circle, ellipse, hyperbola: ctor (0 gp_ object, 1 Ax2 form)
//       then the frame o n x (9 floats) then r | a b; parabola: ctor (0 gp_Parab, 1 (Ax2, f), 2 (directrix Ax1, focus)) then o n x f
//       (ctor 0, 1) or o d focus (ctor 2, 9 floats)
//     kind 5 bezier: n n*(x y z) rat [nw nw*w]     kind 6 bspline: deg periodic nk nk*knot nm nm*mult np np*(x y z) rat [nw nw*w]
//     kind 7 trimmed: u1 u2 sense adjust crv       kind 8 offset: offset notcheck dx dy dz crv
//   crv2d: kind ctor ox oy dx dy sense then r | a b | a b | f [fx fy when kind 4 and ctor 3]; bezier / bspline with (x y);
//     trimmed as above; offset: offset notcheck crv
//   srf: kind 0 plane: ctor (0 Ax3, 1 gp_Pln, 2 point + normal, 3 coefficients) then frame (10) | frame (10) | o n (6) | A B C D
//     kind 1 cylinder: ctor (0 (Ax3, r), 1 gp_Cylinder) frame r    kind 2 cone: ctor frame angle r    kind 3 sphere: ctor frame r
//     kind 4 torus: ctor frame R r      kind 5 bezier: nu nv nu*nv*(x y z) rat [nw nw*w]   (u index major)
//     kind 6 bspline: udeg vdeg uper vper nku knots nmu mults nkv knots nmv mults nu nv nu*nv*(x y z) rat [nw nw*w]
//     kind 7 extrusion: dx dy dz crv       kind 8 revolution: o d (6) crv
//     kind 9 rectangular trimmed: ctor (0: u1 u2 v1 v2 usense vsense, 1: p1 p2 utrim sense) srf      kind 10 offset: offset notcheck srf
//   A frame is o n x flip (10 floats): gp_Ax3(o, n, x), YReverse when flip != 0.
struct CV {
  int dim = 3, kind = 0, ctor = 0;
  double v[9] = {0, 0, 0, 0, 0, 1, 1, 0, 0};
  int sense = 1;
  double a = 0, b = 0, fx = 0, fy = 0;
  int deg = 1, per = 0, rat = 0;
  Vd knots; std::vector<int> mults; Vd px, py, pz; Vd w;
  double u1 = 0, u2 = 1; int adj = 1; double off = 0; int notc = 0; double od[3] = {0, 0, 1};
  std::shared_ptr<CV> basis;
};
struct SV {
  int kind = 0, ctor = 0;
  double fr[10] = {0, 0, 0, 0, 0, 1, 1, 0, 0, 0};
  double a = 0, b = 0, c = 0, d = 0;
  int nu = 0, nv = 0, udeg = 1, vdeg = 1, uper = 0, vper = 0, rat = 0;
  Vd uk, vk; std::vector<int> um, vm; Vd px, py, pz; Vd w;
  double dir[6] = {0, 0, 1, 0, 0, 1};
  double t[4] = {0, 1, 0, 1}; int s1 = 1, s2 = 1, utrim = 1;
  double off = 0; int notc = 0;
  std::shared_ptr<CV> curve; std::shared_ptr<SV> basis;
};
static void p_pts(RB& b, const CV& c) {
  b.i((long)c.px.size());
  for (size_t i = 0; i < c.px.size(); ++i) { b.f(c.px[i]); b.f(c.py[i]); if (c.dim == 3) b.f(c.pz[i]); }
}
static void p_wts(RB& b, const CV& c) { b.i(c.rat); if (c.rat) { b.i((long)c.w.size()); for (double x : c.w) b.f(x); } }
static void p_crv(RB& b, const CV& c) {
  b.i(c.kind);
  if (c.kind <= 4) {
    b.i(c.ctor);
    if (c.dim == 2) {
      for (int i = 0; i < 4; ++i) b.f(c.v[i]);
      b.i(c.sense);
      if (c.kind == 1 || c.kind == 4) b.f(c.a);
      if (c.kind == 2 || c.kind == 3) { b.f(c.a); b.f(c.b); }
      if (c.kind == 4 && c.ctor == 3) { b.f(c.fx); b.f(c.fy); }
    } else if (c.kind == 0) { for (int i = 0; i < 6; ++i) b.f(c.v[i]); }
    else {
      for (int i = 0; i < 9; ++i) b.f(c.v[i]);
      if (c.kind == 1 || (c.kind == 4 && c.ctor != 2)) b.f(c.a);
      if (c.kind == 2 || c.kind == 3) { b.f(c.a); b.f(c.b); }
    }
  } else if (c.kind == 5) {
    p_pts(b, c); p_wts(b, c);
  } else if (c.kind == 6) {
    b.i(c.deg); b.i(c.per); b.i((long)c.knots.size());
    for (double x : c.knots) b.f(x);
    b.i((long)c.mults.size()); for (int m : c.mults) b.i(m);
    p_pts(b, c); p_wts(b, c);
  } else if (c.kind == 7) {
    b.f(c.u1); b.f(c.u2); b.i(c.sense); b.i(c.adj); p_crv(b, *c.basis);
  } else {
    b.f(c.off); b.i(c.notc);
    if (c.dim == 3) { b.f(c.od[0]); b.f(c.od[1]); b.f(c.od[2]); }
    p_crv(b, *c.basis);
  }
}
static void p_srf(RB& b, const SV& s) {
  b.i(s.kind);
  switch (s.kind) {
    case 0:
      b.i(s.ctor);
      if (s.ctor <= 1) { for (int i = 0; i < 10; ++i) b.f(s.fr[i]); }
      else if (s.ctor == 2) { for (int i = 0; i < 6; ++i) b.f(s.fr[i]); }
      else { b.f(s.a); b.f(s.b); b.f(s.c); b.f(s.d); }
      break;
    case 1: case 3: b.i(s.ctor); for (int i = 0; i < 10; ++i) b.f(s.fr[i]); b.f(s.a); break;
    case 2: case 4: b.i(s.ctor); for (int i = 0; i < 10; ++i) b.f(s.fr[i]); b.f(s.a); b.f(s.b); break;
    case 5:
      b.i(s.nu); b.i(s.nv);
      for (size_t i = 0; i < s.px.size(); ++i) { b.f(s.px[i]); b.f(s.py[i]); b.f(s.pz[i]); }
      b.i(s.rat); if (s.rat) { b.i((long)s.w.size()); for (double x : s.w) b.f(x); }
      break;
    case 6:
      b.i(s.udeg); b.i(s.vdeg); b.i(s.uper); b.i(s.vper);
      b.i((long)s.uk.size()); for (double x : s.uk) b.f(x);
      b.i((long)s.um.size()); for (int m : s.um) b.i(m);
      b.i((long)s.vk.size()); for (double x : s.vk) b.f(x);
      b.i((long)s.vm.size()); for (int m : s.vm) b.i(m);
      b.i(s.nu); b.i(s.nv);
      for (size_t i = 0; i < s.px.size(); ++i) { b.f(s.px[i]); b.f(s.py[i]); b.f(s.pz[i]); }
      b.i(s.rat); if (s.rat) { b.i((long)s.w.size()); for (double x : s.w) b.f(x); }
      break;
    case 7: b.f(s.dir[0]); b.f(s.dir[1]); b.f(s.dir[2]); p_crv(b, *s.curve); break;
    case 8: for (int i = 0; i < 6; ++i) b.f(s.dir[i]); p_crv(b, *s.curve); break;
    case 9:
      b.i(s.ctor);
      if (s.ctor == 0) { b.f(s.t[0]); b.f(s.t[1]); b.f(s.t[2]); b.f(s.t[3]); b.i(s.s1); b.i(s.s2); }
      else { b.f(s.t[0]); b.f(s.t[1]); b.i(s.utrim); b.i(s.s1); }
      p_srf(b, *s.basis);
      break;
    default: b.f(s.off); b.i(s.notc); p_srf(b, *s.basis); break;
  }
}

typedef occ::handle<Geom_Curve> HC3;
typedef occ::handle<Geom2d_Curve> HC2;
typedef occ::handle<Geom_Surface> HS3;
static gp_Pnt hp3(const double* v) { return gp_Pnt(v[0], v[1], v[2]); }
static gp_Dir hd3(const double* v) { return gp_Dir(v[0], v[1], v[2]); }
static gp_Ax2 hax2(const double* v) { return gp_Ax2(hp3(v), hd3(v + 3), hd3(v + 6)); }
static gp_Ax3 hax3(const double* v) { gp_Ax3 r(hp3(v), hd3(v + 3), hd3(v + 6)); if (std::lround(v[9]) != 0) r.YReverse(); return r; }
static gp_Ax2d hax2d(const double* v) { return gp_Ax2d(gp_Pnt2d(v[0], v[1]), gp_Dir2d(v[2], v[3])); }
static HC3 build_c(const CV& c);
static HC3 build_c3_basic(const CV& c) {
  const double* v = c.v;
  switch (c.kind) {
    case 0:
      if (c.ctor == 0) return new Geom_Line(gp_Ax1(hp3(v), hd3(v + 3)));
      if (c.ctor == 1) return new Geom_Line(gp_Lin(gp_Ax1(hp3(v), hd3(v + 3))));
      return new Geom_Line(hp3(v), hd3(v + 3));
    case 1:
      if (c.ctor == 0) return new Geom_Circle(gp_Circ(hax2(v), c.a));
      return new Geom_Circle(hax2(v), c.a);
    case 2:
      if (c.ctor == 0) return new Geom_Ellipse(gp_Elips(hax2(v), c.a, c.b));
      return new Geom_Ellipse(hax2(v), c.a, c.b);
    case 3:
      if (c.ctor == 0) return new Geom_Hyperbola(gp_Hypr(hax2(v), c.a, c.b));
      return new Geom_Hyperbola(hax2(v), c.a, c.b);
    default:
      if (c.ctor == 0) return new Geom_Parabola(gp_Parab(hax2(v), c.a));
      if (c.ctor == 1) return new Geom_Parabola(hax2(v), c.a);
      return new Geom_Parabola(gp_Ax1(hp3(v), hd3(v + 3)), hp3(v + 6));
  }
}
static HC3 build_c(const CV& c) {
  if (c.kind <= 4) return build_c3_basic(c);
  if (c.kind == 5 || c.kind == 6) {
    NCollection_Array1<gp_Pnt> P(1, (int)c.px.size());
    for (size_t i = 0; i < c.px.size(); ++i) P((int)i + 1) = gp_Pnt(c.px[i], c.py[i], c.pz[i]);
    NCollection_Array1<double> W(1, (int)c.w.size());
    for (size_t i = 0; i < c.w.size(); ++i) W((int)i + 1) = c.w[i];
    if (c.kind == 5) { if (!c.rat) return new Geom_BezierCurve(P); return new Geom_BezierCurve(P, W); }
    NCollection_Array1<double> K(1, (int)c.knots.size());
    for (size_t i = 0; i < c.knots.size(); ++i) K((int)i + 1) = c.knots[i];
    NCollection_Array1<int> M(1, (int)c.mults.size());
    for (size_t i = 0; i < c.mults.size(); ++i) M((int)i + 1) = c.mults[i];
    if (!c.rat) return new Geom_BSplineCurve(P, K, M, c.deg, c.per != 0);
    return new Geom_BSplineCurve(P, W, K, M, c.deg, c.per != 0);
  }
  HC3 b = build_c(*c.basis);
  if (c.kind == 7) return new Geom_TrimmedCurve(b, c.u1, c.u2, c.sense != 0, c.adj != 0);
  return new Geom_OffsetCurve(b, c.off, gp_Dir(c.od[0], c.od[1], c.od[2]), c.notc != 0);
}
static HC2 build_c2(const CV& c) {
  const double* v = c.v;
  bool s = c.sense != 0;
  if (c.kind <= 4) {
    gp_Ax2d ax = hax2d(v);
    switch (c.kind) {
      case 0:
        if (c.ctor == 0) return new Geom2d_Line(ax);
        if (c.ctor == 1) return new Geom2d_Line(gp_Lin2d(ax));
        return new Geom2d_Line(ax.Location(), ax.Direction());
      case 1:
        if (c.ctor == 0) return new Geom2d_Circle(gp_Circ2d(ax, c.a, s));
        if (c.ctor == 1) return new Geom2d_Circle(ax, c.a, s);
        return new Geom2d_Circle(gp_Ax22d(ax, s), c.a);
      case 2:
        if (c.ctor == 0) return new Geom2d_Ellipse(gp_Elips2d(ax, c.a, c.b, s));
        if (c.ctor == 1) return new Geom2d_Ellipse(ax, c.a, c.b, s);
        return new Geom2d_Ellipse(gp_Ax22d(ax, s), c.a, c.b);
      case 3:
        if (c.ctor == 0) return new Geom2d_Hyperbola(gp_Hypr2d(ax, c.a, c.b, s));
        if (c.ctor == 1) return new Geom2d_Hyperbola(ax, c.a, c.b, s);
        return new Geom2d_Hyperbola(gp_Ax22d(ax, s), c.a, c.b);
      default:
        if (c.ctor == 0) return new Geom2d_Parabola(gp_Parab2d(ax, c.a, s));
        if (c.ctor == 1) return new Geom2d_Parabola(ax, c.a, s);
        if (c.ctor == 2) return new Geom2d_Parabola(gp_Ax22d(ax, s), c.a);
        return new Geom2d_Parabola(ax, gp_Pnt2d(c.fx, c.fy));
    }
  }
  if (c.kind == 5 || c.kind == 6) {
    NCollection_Array1<gp_Pnt2d> P(1, (int)c.px.size());
    for (size_t i = 0; i < c.px.size(); ++i) P((int)i + 1) = gp_Pnt2d(c.px[i], c.py[i]);
    NCollection_Array1<double> W(1, (int)c.w.size());
    for (size_t i = 0; i < c.w.size(); ++i) W((int)i + 1) = c.w[i];
    if (c.kind == 5) { if (!c.rat) return new Geom2d_BezierCurve(P); return new Geom2d_BezierCurve(P, W); }
    NCollection_Array1<double> K(1, (int)c.knots.size());
    for (size_t i = 0; i < c.knots.size(); ++i) K((int)i + 1) = c.knots[i];
    NCollection_Array1<int> M(1, (int)c.mults.size());
    for (size_t i = 0; i < c.mults.size(); ++i) M((int)i + 1) = c.mults[i];
    if (!c.rat) return new Geom2d_BSplineCurve(P, K, M, c.deg, c.per != 0);
    return new Geom2d_BSplineCurve(P, W, K, M, c.deg, c.per != 0);
  }
  HC2 b = build_c2(*c.basis);
  if (c.kind == 7) return new Geom2d_TrimmedCurve(b, c.u1, c.u2, s, c.adj != 0);
  return new Geom2d_OffsetCurve(b, c.off, c.notc != 0);
}
static HS3 build_s(const SV& s) {
  switch (s.kind) {
    case 0:
      if (s.ctor == 0) return new Geom_Plane(hax3(s.fr));
      if (s.ctor == 1) return new Geom_Plane(gp_Pln(hax3(s.fr)));
      if (s.ctor == 2) return new Geom_Plane(gp_Pnt(s.fr[0], s.fr[1], s.fr[2]), gp_Dir(s.fr[3], s.fr[4], s.fr[5]));
      return new Geom_Plane(s.a, s.b, s.c, s.d);
    case 1: if (s.ctor == 0) return new Geom_CylindricalSurface(hax3(s.fr), s.a); return new Geom_CylindricalSurface(gp_Cylinder(hax3(s.fr), s.a));
    case 2: if (s.ctor == 0) return new Geom_ConicalSurface(hax3(s.fr), s.a, s.b); return new Geom_ConicalSurface(gp_Cone(hax3(s.fr), s.a, s.b));
    case 3: if (s.ctor == 0) return new Geom_SphericalSurface(hax3(s.fr), s.a); return new Geom_SphericalSurface(gp_Sphere(hax3(s.fr), s.a));
    case 4: if (s.ctor == 0) return new Geom_ToroidalSurface(hax3(s.fr), s.a, s.b); return new Geom_ToroidalSurface(gp_Torus(hax3(s.fr), s.a, s.b));
    case 5: case 6: {
      NCollection_Array2<gp_Pnt> P(1, std::max(s.nu, 0), 1, std::max(s.nv, 0));
      NCollection_Array2<double> W(1, std::max(s.nu, 0), 1, std::max(s.nv, 0));
      for (int i = 0; i < s.nu; ++i) for (int j = 0; j < s.nv; ++j) {
        size_t k = (size_t)(i * s.nv + j);
        P(i + 1, j + 1) = gp_Pnt(s.px[k], s.py[k], s.pz[k]);
        W(i + 1, j + 1) = k < s.w.size() ? s.w[k] : 1.0;
      }
      if (s.kind == 5) { if (!s.rat) return new Geom_BezierSurface(P); return new Geom_BezierSurface(P, W); }
      NCollection_Array1<double> UK(1, (int)s.uk.size()), VK(1, (int)s.vk.size());
      for (size_t i = 0; i < s.uk.size(); ++i) UK((int)i + 1) = s.uk[i];
      for (size_t i = 0; i < s.vk.size(); ++i) VK((int)i + 1) = s.vk[i];
      NCollection_Array1<int> UM(1, (int)s.um.size()), VM(1, (int)s.vm.size());
      for (size_t i = 0; i < s.um.size(); ++i) UM((int)i + 1) = s.um[i];
      for (size_t i = 0; i < s.vm.size(); ++i) VM((int)i + 1) = s.vm[i];
      if (!s.rat) return new Geom_BSplineSurface(P, UK, VK, UM, VM, s.udeg, s.vdeg, s.uper != 0, s.vper != 0);
      return new Geom_BSplineSurface(P, W, UK, VK, UM, VM, s.udeg, s.vdeg, s.uper != 0, s.vper != 0);
    }
    case 7: return new Geom_SurfaceOfLinearExtrusion(build_c(*s.curve), gp_Dir(s.dir[0], s.dir[1], s.dir[2]));
    case 8: return new Geom_SurfaceOfRevolution(build_c(*s.curve), gp_Ax1(gp_Pnt(s.dir[0], s.dir[1], s.dir[2]), gp_Dir(s.dir[3], s.dir[4], s.dir[5])));
    case 9: {
      HS3 b = build_s(*s.basis);
      if (s.ctor == 0) return new Geom_RectangularTrimmedSurface(b, s.t[0], s.t[1], s.t[2], s.t[3], s.s1 != 0, s.s2 != 0);
      return new Geom_RectangularTrimmedSurface(b, s.t[0], s.t[1], s.utrim != 0, s.s1 != 0);
    }
    default: return new Geom_OffsetSurface(build_s(*s.basis), s.off, s.notc != 0);
  }
}
template <class T, class B> static occ::handle<T> hdown(const occ::handle<B>& h) {
  occ::handle<T> r = occ::down_cast<T>(h);
  if (r.IsNull()) throw Discard();
  return r;
}

// ----- random recipes (valid ones: a recipe whose objects cannot be built is regenerated)
static void rdirv(double* d, int n) {
  for (;;) {
    RB t; ap_dir(t, n, nullptr);
    double s = 0; for (int k = 0; k < n; ++k) s += t.v[k] * t.v[k];
    if (s > 0.04) { for (int k = 0; k < n; ++k) d[k] = t.v[k]; return; }
  }
}
static void rpnt(double* p, int n) { for (int k = 0; k < n; ++k) p[k] = crd(); }
static void rframe3(double* v) {   // o n x (9)
  rpnt(v, 3); rdirv(v + 3, 3);
  for (;;) { rdirv(v + 6, 3); if (!near_par3(v + 3, v + 6)) break; }
}
static double rrad(double lo, double hi) { return coin(0.3) ? 0.5 * std::ceil(2 * rnd(lo, hi)) : rnd(lo, hi); }
// knots / multiplicities / pole count of one B-spline direction; false when the combination is not usable
static bool rbs(int& deg, bool per, Vd& k, std::vector<int>& m, int& np) {
  k.clear(); m.clear();
  deg = rint(1, 3);
  if (per) {
    int nk = rint(3, 6);
    for (int i = 0; i < nk; ++i) { k.push_back(i == 0 ? rnd(-2, 2) : k.back() + (coin(0.5) ? 1.0 : rnd(0.3, 1.5))); m.push_back(1); }
    np = nk - 1;
    return np >= 2 && deg < np;
  }
  np = rint(deg + 1, deg + 4);
  int inner = np - deg - 1;
  int nk = rint(2, 2 + inner);
  if (nk - 2 > inner) return false;
  std::vector<int> mm(nk, 1);
  mm[0] = mm[nk - 1] = deg + 1;
  int rest = inner - (nk - 2);
  for (int g = 0; g < rest; ++g) {
    if (nk <= 2) return false;
    int j = rint(1, nk - 2);
    if (mm[j] >= deg) { bool ok = false; for (int t = 1; t < nk - 1; ++t) if (mm[t] < deg) { ++mm[t]; ok = true; break; } if (!ok) return false; }
    else ++mm[j];
  }
  for (int i = 0; i < nk; ++i) { k.push_back(i == 0 ? rnd(-2, 2) : k.back() + (coin(0.5) ? 1.0 : rnd(0.3, 1.5))); m.push_back(mm[i]); }
  return true;
}
static void rwts(std::vector<double>& w, size_t n) { w.clear(); for (size_t i = 0; i < n; ++i) w.push_back(coin(0.3) ? 1.0 : rnd(0.3, 3.0)); }
static CV rcv(int dim, const std::vector<int>& kinds, int depth);
static void cv_range(const CV& c, double& f, double& l) {
  if (c.dim == 3) { HC3 h = build_c(c); f = h->FirstParameter(); l = h->LastParameter(); }
  else { HC2 h = build_c2(c); f = h->FirstParameter(); l = h->LastParameter(); }
  if (!(std::fabs(f) < 1e50) || !(std::fabs(l) < 1e50)) { f = -3; l = 3; }
}
static bool rcv_kind(CV& c, int kind, int depth) {
  const int dim = c.dim;
  c.kind = kind;
  switch (kind) {
    case 0: {
      c.ctor = rint(0, 2);
      if (dim == 3) { rpnt(c.v, 3); rdirv(c.v + 3, 3); } else { rpnt(c.v, 2); rdirv(c.v + 2, 2); }
      return true;
    }
    case 1: case 2: case 3: case 4: {
      c.ctor = dim == 3 ? (kind == 4 ? rint(0, 2) : rint(0, 1)) : (kind == 4 ? rint(0, 3) : rint(0, 2));
      c.sense = coin(0.7) ? 1 : 0;
      if (dim == 3) {
        if (kind == 4 && c.ctor == 2) { rpnt(c.v, 3); rdirv(c.v + 3, 3); rpnt(c.v + 6, 3); }
        else rframe3(c.v);
      } else { rpnt(c.v, 2); rdirv(c.v + 2, 2); }
      if (kind == 1) c.a = rrad(0.3, 5);
      else if (kind == 2) { c.a = rrad(1, 5); c.b = c.a * rnd(0.2, 0.95); }
      else if (kind == 3) { c.a = rrad(0.5, 4); c.b = rrad(0.5, 4); }
      else { c.a = rrad(0.25, 4); c.fx = crd(); c.fy = crd(); }
      return true;
    }
    case 5: {
      int n = rint(2, 5);
      c.px.clear(); c.py.clear(); c.pz.clear();
      for (int i = 0; i < n; ++i) { c.px.push_back(crd()); c.py.push_back(crd()); c.pz.push_back(dim == 3 ? crd() : 0.0); }
      c.rat = coin(0.4) ? 1 : 0;
      if (c.rat) rwts(c.w, (size_t)n);
      return true;
    }
    case 6: {
      int np = 0; bool per = coin(0.15);
      if (!rbs(c.deg, per, c.knots, c.mults, np)) return false;
      c.per = per ? 1 : 0;
      c.px.clear(); c.py.clear(); c.pz.clear();
      for (int i = 0; i < np; ++i) { c.px.push_back(crd()); c.py.push_back(crd()); c.pz.push_back(dim == 3 ? crd() : 0.0); }
      c.rat = coin(0.35) ? 1 : 0;
      if (c.rat) rwts(c.w, (size_t)np);
      return true;
    }
    case 7: {
      if (depth > 0) return false;
      static const std::vector<int> bk = {0, 1, 2, 3, 4, 5, 6};
      c.basis = std::make_shared<CV>(rcv(dim, bk, depth + 1));
      double f, l; cv_range(*c.basis, f, l);
      double x = rnd(0, 0.5), y = rnd(0.55, 1.0);
      c.u1 = f + (l - f) * x; c.u2 = f + (l - f) * y;
      c.sense = coin(0.7) ? 1 : 0; c.adj = coin(0.5) ? 1 : 0;
      return true;
    }
    default: {
      if (depth > 0) return false;
      static const std::vector<int> ok = {0, 1, 2, 5, 6};
      c.basis = std::make_shared<CV>(rcv(dim, ok, depth + 1));
      c.off = coin(0.2) ? 0.0 : rnd(-2, 2); c.notc = coin(0.5) ? 1 : 0;
      if (dim == 3) rdirv(c.od, 3);
      return true;
    }
  }
}
static CV rcv(int dim, const std::vector<int>& kinds, int depth) {
  for (int t = 0; t < 80; ++t) {
    CV c; c.dim = dim;
    try {
      if (!rcv_kind(c, kinds[(size_t)rint(0, (int)kinds.size() - 1)], depth)) continue;
      if (dim == 3) (void)build_c(c); else (void)build_c2(c);
      return c;
    } catch (Standard_Failure&) {
    } catch (Discard&) {
    }
  }
  throw Discard();
}
static SV rsv(const std::vector<int>& kinds, int depth);
static void sv_bounds(const SV& s, double& u1, double& u2, double& v1, double& v2) {
  HS3 h = build_s(s);
  h->Bounds(u1, u2, v1, v2);
  if (!(std::fabs(u1) < 1e50)) u1 = -3;
  if (!(std::fabs(u2) < 1e50)) u2 = 3;
  if (!(std::fabs(v1) < 1e50)) v1 = -3;
  if (!(std::fabs(v2) < 1e50)) v2 = 3;
}
static bool rsv_kind(SV& s, int kind, int depth) {
  s.kind = kind;
  switch (kind) {
    case 0: case 1: case 2: case 3: case 4: {
      s.ctor = kind == 0 ? rint(0, 3) : rint(0, 1);
      if (kind == 0 && s.ctor == 3) {
        double n[3]; rdirv(n, 3); s.a = n[0]; s.b = n[1]; s.c = n[2]; s.d = crd();
        return true;
      }
      rframe3(s.fr); s.fr[9] = coin(0.2) ? 1 : 0;
      if (kind == 1 || kind == 3) s.a = rrad(0.3, 4);
      if (kind == 2) { s.a = (coin(0.2) ? -1 : 1) * rnd(0.1, 1.2); s.b = coin(0.3) ? 0.0 : rrad(0.2, 3); }
      if (kind == 4) { s.a = rrad(1, 5); s.b = s.a * rnd(0.1, 0.9); }
      return true;
    }
    case 5: {
      s.nu = rint(2, 4); s.nv = rint(2, 4);
      s.px.clear(); s.py.clear(); s.pz.clear();
      for (int i = 0; i < s.nu * s.nv; ++i) { s.px.push_back(crd()); s.py.push_back(crd()); s.pz.push_back(crd()); }
      s.rat = coin(0.4) ? 1 : 0;
      if (s.rat) rwts(s.w, (size_t)(s.nu * s.nv));
      return true;
    }
    case 6: {
      bool up = coin(0.1), vp = coin(0.1);
      if (!rbs(s.udeg, up, s.uk, s.um, s.nu)) return false;
      if (!rbs(s.vdeg, vp, s.vk, s.vm, s.nv)) return false;
      s.uper = up ? 1 : 0; s.vper = vp ? 1 : 0;
      s.px.clear(); s.py.clear(); s.pz.clear();
      for (int i = 0; i < s.nu * s.nv; ++i) { s.px.push_back(crd()); s.py.push_back(crd()); s.pz.push_back(crd()); }
      s.rat = coin(0.3) ? 1 : 0;
      if (s.rat) rwts(s.w, (size_t)(s.nu * s.nv));
      return true;
    }
    case 7: case 8: {
      static const std::vector<int> ck = {0, 1, 2, 5, 6};
      s.curve = std::make_shared<CV>(rcv(3, ck, 1));
      if (kind == 7) rdirv(s.dir, 3);
      else { rpnt(s.dir, 3); rdirv(s.dir + 3, 3); }
      return true;
    }
    case 9: {
      if (depth > 0) return false;
      static const std::vector<int> bk = {0, 1, 2, 3, 4, 5, 6};
      s.basis = std::make_shared<SV>(rsv(bk, depth + 1));
      double u1, u2, v1, v2; sv_bounds(*s.basis, u1, u2, v1, v2);
      s.ctor = rint(0, 1);
      double a = rnd(0, 0.45), b = rnd(0.55, 1), c = rnd(0, 0.45), d = rnd(0.55, 1);
      s.t[0] = u1 + (u2 - u1) * a; s.t[1] = u1 + (u2 - u1) * b;
      s.t[2] = v1 + (v2 - v1) * c; s.t[3] = v1 + (v2 - v1) * d;
      if (s.ctor == 1) { s.utrim = coin(0.5) ? 1 : 0; if (!s.utrim) { s.t[0] = s.t[2]; s.t[1] = s.t[3]; } }
      s.s1 = coin(0.7) ? 1 : 0; s.s2 = coin(0.7) ? 1 : 0;
      return true;
    }
    default: {
      if (depth > 0) return false;
      static const std::vector<int> ok = {0, 1, 2, 3, 4};
      s.basis = std::make_shared<SV>(rsv(ok, depth + 1));
      s.off = coin(0.2) ? 0.0 : rnd(-1.5, 1.5); s.notc = coin(0.5) ? 1 : 0;
      return true;
    }
  }
}
static SV rsv(const std::vector<int>& kinds, int depth) {
  for (int t = 0; t < 80; ++t) {
    SV s;
    try {
      if (!rsv_kind(s, kinds[(size_t)rint(0, (int)kinds.size() - 1)], depth)) continue;
      (void)build_s(s);
      return s;
    } catch (Standard_Failure&) {
    } catch (Discard&) {
    }
  }
  throw Discard();
}
// recipes as raw inputs of a schema word
static Raw g_hc(const Bag& B, const char* w, int dim, std::initializer_list<int> kinds) {
  if (const Raw* l = B.last(w)) { if (coin(0.4)) return *l; }
  CV c = rcv(dim, std::vector<int>(kinds), 0);
  RB b; p_crv(b, c);
  Raw r{w, b.pat, b.v}; r.cv = std::make_shared<CV>(c);
  return r;
}
static Raw g_hs(const Bag& B, const char* w, std::initializer_list<int> kinds) {
  if (const Raw* l = B.last(w)) { if (coin(0.4)) return *l; }
  SV s = rsv(std::vector<int>(kinds), 0);
  RB b; p_srf(b, s);
  Raw r{w, b.pat, b.v}; r.sv = std::make_shared<SV>(s);
  return r;
}
// adaptors: use_range first last, then the recipe of the curve / surface they load
static Raw g_adc(const Bag& B, const char* w, int dim) {
  if (const Raw* l = B.last(w)) { if (coin(0.4)) return *l; }
  static const std::vector<int> all = {0, 1, 2, 3, 4, 5, 6, 7, 8};
  CV c = rcv(dim, all, 0);
  double f, l; cv_range(c, f, l);
  RB b; bool rng = coin(0.5);
  double a = rnd(0, 0.45), z = rnd(0.55, 1);
  b.i(rng ? 1 : 0);
  if (rng) { b.f(f + (l - f) * a); b.f(f + (l - f) * z); } else { b.f(0.0); b.f(0.0); }
  p_crv(b, c);
  Raw r{w, b.pat, b.v}; r.cv = std::make_shared<CV>(c);
  return r;
}
static Raw g_ads(const Bag& B, const char* w) {
  if (const Raw* l = B.last(w)) { if (coin(0.4)) return *l; }
  static const std::vector<int> all = {0, 1, 2, 3, 4, 5, 6, 7, 8, 9};
  SV s = rsv(all, 0);
  double u1, u2, v1, v2; sv_bounds(s, u1, u2, v1, v2);
  RB b; bool rng = coin(0.5);
  double a = rnd(0, 0.45), z = rnd(0.55, 1), c = rnd(0, 0.45), d = rnd(0.55, 1);
  b.i(rng ? 1 : 0);
  if (rng) { b.f(u1 + (u2 - u1) * a); b.f(u1 + (u2 - u1) * z); b.f(v1 + (v2 - v1) * c); b.f(v1 + (v2 - v1) * d); }
  else { b.f(0.0); b.f(0.0); b.f(0.0); b.f(0.0); }
  p_srf(b, s);
  Raw r{w, b.pat, b.v}; r.sv = std::make_shared<SV>(s);
  return r;
}
static occ::handle<GeomAdaptor_Curve> mk_adc3(const Raw& r) {
  HC3 h = build_c(*r.cv);
  if (r.v.at(0) != 0) return new GeomAdaptor_Curve(h, r.v.at(1), r.v.at(2));
  return new GeomAdaptor_Curve(h);
}
static occ::handle<Geom2dAdaptor_Curve> mk_adc2(const Raw& r) {
  HC2 h = build_c2(*r.cv);
  if (r.v.at(0) != 0) return new Geom2dAdaptor_Curve(h, r.v.at(1), r.v.at(2));
  return new Geom2dAdaptor_Curve(h);
}
static occ::handle<GeomAdaptor_Surface> mk_ads3(const Raw& r) {
  HS3 h = build_s(*r.sv);
  if (r.v.at(0) != 0) return new GeomAdaptor_Surface(h, r.v.at(1), r.v.at(2), r.v.at(3), r.v.at(4));
  return new GeomAdaptor_Surface(h);
}

// ----- snapshots of handles and adaptors: null flag, first / last parameter, closed, periodic, continuity, D0 and D1 at five
// parameters (a surface: the bounds, closedness, continuity, D0 and D1 at four points); an evaluation that raised prints `x`
static void snap_params(double f, double l, double* t) {
  if (std::fabs(f) < 1e50 && std::fabs(l) < 1e50) { for (int k = 0; k < 5; ++k) t[k] = f + (l - f) * (k * 0.25); }
  else { t[0] = -2; t[1] = -1; t[2] = 0.25; t[3] = 1; t[4] = 2; }
}
static void snap_uv(double u1, double u2, double v1, double v2, double* u, double* v) {
  static const double K[4][2] = {{0.0, 0.0}, {0.25, 0.75}, {0.5, 0.5}, {1.0, 1.0}};
  bool fu = std::fabs(u1) < 1e50 && std::fabs(u2) < 1e50, fv = std::fabs(v1) < 1e50 && std::fabs(v2) < 1e50;
  for (int k = 0; k < 4; ++k) {
    u[k] = fu ? u1 + (u2 - u1) * K[k][0] : -2.0 + k;
    v[k] = fv ? v1 + (v2 - v1) * K[k][1] : 2.0 - k;
  }
}
#define SNAP(...) try { __VA_ARGS__; } catch (Standard_Failure&) { ox(); }
// a parameter bound: 1 and the value when finite, 0 otherwise
static void hb(double x) { if (std::fabs(x) < 1e50) { in(1); h(x); } else in(0); }
static void o_h_c3(const occ::handle<Geom_Curve>& c) {
  if (c.IsNull()) { in(0); return; }
  in(1);
  const Geom_Curve& k = *c;
  double f = 0, l = 0, t[5];
  SNAP(f = k.FirstParameter(); hb(f)); SNAP(l = k.LastParameter(); hb(l));
  SNAP(ob(k.IsPeriodic())); SNAP(in((long)static_cast<int>(k.Continuity())));
  snap_params(f, l, t);
  for (int i = 0; i < 5; ++i) { SNAP(o_pnt(k.EvalD0(t[i]))); }
  for (int i = 0; i < 5; ++i) { SNAP(Geom_Curve::ResD1 r = k.EvalD1(t[i]); o_pnt(r.Point); o_vec(r.D1)); }
}
static void o_h_c2(const occ::handle<Geom2d_Curve>& c) {
  if (c.IsNull()) { in(0); return; }
  in(1);
  const Geom2d_Curve& k = *c;
  double f = 0, l = 0, t[5];
  SNAP(f = k.FirstParameter(); hb(f)); SNAP(l = k.LastParameter(); hb(l));
  SNAP(ob(k.IsPeriodic())); SNAP(in((long)static_cast<int>(k.Continuity())));
  snap_params(f, l, t);
  for (int i = 0; i < 5; ++i) { SNAP(o_pnt2d(k.EvalD0(t[i]))); }
  for (int i = 0; i < 5; ++i) { SNAP(Geom2d_Curve::ResD1 r = k.EvalD1(t[i]); o_pnt2d(r.Point); o_vec2d(r.D1)); }
}
static void o_h_s3(const occ::handle<Geom_Surface>& s) {
  if (s.IsNull()) { in(0); return; }
  in(1);
  const Geom_Surface& k = *s;
  double u1 = 0, u2 = 0, v1 = 0, v2 = 0, u[4], v[4];
  SNAP(k.Bounds(u1, u2, v1, v2); hb(u1); hb(u2); hb(v1); hb(v2));
  SNAP(ob(k.IsUClosed())); SNAP(ob(k.IsVClosed())); SNAP(ob(k.IsUPeriodic())); SNAP(ob(k.IsVPeriodic()));
  SNAP(in((long)static_cast<int>(k.Continuity())));
  snap_uv(u1, u2, v1, v2, u, v);
  for (int i = 0; i < 4; ++i) { SNAP(o_pnt(k.EvalD0(u[i], v[i]))); }
  for (int i = 0; i < 4; ++i) { SNAP(Geom_Surface::ResD1 r = k.EvalD1(u[i], v[i]); o_pnt(r.Point); o_vec(r.D1U); o_vec(r.D1V)); }
}
template <class A> static void o_ad_c3(const A& c) {
  double f = 0, l = 0, t[5];
  SNAP(f = c.FirstParameter(); hb(f)); SNAP(l = c.LastParameter(); hb(l));
  SNAP(ob(c.IsClosed())); SNAP(ob(c.IsPeriodic())); SNAP(in((long)static_cast<int>(c.Continuity())));
  snap_params(f, l, t);
  for (int i = 0; i < 5; ++i) { SNAP(gp_Pnt p; c.D0(t[i], p); o_pnt(p)); }
  for (int i = 0; i < 5; ++i) { SNAP(gp_Pnt p; gp_Vec d; c.D1(t[i], p, d); o_pnt(p); o_vec(d)); }
}
template <class A> static void o_ad_c2(const A& c) {
  double f = 0, l = 0, t[5];
  SNAP(f = c.FirstParameter(); hb(f)); SNAP(l = c.LastParameter(); hb(l));
  SNAP(ob(c.IsClosed())); SNAP(ob(c.IsPeriodic())); SNAP(in((long)static_cast<int>(c.Continuity())));
  snap_params(f, l, t);
  for (int i = 0; i < 5; ++i) { SNAP(gp_Pnt2d p; c.D0(t[i], p); o_pnt2d(p)); }
  for (int i = 0; i < 5; ++i) { SNAP(gp_Pnt2d p; gp_Vec2d d; c.D1(t[i], p, d); o_pnt2d(p); o_vec2d(d)); }
}
template <class A> static void o_ad_s3(const A& s) {
  double u1 = 0, u2 = 0, v1 = 0, v2 = 0, u[4], v[4];
  SNAP(u1 = s.FirstUParameter(); u2 = s.LastUParameter(); v1 = s.FirstVParameter(); v2 = s.LastVParameter(); hb(u1); hb(u2); hb(v1); hb(v2));
  SNAP(ob(s.IsUClosed())); SNAP(ob(s.IsVClosed())); SNAP(ob(s.IsUPeriodic())); SNAP(ob(s.IsVPeriodic()));
  SNAP(in((long)static_cast<int>(s.UContinuity()))); SNAP(in((long)static_cast<int>(s.VContinuity())));
  snap_uv(u1, u2, v1, v2, u, v);
  for (int i = 0; i < 4; ++i) { SNAP(gp_Pnt p; s.D0(u[i], v[i], p); o_pnt(p)); }
  for (int i = 0; i < 4; ++i) { SNAP(gp_Pnt p; gp_Vec a, b; s.D1(u[i], v[i], p, a, b); o_pnt(p); o_vec(a); o_vec(b)); }
}
'''

CPP_BX = r'''
// ----- Bnd_Box / Bnd_Box2d arguments (a call fills a box: the box is a raw input and is printed after the call)
//   bx_Bnd_Box: kind (0 void, 1 Update) then 6 floats when kind is 1, gap;  bx_Bnd_Box2d: kind, 4 floats, gap
static Raw g_bx(const char* w, int dim) {
  RB b; bool full = coin(0.45);
  b.i(full ? 1 : 0);
  if (full) {
    double lo[3], hi[3];
    for (int k = 0; k < dim; ++k) { lo[k] = crd(); hi[k] = lo[k] + (coin(0.1) ? 0.0 : rnd(0.1, 4)); }
    for (int k = 0; k < dim; ++k) b.f(lo[k]);
    for (int k = 0; k < dim; ++k) b.f(hi[k]);
  }
  b.f(coin(0.6) ? 0.0 : rnd(0, 0.5));
  return Raw{w, b.pat, b.v};
}
static Bnd_Box mk_bx(const Raw& r) {
  Bnd_Box b; const Vd& v = r.v;
  if (v.at(0) != 0) b.Update(v.at(1), v.at(2), v.at(3), v.at(4), v.at(5), v.at(6));
  b.SetGap(v.at(v.size() - 1));
  return b;
}
static Bnd_Box2d mk_bx2(const Raw& r) {
  Bnd_Box2d b; const Vd& v = r.v;
  if (v.at(0) != 0) b.Update(v.at(1), v.at(2), v.at(3), v.at(4));
  b.SetGap(v.at(v.size() - 1));
  return b;
}
static void o_bx(const Bnd_Box& b) {
  ob(b.IsVoid());
  if (!b.IsVoid()) { double x[6]; b.Get(x[0], x[1], x[2], x[3], x[4], x[5]); for (double d : x) h(d); h(b.GetGap()); }
}
static void o_bx2(const Bnd_Box2d& b) {
  ob(b.IsVoid());
  if (!b.IsVoid()) { double x[4]; b.Get(x[0], x[1], x[2], x[3]); for (double d : x) h(d); h(b.GetGap()); }
}
'''

CPP_DOM = r'''
// ----- MathUtils::Domain1D / Domain2D arguments (min max; umin umax vmin vmax)
static Raw g_dom(const char* w, int n) {
  RB b;
  for (int k = 0; k < n; k += 2) {
    double lo = crd(), hi = lo + (coin(0.08) ? -rnd(0, 1) : (coin(0.05) ? 0.0 : rnd(0.2, 7)));
    b.f(lo); b.f(hi);
  }
  return Raw{w, b.pat, b.v};
}
static MathUtils::Domain1D mk_dom1(const Raw& r) { return MathUtils::Domain1D(r.v.at(0), r.v.at(1)); }
static MathUtils::Domain2D mk_dom2(const Raw& r) { return MathUtils::Domain2D(r.v.at(0), r.v.at(1), r.v.at(2), r.v.at(3)); }
static void o_dom1(const MathUtils::Domain1D& d) { h(d.Min); h(d.Max); }
static void o_dom2(const MathUtils::Domain2D& d) { h(d.UMin); h(d.UMax); h(d.VMin); h(d.VMax); }
'''

H_HEADERS = ['Geom_Line', 'Geom_Circle', 'Geom_Ellipse', 'Geom_Hyperbola', 'Geom_Parabola', 'Geom_BezierCurve', 'Geom_BSplineCurve',
             'Geom_TrimmedCurve', 'Geom_OffsetCurve', 'Geom_Plane', 'Geom_CylindricalSurface', 'Geom_ConicalSurface',
             'Geom_SphericalSurface', 'Geom_ToroidalSurface', 'Geom_BezierSurface', 'Geom_BSplineSurface',
             'Geom_SurfaceOfLinearExtrusion', 'Geom_SurfaceOfRevolution', 'Geom_RectangularTrimmedSurface', 'Geom_OffsetSurface',
             'Geom2d_Line', 'Geom2d_Circle', 'Geom2d_Ellipse', 'Geom2d_Hyperbola', 'Geom2d_Parabola', 'Geom2d_BezierCurve',
             'Geom2d_BSplineCurve', 'Geom2d_TrimmedCurve', 'Geom2d_OffsetCurve', 'GeomAdaptor_Curve', 'GeomAdaptor_Surface',
             'Geom2dAdaptor_Curve', 'Adaptor3d_Curve', 'Adaptor3d_Surface', 'Adaptor2d_Curve2d', 'gp_Ax22d', 'gp_Lin2d',
             'gp_Circ2d', 'gp_Elips2d', 'gp_Hypr2d', 'gp_Parab2d']
H_LIBS = ['TKGeomBase', 'TKG3d', 'TKG2d']


# --------------------------------------------------------------------------------------------------------------
# C++ generation of the families

HINT = {'GEN': 'H_GEN', 'TOL': 'H_TOL', 'ANG': 'H_ANG', 'POS': 'H_POS', 'PAR': 'H_PAR', 'IDX': 'H_IDX', 'DEG': 'H_DEG',
        'UNIT': 'H_UNIT'}


def stride_of(e):
    return 1 if e in ('f', 'i') else GP[e][2]


def cpp_type(k):
    t = k[0]
    if t == 'f':
        return 'double'
    if t == 'i':
        return 'int'
    if t == 'b':
        return 'bool'
    if t == 'enum':
        return k[1]
    if t == 'gp':
        return GP[k[1]][0]
    if t == 'arr':
        return 'NCollection_Array1<%s>' % ELEM_CPP[k[1]]
    if t == 'arr2':
        return 'NCollection_Array2<%s>' % ELEM_CPP[k[1]]
    if t == 'cls':
        return 'std::unique_ptr<%s>' % k[1]
    if t in ('h', 'ad'):
        return 'occ::handle<%s>' % k[1]
    if t in ('bx', 'dom'):
        return k[1]
    raise ValueError(k)


def arg_expr(p, j, padded=True):
    """How the variable aJ of an `in` parameter is passed to the call (a gp value is `Padded` in the families)."""
    k = p.kind
    if k[0] == 'cls' or (k[0] == 'gp' and padded):
        return '*a%d' % j
    if k[0] == 'ad' and not p.info.get('handle'):
        return '*a%d' % j
    return 'a%d' % j


def gen_stmt(p, B='B'):
    """C++ statement generating (and printing) the raw inputs of an `in` parameter."""
    k = p.kind
    t = k[0]
    if t == 'f':
        return 'put(%s, g_f(%s));' % (B, HINT[p.hint])
    if t == 'i':
        return 'put(%s, g_i(%s));' % (B, HINT[p.hint])
    if t == 'b':
        return 'put(%s, g_b());' % B
    if t == 'enum':
        return 'put(%s, g_enum("e_%s", %d));' % (B, k[1], len(ENUM_N[k[1]]))
    if t == 'gp':
        return 'put(%s, g_gp("%s", %s.last("%s")));' % (B, k[1], B, k[1])
    if t == 'arr':
        return 'put(%s, g_arr("%s", 1));' % (B, k[1])
    if t == 'arr2':
        return 'put(%s, g_arr("%s", 2));' % (B, k[1])
    if t == 'cls':
        return '{ int s_ = ntok; ++g_nested; gen_obj_%s(%s, -1); --g_nested; word("obj_%s", s_, -1); }' % (k[1], B, k[1])
    if t == 'bx':
        return 'put(%s, g_bx("bx_%s", %d));' % (B, k[1], 3 if k[1] == 'Bnd_Box' else 2)
    if t == 'dom':
        return 'put(%s, g_dom("dom_%s", %d));' % (B, k[1].replace('::', '_'), len(DOMAINS[k[1]][1]))
    if t == 'h':
        fam, _, kinds, _ = H_INFO[k[1]]
        ks = '{' + ', '.join(str(x) for x in kinds) + '}'
        if fam == 's3':
            return 'put(%s, g_hs(%s, "h_%s", %s));' % (B, B, k[1], ks)
        return 'put(%s, g_hc(%s, "h_%s", %d, %s));' % (B, B, k[1], 3 if fam == 'c3' else 2, ks)
    if t == 'ad':
        fam = AD_INFO[k[1]][0]
        if fam == 'as3':
            return 'put(%s, g_ads(%s, "ad_%s"));' % (B, B, k[1])
        return 'put(%s, g_adc(%s, "ad_%s", %d));' % (B, B, k[1], 3 if fam == 'ac3' else 2)
    raise ValueError(k)


def mk_expr(k, C='C'):
    t = k[0]
    if t in ('f', 'i', 'b'):
        return 'mk_%s(%s.next())' % (t, C)
    if t == 'enum':
        return 'static_cast<%s>((int)%s.next().v.at(0))' % (k[1], C)
    if t == 'gp':
        return 'mk_%s(%s.next())' % (k[1], C)
    if t == 'arr':
        e = k[1]
        return 'mk_arr1<%s, mkp_%s>(%s.next(), %d)' % (ELEM_CPP[e], e, C, stride_of(e))
    if t == 'arr2':
        e = k[1]
        return 'mk_arr2<%s, mkp_%s>(%s.next(), %d)' % (ELEM_CPP[e], e, C, stride_of(e))
    if t == 'cls':
        return 'mk_obj_%s(%s)' % (k[1], C)
    if t == 'bx':
        return 'mk_bx%s(%s.next())' % ('' if k[1] == 'Bnd_Box' else '2', C)
    if t == 'dom':
        return 'mk_dom%d(%s.next())' % (len(DOMAINS[k[1]][1]) // 2, C)
    if t == 'h':
        fam = H_INFO[k[1]][0]
        fld, bld = {'c3': ('cv', 'build_c'), 'c2': ('cv', 'build_c2'), 's3': ('sv', 'build_s')}[fam]
        return 'hdown<%s>(%s(*%s.next().%s))' % (k[1], bld, C, fld)
    if t == 'ad':
        return {'ac3': 'mk_adc3', 'ac2': 'mk_adc2', 'as3': 'mk_ads3'}[AD_INFO[k[1]][0]] + '(%s.next())' % C
    raise ValueError(k)


def print_stmt(k, var):
    t = k[0]
    if t == 'f':
        return 'o_f(%s);' % var
    if t == 'i':
        return 'o_i((long)%s);' % var
    if t == 'b':
        return 'o_b(%s);' % var
    if t == 'enum':
        return 'o_i((long)static_cast<int>(%s));' % var
    if t == 'gp':
        return 'o_%s(%s);' % (k[1], var)
    if t == 'arr':
        return 'o_arr_%s(%s);' % (k[1], var)
    if t == 'arr2':
        return 'o_arr2_%s(%s);' % (k[1], var)
    if t == 'u':
        return 'o_u((unsigned long long)%s);' % var
    if t == 'dom':
        return 'o_dom%d(%s);' % (len(DOMAINS[k[1]][1]) // 2, var)
    if t == 'ns':
        return 'o_ns_%s(%s);' % (mname(k[1]), var)
    if t == 'bx':
        return 'o_bx%s(%s);' % ('' if k[1] == 'Bnd_Box' else '2', var)
    if t == 'st':
        return 'o_st_%s(%s);' % (mname(k[1]), var)
    if t == 'sarr':
        return 'o_sarr%d(%s, o_st_%s);' % (k[2], var, mname(k[1]))
    if t == 'h':
        return 'o_h_%s(%s);' % (H_INFO[k[1]][0], var)
    if t == 'ad':
        f = AD_INFO[k[1]][0]
        if len(k) > 2 and k[2]:
            return 'if (%s.IsNull()) in(0); else { in(1); o_ad_%s(*%s); }' % (var, {'ac3': 'c3', 'ac2': 'c2', 'as3': 's3'}[f], var)
        return '{ in(1); o_ad_%s(%s); }' % ({'ac3': 'c3', 'ac2': 'c2', 'as3': 's3'}[f], var)
    raise ValueError(k)


ENUM_N = {}


def obj_helpers(c):
    """gen_obj_<C> and mk_obj_<C> for a drivable class."""
    n = c.name
    ctors = c.drv_ctors
    out = ['static void gen_obj_%s(Bag& B, int sel) {' % n,
           '  int k = sel >= 0 ? sel %% %d : rint(0, %d);' % (len(ctors), len(ctors) - 1),
           '  put(B, Raw{"obj_idx", "i", {(double)k}});',
           '  switch (k) {']
    for i, f in enumerate(ctors):
        out.append('    case %d: {' % i)
        for p in (f.params if f else []):
            out.append('      ' + gen_stmt(p))
        out.append('      break; }')
    out.append('  }')
    out.append('}')
    out.append('static std::unique_ptr<%s> mk_obj_%s(Cursor& C) {' % (n, n))
    out.append('  int k = (int)C.next().v.at(0);')
    out.append('  switch (k) {')
    for i, f in enumerate(ctors):
        out.append('    case %d: {' % i)
        args = []
        for j, p in enumerate(f.params if f else []):
            out.append('      %s a%d = %s;' % (cpp_type(p.kind), j, mk_expr(p.kind)))
            args.append(arg_expr(p, j, False))
        out.append('      return std::unique_ptr<%s>(new %s(%s)); }' % (n, n, ', '.join(args)))
    out.append('  }')
    out.append('  throw Discard();')
    out.append('}')
    return '\n'.join(out)


def snapshot_code(plan, var):
    out = []
    for g in plan.getters:
        out.append('    try { auto&& g_ = %s->%s(); %s } catch (Standard_Failure&) { ox(); }' % (
            var, g.cname, print_stmt(g.ret, 'g_')))
    return out


def family_code(idx, plan, m):
    f = plan.fn
    N = plan.name
    cands = plan.cands
    lines = ['static void fam_%d() {   // %s' % (idx, N), '  const char* N = "%s"; reg(N); seed_for(N);' % N]
    if cands:
        lines.append('  bool io[%d];' % len(f.params))
        for j in range(len(f.params)):
            lines.append('  io[%d] = %s;' % (j, 'inout_flag(N, %d)' % j if j in cands else 'false'))
    elif True:
        lines.append('  if (g_probe) return;')
    nctor = 1
    if plan.kind == 'ctor_obj':
        nctor = len(plan.cls.drv_ctors)
    lim = 'g_cases * %d' % min(nctor, 4)
    lines.append('  for (int att = 0; fam_wanted(N) && fam_hangs[N] < 2 && att < (g_probe ? 24 : MAXATT * %d) && fam_lines[N] < %s; ++att) {' % (
        max(nctor, 1), lim))
    lines.append('    g_att = att;')
    if cands:
        lines.append('    std::string schema;')
        sch = []
        if plan.recv:
            sch.append('    schema += " %s";' % word_of_recv(plan.recv))
        for j, p in enumerate(f.params):
            if p.mode == 'in':
                sch.append('    schema += " %s";' % word_of(p.kind))
            elif j in cands:
                sch.append('    if (io[%d]) schema += " %s";' % (j, word_of(p.kind)))
        lines.extend(sch)
        lines.append('    begin(N, schema);')
    else:
        lines.append('    begin(N, "%s");' % ' '.join(plan_words(plan)))
    lines.append('    Bag B;')
    gen = []
    if plan.kind == 'ctor_obj':
        gen.append('{ int s_ = ntok; ++g_nested; gen_obj_%s(B, att); --g_nested; word("obj_%s", s_, -1); }' % (
            plan.cls.name, plan.cls.name))
    else:
        if plan.recv:
            if plan.recv[0] == 'gp':
                gen.append('put(B, g_gp("%s", nullptr));' % plan.recv[1])
            else:
                c = plan.recv[1]
                gen.append('{ int s_ = ntok; ++g_nested; gen_obj_%s(B, -1); --g_nested; word("obj_%s", s_, -1); }' % (
                    c.name, c.name))
        for j, p in enumerate(f.params):
            if p.mode == 'in':
                gen.append(gen_stmt(p))
            elif j in cands:
                gen.append('if (io[%d]) %s' % (j, gen_stmt(p)))
    for g in gen:
        lines.append('    ' + g)
    lines.append('    bar();')
    lines.append('    std::function<void()> bodyf = [&] {')
    lines.append('      Cursor C(B); (void)C;')
    body = []
    if plan.kind == 'ctor_obj':
        body.append('      g_call_phase = true;')
        body.append('      auto s = mk_obj_%s(C);' % plan.cls.name)
    elif plan.recv:
        if plan.recv[0] == 'gp':
            body.append('      Padded<%s> s(mk_%s(C.next()));' % (GP[plan.recv[1]][0], plan.recv[1]))
        else:
            body.append('      auto s = mk_obj_%s(C);' % plan.recv[1].name)
    args = []
    outs = []
    for j, p in enumerate(f.params if f else []):
        if p.mode == 'in':
            if p.kind[0] == 'gp':
                body.append('      Padded<%s> a%d(%s);' % (cpp_type(p.kind), j, mk_expr(p.kind)))
            else:
                body.append('      %s a%d = %s;' % (cpp_type(p.kind), j, mk_expr(p.kind)))
            args.append(arg_expr(p, j))
        else:
            init = {'f': ' = 0', 'i': ' = 0', 'b': ' = false'}.get(p.kind[0], '')
            body.append('      %s o%d%s;' % (cpp_type(p.kind), j, init))
            if j in cands:
                t = p.kind[0]
                body.append('      if (io[%d]) o%d = %s;' % (j, j, mk_expr(p.kind)))
                pert = {'f': 'o%d = o%d * 1.37 + 0.911;', 'i': 'o%d += 3;', 'b': 'o%d = !o%d;'}[t]
                body.append('      if (g_probe_j == %d) { %s }' % (j, pert % ((j, j) if t != 'i' else (j,))))
            args.append('o%d' % j)
            outs.append((j, p))
    al = ', '.join(args)
    body.append('      g_call_phase = true;')
    if plan.kind == 'ctor_obj':
        pass
    elif plan.kind == 'ctor_gp':
        body.append('      %s r = %s(%s);' % (plan.cls.name, plan.cls.name, al))
        body.append('      ' + print_stmt(('gp', GP_BY_CLASS[plan.cls.name]), 'r'))
    else:
        if f.kind == 'static':
            callee = '%s::%s' % (plan.cls.name, f.cname)
        elif f.kind == 'free':
            callee = f.cname
        else:
            obj = '(*s)'
            callee = ('std::as_const(%s).%s' if f.const else '%s.%s') % (obj, f.cname)
        if f.ret[0] == 'void':
            body.append('      %s(%s);' % (callee, al))
        else:
            body.append('      auto&& r = %s(%s);' % (callee, al))
            body.append('      ' + print_stmt(f.ret, 'r'))
        for j, p in outs:
            body.append('      ' + print_stmt(p.kind, 'o%d' % j))
        for j, p in enumerate(f.params):
            if p.mutref:
                body.append('      ' + print_stmt(p.kind, 'a%d' % j))
        if plan.mutates:
            if plan.recv[0] == 'gp':
                body.append('      ' + print_stmt(('gp', plan.recv[1]), '(*s)'))
    if plan.getters:
        body.extend(snapshot_code(plan, 's'))
    lines.extend(body)
    lines.append('    };')
    if cands:
        lines.append('    if (g_probe) { for (int j = 0; j < %d; ++j) if (io[j] && probe_diff(bodyf, j)) g_probe_hits.insert(std::string(N) + ":" + std::to_string(j)); continue; }' % len(f.params))
    lines.append('    emit(bodyf);')
    lines.append('  }')
    lines.append('}')
    return '\n'.join(lines)


BUILD_NOTE = ['']


def build_cpp(m, plans, excluded, cases, skip_default, hdrs, enum_hdrs, inout_default=()):
    """-> (text, {family: (first line, last line)}, {class: (first line, last line)} of the object helpers)"""
    ctx = m.ctx
    inc = ''.join('#include <%s>\n' % h for h in hdrs) + ''.join('#include <%s.hxx>\n' % e for e in sorted(enum_hdrs))
    if ctx.saw_handle:
        inc += ''.join('#include <%s.hxx>\n' % h for h in H_HEADERS)
    if ctx.saw_bx:
        inc += '#include <Bnd_Box.hxx>\n#include <Bnd_Box2d.hxx>\n'
    if ctx.saw_dom:
        inc += '#include <MathUtils_Domain.hxx>\n'
    pre = CPP_PRELUDE.replace('@@HANDLES@@', (CPP_HANDLES if ctx.saw_handle else '') + (CPP_BX if ctx.saw_bx else '') + (CPP_DOM if ctx.saw_dom else '')).replace('@@BUILDNOTE@@', BUILD_NOTE[0]).replace('@@PKG@@', m.pkg).replace('@@INCLUDES@@', inc.rstrip('\n')).replace('@@CASES@@', str(cases))
    parts = [pre]
    if ctx.used_ns:
        pr_ = []
        for sn in ctx.used_ns:
            body_ = []
            for fname, qt in ctx.structs[sn]:
                ma = re.match(r'^NCollection_(DynamicArray|Array1|Vector)<\s*([\w:]+)\s*>$', qt.strip())
                if ma:
                    en = ma.group(2)
                    en = en if en in ctx.structs else m.pkg + '::' + en
                    first = 'a_.Lower() + i_' if ma.group(1) == 'Array1' else 'i_'
                    body_.append('{ const auto& a_ = v.%s; in(1); in((long)a_.Length()); for (int i_ = 0; i_ < a_.Length(); ++i_) o_ns_%s(a_.Value(%s)); }' % (fname, mname(en), first))
                    continue
                t_ = split_type(qt)['base']
                if t_ in ctx.structs:
                    body_.append('o_ns_%s(v.%s);' % (mname(t_), fname))
                    continue
                k_ = ctx.classify(qt)[0]
                body_.append(print_stmt(k_, 'v.' + fname) if k_[0] != 'enum' else 'in((long)static_cast<int>(v.%s));' % fname)
            pr_.append('static void o_ns_%s(const %s& v) { %s }' % (mname(sn), sn, ' '.join(body_)))
        parts.append('\n'.join(pr_))
    if ctx.used_structs:
        pr_ = ['template <class A, class F> static void o_sarr1(const A& a, F f) { in(a.Lower()); in(a.Upper()); for (int i = a.Lower(); i <= a.Upper(); ++i) f(a(i)); }',
               'template <class A, class F> static void o_sarr2(const A& a, F f) { in(a.LowerRow()); in(a.UpperRow()); in(a.LowerCol()); in(a.UpperCol()); for (int i = a.LowerRow(); i <= a.UpperRow(); ++i) for (int j = a.LowerCol(); j <= a.UpperCol(); ++j) f(a(i, j)); }']
        for sn in sorted(ctx.used_structs):
            pr_.append('static void o_st_%s(const %s& v) { %s }' % (mname(sn), sn, ' '.join('o_%s(v.%s);' % (w, cf) for cf, rf, w in STRUCTS[sn][1])))
        parts.append('\n'.join(pr_))
    used = [c for c in ctx.classes.values() if c.name in ctx.drivable]
    for c in used:
        parts.append('static void gen_obj_%s(Bag& B, int sel);' % c.name)
        parts.append('static std::unique_ptr<%s> mk_obj_%s(Cursor& C);' % (c.name, c.name))
    line = '\n'.join(parts).count('\n') + 2
    obj_ranges = {}
    for c in used:
        code = obj_helpers(c)
        n = code.count('\n') + 1
        obj_ranges[c.name] = (line, line + n - 1)
        line += n
        parts.append(code)
    text_so_far = '\n'.join(parts) + '\n'
    line = text_so_far.count('\n') + 1
    ranges = {}
    calls = []
    fams = []
    for i, p in enumerate(plans):
        if p.name in excluded:
            continue
        code = family_code(i, p, m)
        n = code.count('\n') + 1
        ranges[p.name] = (line, line + n - 1)
        line += n + 1
        fams.append(code)
        calls.append('fam_%d();' % i)
    main = CPP_MAIN.replace('@@INOUT@@', ','.join(sorted(inout_default))).replace('@@SKIP@@', ','.join(sorted(skip_default))).replace('@@CALLS@@', '\n  '.join(calls))
    return text_so_far + '\n\n'.join(fams) + '\n' + main, ranges, obj_ranges


# --------------------------------------------------------------------------------------------------------------
# Rust test generation

RS_HEAD = r'''//! GENERATED by tools/judgegen.py -- numerical reference test for OCCT 8.0.1's package `@@PKG@@` (module `@@MOD@@`).
//!
//! `@@MOD@@.txt` is printed by `oracle/@@MOD@@.cpp` (real OCCT). Each line is `<family> <inputs...> | <outputs...>`:
//! f64 values are 16-hex-digit bit patterns, ints are decimal, bools are 0/1 and an enum is its ordinal. Every input of
//! a call is on the left of ` | `; this test reads them back with the `Rd` reader methods named after the oracle's
//! schema words and replays the call on the Rust API of docs/api-@@MOD@@.md. The right side is the expected values or
//! `throws` (OCCT raised a Standard_Failure; the Rust API must return an error). The snapshot of an object (its
//! zero-argument const getters, read after the call) puts an `x` where a getter raised. A family none of whose cases
//! calls a C library transcendental function is compared bit for bit (`EXACT`), the others within `MAX_ULPS`.
//! Every call may return the plain value or a `Result`; an `Err` is the Rust counterpart of `throws`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
#![allow(dead_code, unused_imports, unused_mut, unused_variables, clippy::clone_on_copy, clippy::needless_borrow)]
#![allow(clippy::needless_borrows_for_generic_args, clippy::let_unit_value, clippy::unit_arg, clippy::useless_conversion)]

@@USES@@
use std::collections::{BTreeMap, BTreeSet, HashSet};

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for the families that use the C library's sin / cos / ... , in units of f64::EPSILON times
/// the largest absolute value on the line (at least 1; zero tolerance if that value is infinite). The oracle only prints
/// cases whose outputs stay within 2 ulps when every such result is moved by an ulp.
const MAX_ULPS: f64 = 8.0;

/// Every family with inputs must have at least this many distinct lines (except the `SMALL_DOMAIN` ones).
const MIN_DISTINCT: usize = 16;

// BEGIN GENERATED
@@CONSTS@@
// END GENERATED
'''

RS_BODY = r'''
// ---------------------------------------------------------------------------------
// Results of one replayed call

/// The Rust API returned an error (or the inputs are invalid).
#[derive(Debug)]
struct Thrown;

#[derive(Clone, Debug)]
enum Tok {
    F(f64),
    B(bool),
    I(i64),
    S(String),
}

/// Collects the values produced by the Rust side, in the order the oracle printed them.
#[derive(Default)]
struct Got(Vec<Tok>);

impl Got {
    fn f(&mut self, v: f64) {
        self.0.push(Tok::F(v));
    }
    fn b(&mut self, v: bool) {
        self.0.push(Tok::B(v));
    }
    fn i(&mut self, v: i32) {
        self.0.push(Tok::I(i64::from(v)));
    }
    fn l(&mut self, v: usize) {
        self.0.push(Tok::I(v as i64));
    }
    /// A getter of an object snapshot that raised.
    fn x(&mut self) {
        self.0.push(Tok::S("x".to_string()));
    }
    fn put<T: Emit + ?Sized>(&mut self, v: &T) {
        v.emit(self);
    }
}

/// A value the oracle prints.
trait Emit {
    fn emit(&self, g: &mut Got);
}
impl<T: Emit + ?Sized> Emit for &T {
    fn emit(&self, g: &mut Got) {
        (**self).emit(g);
    }
}
impl<T: Emit + ?Sized> Emit for &mut T {
    fn emit(&self, g: &mut Got) {
        (**self).emit(g);
    }
}
impl Emit for f64 {
    fn emit(&self, g: &mut Got) {
        g.f(*self);
    }
}
impl Emit for bool {
    fn emit(&self, g: &mut Got) {
        g.b(*self);
    }
}
impl Emit for i32 {
    fn emit(&self, g: &mut Got) {
        g.i(*self);
    }
}
impl Emit for usize {
    fn emit(&self, g: &mut Got) {
        g.0.push(Tok::S(format!("h{:016x}", *self as u64)));
    }
}
impl Plain for usize {}
impl Emit for () {
    fn emit(&self, _: &mut Got) {}
}
/// An array: lower bound (always 1), upper bound, the values.
impl<T: Emit> Emit for Vec<T> {
    fn emit(&self, g: &mut Got) {
        g.i(1);
        g.l(self.len());
        for x in self {
            x.emit(g);
        }
    }
}
impl<T: Emit> Emit for [T] {
    fn emit(&self, g: &mut Got) {
        g.i(1);
        g.l(self.len());
        for x in self {
            x.emit(g);
        }
    }
}
/// A two-dimensional array: lower and upper row, lower and upper column, the values row by row.
impl<T: Emit> Emit for Array2<T> {
    fn emit(&self, g: &mut Got) {
        g.i(1);
        g.l(self.rows());
        g.i(1);
        g.l(self.cols());
        for x in self.as_slice() {
            x.emit(g);
        }
    }
}
macro_rules! tuple_emit {
    ($($t:ident $i:tt),+) => {
        impl<$($t: Emit),+> Emit for ($($t,)+) {
            fn emit(&self, g: &mut Got) {
                $( self.$i.emit(g); )+
            }
        }
    };
}
tuple_emit!(A 0);
tuple_emit!(A 0, B 1);
tuple_emit!(A 0, B 1, C 2);
tuple_emit!(A 0, B 1, C 2, D 3);
tuple_emit!(A 0, B 1, C 2, D 3, E 4);
tuple_emit!(A 0, B 1, C 2, D 3, E 4, F 5);
tuple_emit!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
tuple_emit!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
tuple_emit!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
tuple_emit!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);
tuple_emit!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10);
tuple_emit!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);

/// Types a call may return (so that `Result<T, E>` and a plain `T` both work).
trait Plain {}
impl Plain for f64 {}
impl Plain for bool {}
impl Plain for i32 {}
impl Plain for () {}
impl<T: Plain + ?Sized> Plain for &T {}
impl<T: Plain + ?Sized> Plain for &mut T {}
impl<T: Plain> Plain for Vec<T> {}
impl<T: Plain> Plain for [T] {}
impl<T: Plain> Plain for Array2<T> {}
impl<A: Plain> Plain for (A,) {}
impl<A: Plain, B: Plain> Plain for (A, B) {}
impl<A: Plain, B: Plain, C: Plain> Plain for (A, B, C) {}
impl<A: Plain, B: Plain, C: Plain, D: Plain> Plain for (A, B, C, D) {}
impl<A: Plain, B: Plain, C: Plain, D: Plain, E: Plain> Plain for (A, B, C, D, E) {}
impl<A: Plain, B: Plain, C: Plain, D: Plain, E: Plain, F: Plain> Plain for (A, B, C, D, E, F) {}
impl<A: Plain, B: Plain, C: Plain, D: Plain, E: Plain, F: Plain, G: Plain> Plain for (A, B, C, D, E, F, G) {}
impl<A: Plain, B: Plain, C: Plain, D: Plain, E: Plain, F: Plain, G: Plain, H: Plain> Plain for (A, B, C, D, E, F, G, H) {}
impl<A: Plain, B: Plain, C: Plain, D: Plain, E: Plain, F: Plain, G: Plain, H: Plain, I: Plain> Plain for (A, B, C, D, E, F, G, H, I) {}
impl<A: Plain, B: Plain, C: Plain, D: Plain, E: Plain, F: Plain, G: Plain, H: Plain, I: Plain, J: Plain> Plain for (A, B, C, D, E, F, G, H, I, J) {}
impl<A: Plain, B: Plain, C: Plain, D: Plain, E: Plain, F: Plain, G: Plain, H: Plain, I: Plain, J: Plain, K: Plain> Plain for (A, B, C, D, E, F, G, H, I, J, K) {}
impl<A: Plain, B: Plain, C: Plain, D: Plain, E: Plain, F: Plain, G: Plain, H: Plain, I: Plain, J: Plain, K: Plain, L: Plain> Plain for (A, B, C, D, E, F, G, H, I, J, K, L) {}

trait Lift {
    type O;
    fn lift(self) -> Result<Self::O, Thrown>;
}
impl<T: Plain> Lift for T {
    type O = T;
    fn lift(self) -> Result<T, Thrown> {
        Ok(self)
    }
}
impl<T: Plain, E> Lift for Result<T, E> {
    type O = T;
    fn lift(self) -> Result<T, Thrown> {
        self.map_err(|_| Thrown)
    }
}

@@EMITS@@

// ---------------------------------------------------------------------------------
// Reading the inputs of a line

fn bits(w: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(w, 16).unwrap_or_else(|e| panic!("bad hex word {w}: {e}")))
}

struct Rd<'a> {
    words: &'a [&'a str],
    at: usize,
}

impl<'a> Rd<'a> {
    fn word(&mut self) -> &'a str {
        let w = *self.words.get(self.at).unwrap_or_else(|| panic!("input word {} missing", self.at));
        self.at += 1;
        w
    }
    fn f(&mut self) -> f64 {
        bits(self.word())
    }
    fn i(&mut self) -> i32 {
        self.word().parse().expect("integer input")
    }
    fn b(&mut self) -> bool {
        self.i() != 0
    }
    fn count(&mut self) -> usize {
        self.word().parse().expect("count input")
    }
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread inputs");
    }
@@READERS@@
}

fn run(name: &str, r: &mut Rd, g: &mut Got) -> Result<(), Thrown> {
    match name {
@@ARMS@@
        _ => panic!("unknown oracle family {name}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Comparison

fn ulps(a: f64, b: f64) -> u64 {
    let key = |x: f64| {
        let n = x.to_bits() as i64;
        if n < 0 { i64::MIN - n } else { n }
    };
    key(a).abs_diff(key(b))
}

#[derive(Default)]
struct Tally {
    values: usize,
    exact: usize,
    worst: u64,
}

/// Compares the expected words of a line with the replayed values. Floats are compared bit for bit for `EXACT` families and
/// within `MAX_ULPS` of the line's largest magnitude otherwise; bools, ints and texts always exactly.
fn compare(tally: &mut Tally, line: &str, name: &str, want: &[&str], got: &[Tok]) {
    assert_eq!(want.len(), got.len(), "value count differs\n{line}");
    let exact = EXACT.contains(&name);
    let mut scale = 1.0_f64;
    for (w, t) in want.iter().zip(got) {
        if let Tok::F(_) = t {
            scale = scale.max(bits(w).abs());
        }
    }
    let tol = if scale.is_finite() { MAX_ULPS * f64::EPSILON * scale } else { 0.0 };
    for (index, (w, t)) in want.iter().zip(got).enumerate() {
        match t {
            Tok::F(g) => {
                let w = bits(w);
                let d = ulps(w, *g);
                let ok =
                    if exact { w.to_bits() == g.to_bits() } else { (w - g).abs() <= tol || w.to_bits() == g.to_bits() };
                assert!(ok, "value {index}: want {w:e}, got {g:e} ({d} ulps)\n{line}");
                tally.values += 1;
                tally.exact += usize::from(d == 0);
                tally.worst = tally.worst.max(d);
            }
            Tok::B(g) => assert_eq!(*w, if *g { "1" } else { "0" }, "value {index} (bool)\n{line}"),
            Tok::I(g) => assert_eq!(w.parse::<i64>().expect("integer output"), *g, "value {index} (int)\n{line}"),
            Tok::S(g) => assert_eq!(*w, g.as_str(), "value {index} (text)\n{line}"),
        }
    }
}

// ---------------------------------------------------------------------------------

/// Replays one line (panics with the line on the first mismatch).
fn check_line(tally: &mut Tally, line: &str) {
    let (head, tail) = line.split_once(" |").unwrap_or_else(|| panic!("line without ' |': {line}"));
    let head: Vec<&str> = head.split_whitespace().collect();
    let want: Vec<&str> = tail.split_whitespace().collect();
    let (name, inputs) = head.split_first().expect("family name");
    let name: &str = name;
    let mut reader = Rd { words: inputs, at: 0 };
    let mut got = Got::default();
    let outcome = run(name, &mut reader, &mut got);
    if outcome.is_ok() {
        reader.finish();
    }
    let marker = matches!(want.first(), Some(&"throws"));
    match outcome {
        Err(Thrown) => assert!(marker, "Rust API returned an error but OCCT returned values\n{line}"),
        Ok(()) => {
            assert!(!marker, "Rust API returned values but OCCT raised\n{line}");
            compare(tally, line, name, &want, &got.0);
        }
    }
}

#[test]
fn @@MOD@@_matches_occt() {
    let text = include_str!("@@MOD@@.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let (mut n_throws, mut n_ok) = (0usize, 0usize);
    // JUDGE_ALL=1 reports every failing family instead of stopping at the first mismatch (for triage)
    let keep_going = std::env::var_os("JUDGE_ALL").is_some();
    let mut failures: BTreeMap<String, (usize, String)> = BTreeMap::new();
    if keep_going {
        std::panic::set_hook(Box::new(|_| {}));
    }

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in @@MOD@@.txt:\n{line}");
        let (head, tail) = line.split_once(" |").unwrap_or_else(|| panic!("line without ' |': {line}"));
        let head: Vec<&str> = head.split_whitespace().collect();
        let (name, inputs) = head.split_first().expect("family name");
        let name: &str = name;
        let entry = stats.entry(name).or_default();
        entry.0 += 1;
        entry.1 += usize::from(!inputs.is_empty());
        if tail.trim() == "throws" {
            n_throws += 1;
        } else {
            n_ok += 1;
        }
        if keep_going {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check_line(&mut tally, line)));
            if let Err(e) = r {
                let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                let f = failures.entry(name.to_string()).or_insert((0, msg.chars().take(400).collect()));
                f.0 += 1;
            }
        } else {
            check_line(&mut tally, line);
        }
    }
    if !failures.is_empty() {
        let _ = std::panic::take_hook();
        for (name, (n, msg)) in &failures {
            eprintln!("FAIL {name} ({n} lines): {}", msg.replace('\n', " || "));
        }
        panic!("{} families have mismatching lines", failures.len());
    }

    // Input coverage: no duplicates (checked above), every family with inputs has at least MIN_DISTINCT distinct lines (the
    // families of SMALL_DOMAIN have all the inputs their domain has), and only the listed families are input-less.
    for (name, (count, with_inputs)) in &stats {
        if INPUT_LESS.contains(name) {
            // either no input at all or only the constructor index of an object built by a parameterless constructor
            assert_eq!(*count, 1, "input-less family {name} must be printed exactly once");
        } else if let Some((_, size)) = SMALL_DOMAIN.iter().find(|(n, _)| n == name) {
            assert_eq!(*with_inputs, *count, "family {name} lost its inputs");
            assert_eq!(*count, *size, "family {name} must print its whole input domain ({size} lines)");
        } else {
            assert_eq!(*with_inputs, *count, "family {name} lost its inputs");
            assert!(*count >= MIN_DISTINCT, "family {name} has only {count} distinct lines");
        }
    }
    for name in INPUT_LESS {
        assert!(stats.contains_key(name), "input-less family {name} missing");
    }
    // Every family is still covered.
    let present: BTreeSet<&str> = stats.keys().copied().collect();
    let expected: BTreeSet<&str> = FAMILIES.iter().copied().collect();
    assert_eq!(present, expected, "family list changed");

    assert_eq!(lines, text.lines().count());
    assert_eq!(lines, GOLDEN_LINES, "golden line count changed");
    println!(
        "{lines} lines ({n_throws} throws, {n_ok} values lines), {} values, {} bit-identical, worst {} ulps",
        tally.values, tally.exact, tally.worst
    );
}
'''
# fixed readers and emitters of the gp words
RS_GP_READERS = r'''
    fn pnt(&mut self) -> Result<Pnt, Thrown> {
        let (x, y, z) = (self.f(), self.f(), self.f());
        Ok(Pnt::new(x, y, z))
    }
    fn vec(&mut self) -> Result<GpVec, Thrown> {
        let (x, y, z) = (self.f(), self.f(), self.f());
        Ok(GpVec::new(x, y, z))
    }
    fn xyz(&mut self) -> Result<XYZ, Thrown> {
        let (x, y, z) = (self.f(), self.f(), self.f());
        Ok(XYZ::new(x, y, z))
    }
    /// A raw (un-normalized) direction; `Dir::new` normalizes it exactly like the oracle did.
    fn dir(&mut self) -> Result<Dir, Thrown> {
        let (x, y, z) = (self.f(), self.f(), self.f());
        Dir::new(x, y, z).lift()
    }
    /// Location and a raw direction.
    fn ax1(&mut self) -> Result<Ax1, Thrown> {
        let p = self.pnt();
        let d = self.dir();
        Ok(Ax1::new(p?, d?))
    }
    /// Location, raw main direction and raw X direction.
    fn ax2(&mut self) -> Result<Ax2, Thrown> {
        let p = self.pnt();
        let n = self.dir();
        let x = self.dir();
        Ax2::new(p?, n?, &x?).lift()
    }
    /// An `ax2` and a handedness flag (0 direct, 1 X reversed, 2 Y reversed).
    fn ax3(&mut self) -> Result<Ax3, Thrown> {
        let a = self.ax2();
        let ind = self.i();
        let mut a3 = Ax3::from_ax2(&a?);
        match ind {
            1 => a3.x_reverse(),
            2 => a3.y_reverse(),
            _ => {}
        }
        Ok(a3)
    }
    fn lin(&mut self) -> Result<Lin, Thrown> {
        let a = self.ax1();
        Ok(Lin::from_axis(a?))
    }
    fn circ(&mut self) -> Result<Circ, Thrown> {
        let a = self.ax2();
        let r = self.f();
        Circ::new(a?, r).lift()
    }
    fn elips(&mut self) -> Result<Elips, Thrown> {
        let a = self.ax2();
        let (major, minor) = (self.f(), self.f());
        Elips::new(a?, major, minor).lift()
    }
    fn hypr(&mut self) -> Result<Hypr, Thrown> {
        let a = self.ax2();
        let (major, minor) = (self.f(), self.f());
        Hypr::new(a?, major, minor).lift()
    }
    fn parab(&mut self) -> Result<Parab, Thrown> {
        let a = self.ax2();
        let focal = self.f();
        Parab::new(a?, focal).lift()
    }
    fn pln(&mut self) -> Result<Pln, Thrown> {
        let a = self.ax3();
        Ok(Pln::new(a?))
    }
    fn cylinder(&mut self) -> Result<Cylinder, Thrown> {
        let a = self.ax3();
        let r = self.f();
        Cylinder::new(a?, r).lift()
    }
    /// Frame, semi-angle, reference radius.
    fn cone(&mut self) -> Result<Cone, Thrown> {
        let a = self.ax3();
        let (angle, r) = (self.f(), self.f());
        Cone::new(a?, angle, r).lift()
    }
    fn sphere(&mut self) -> Result<Sphere, Thrown> {
        let a = self.ax3();
        let r = self.f();
        Sphere::new(a?, r).lift()
    }
    fn torus(&mut self) -> Result<Torus, Thrown> {
        let a = self.ax3();
        let (major, minor) = (self.f(), self.f());
        Torus::new(a?, major, minor).lift()
    }
    /// Nine values, row-major.
    fn mat(&mut self) -> Result<Mat, Thrown> {
        let mut v = [0.0; 9];
        for e in &mut v {
            *e = self.f();
        }
        Ok(Mat::from_values(v))
    }
    /// Components x y z w, exactly as printed (not normalized).
    fn quat(&mut self) -> Result<Quaternion, Thrown> {
        let (x, y, z, w) = (self.f(), self.f(), self.f(), self.f());
        Ok(Quaternion::from_components(x, y, z, w))
    }
    /// A transformation recipe: kind 0 rotation (axis, angle), 2 scale (point, factor), 3 translation (vector), 4 mirror
    /// about an axis, 5 mirror about a point, 6 mirror about the plane through a point with a raw normal.
    fn trsf(&mut self) -> Result<Trsf, Thrown> {
        let kind = self.i();
        let mut t = Trsf::new();
        match kind {
            0 => {
                let axis = self.ax1();
                let angle = self.f();
                t.set_rotation(&axis?, angle).lift()?;
            }
            2 => {
                let p = self.pnt();
                let factor = self.f();
                t.set_scale(&p?, factor).lift()?;
            }
            3 => {
                let v = self.vec();
                t.set_translation(&v?).lift()?;
            }
            4 => {
                let a = self.ax1();
                t.set_mirror(&a?).lift()?;
            }
            5 => {
                let p = self.pnt();
                t.set_mirror(&p?).lift()?;
            }
            6 => {
                let p = self.pnt();
                let d = self.dir();
                t.set_mirror(&Ax2::from_direction(p?, d?)).lift()?;
            }
            _ => panic!("unknown transformation recipe {kind}"),
        }
        Ok(t)
    }
    fn pnt2d(&mut self) -> Result<Pnt2d, Thrown> {
        let (x, y) = (self.f(), self.f());
        Ok(Pnt2d::new(x, y))
    }
    fn vec2d(&mut self) -> Result<Vec2d, Thrown> {
        let (x, y) = (self.f(), self.f());
        Ok(Vec2d::new(x, y))
    }
    fn xy(&mut self) -> Result<XY, Thrown> {
        let (x, y) = (self.f(), self.f());
        Ok(XY::new(x, y))
    }
    fn dir2d(&mut self) -> Result<Dir2d, Thrown> {
        let (x, y) = (self.f(), self.f());
        Dir2d::new(x, y).lift()
    }
    fn ax2d(&mut self) -> Result<Ax2d, Thrown> {
        let p = self.pnt2d();
        let d = self.dir2d();
        Ok(Ax2d::new(p?, d?))
    }
    /// Location, raw X direction and a second raw direction that only selects the handedness.
    fn ax22d(&mut self) -> Result<Ax22d, Thrown> {
        let p = self.pnt2d();
        let vx = self.dir2d();
        let vy = self.dir2d();
        Ok(Ax22d::from_dirs(p?, vx?, vy?))
    }
    fn lin2d(&mut self) -> Result<Lin2d, Thrown> {
        let a = self.ax2d();
        Ok(Lin2d::from_ax2d(a?))
    }
    fn circ2d(&mut self) -> Result<Circ2d, Thrown> {
        let a = self.ax22d();
        let r = self.f();
        Circ2d::from_ax22d(a?, r).lift()
    }
    fn elips2d(&mut self) -> Result<Elips2d, Thrown> {
        let a = self.ax22d();
        let (major, minor) = (self.f(), self.f());
        Elips2d::from_ax22d(a?, major, minor).lift()
    }
    fn hypr2d(&mut self) -> Result<Hypr2d, Thrown> {
        let a = self.ax22d();
        let (major, minor) = (self.f(), self.f());
        Hypr2d::from_ax22d(a?, major, minor).lift()
    }
    fn parab2d(&mut self) -> Result<Parab2d, Thrown> {
        let a = self.ax22d();
        let focal = self.f();
        Parab2d::from_ax22d(a?, focal).lift()
    }
    /// Four values, row-major.
    fn mat2d(&mut self) -> Result<Mat2d, Thrown> {
        let mut v = [0.0; 4];
        for e in &mut v {
            *e = self.f();
        }
        Ok(Mat2d::from_columns(XY::new(v[0], v[2]), XY::new(v[1], v[3])))
    }
    /// kind 0 rotation (point, angle), 2 scale (point, factor), 3 translation (vector), 5 mirror about a point, 6 mirror
    /// about an axis.
    fn trsf2d(&mut self) -> Result<Trsf2d, Thrown> {
        let kind = self.i();
        let mut t = Trsf2d::new();
        match kind {
            0 => {
                let p = self.pnt2d();
                let angle = self.f();
                t.set_rotation(&p?, angle).lift()?;
            }
            2 => {
                let p = self.pnt2d();
                let factor = self.f();
                t.set_scale(&p?, factor).lift()?;
            }
            3 => {
                let v = self.vec2d();
                t.set_translation(&v?).lift()?;
            }
            5 => {
                let p = self.pnt2d();
                t.set_mirror(&p?).lift()?;
            }
            6 => {
                let a = self.ax2d();
                t.set_mirror(&a?).lift()?;
            }
            _ => panic!("unknown 2D transformation recipe {kind}"),
        }
        Ok(t)
    }
'''

RS_GP_EMITS = r'''
macro_rules! plain {
    ($($t:ty),+) => { $( impl Plain for $t {} )+ };
}
plain!(XYZ, Pnt, GpVec, Dir, Ax1, Ax2, Ax3, Lin, Circ, Elips, Hypr, Parab, Pln, Cylinder, Cone, Sphere, Torus, Mat, Trsf);
plain!(Quaternion, Pnt2d, Vec2d, Dir2d, XY, Ax2d, Ax22d, Lin2d, Circ2d, Elips2d, Hypr2d, Parab2d, Mat2d, Trsf2d);

impl Emit for XYZ {
    fn emit(&self, g: &mut Got) {
        g.f(self.x());
        g.f(self.y());
        g.f(self.z());
    }
}
impl Emit for Pnt {
    fn emit(&self, g: &mut Got) {
        self.xyz().emit(g);
    }
}
impl Emit for GpVec {
    fn emit(&self, g: &mut Got) {
        self.xyz().emit(g);
    }
}
impl Emit for Dir {
    fn emit(&self, g: &mut Got) {
        self.xyz().emit(g);
    }
}
impl Emit for Ax1 {
    fn emit(&self, g: &mut Got) {
        self.location().emit(g);
        self.direction().emit(g);
    }
}
impl Emit for Ax2 {
    fn emit(&self, g: &mut Got) {
        self.location().emit(g);
        self.direction().emit(g);
        self.x_direction().emit(g);
        self.y_direction().emit(g);
    }
}
impl Emit for Ax3 {
    fn emit(&self, g: &mut Got) {
        self.location().emit(g);
        self.direction().emit(g);
        self.x_direction().emit(g);
        self.y_direction().emit(g);
    }
}
impl Emit for Lin {
    fn emit(&self, g: &mut Got) {
        self.position().emit(g);
    }
}
impl Emit for Circ {
    fn emit(&self, g: &mut Got) {
        self.position().emit(g);
        g.f(self.radius());
    }
}
impl Emit for Elips {
    fn emit(&self, g: &mut Got) {
        self.position().emit(g);
        g.f(self.major_radius());
        g.f(self.minor_radius());
    }
}
impl Emit for Hypr {
    fn emit(&self, g: &mut Got) {
        self.position().emit(g);
        g.f(self.major_radius());
        g.f(self.minor_radius());
    }
}
impl Emit for Parab {
    fn emit(&self, g: &mut Got) {
        self.position().emit(g);
        g.f(self.focal());
    }
}
impl Emit for Pln {
    fn emit(&self, g: &mut Got) {
        self.position().emit(g);
    }
}
impl Emit for Cylinder {
    fn emit(&self, g: &mut Got) {
        self.position().emit(g);
        g.f(self.radius());
    }
}
impl Emit for Cone {
    fn emit(&self, g: &mut Got) {
        self.position().emit(g);
        g.f(self.semi_angle());
        g.f(self.ref_radius());
    }
}
impl Emit for Sphere {
    fn emit(&self, g: &mut Got) {
        self.position().emit(g);
        g.f(self.radius());
    }
}
impl Emit for Torus {
    fn emit(&self, g: &mut Got) {
        self.position().emit(g);
        g.f(self.major_radius());
        g.f(self.minor_radius());
    }
}
impl Emit for Mat {
    fn emit(&self, g: &mut Got) {
        for r in 1..=3 {
            for c in 1..=3 {
                g.f(self.value(r, c).expect("valid index"));
            }
        }
    }
}
impl Emit for Trsf {
    fn emit(&self, g: &mut Got) {
        for r in 1..=3 {
            for c in 1..=4 {
                g.f(self.value(r, c).expect("valid index"));
            }
        }
    }
}
impl Emit for Quaternion {
    fn emit(&self, g: &mut Got) {
        g.f(self.x());
        g.f(self.y());
        g.f(self.z());
        g.f(self.w());
    }
}
impl Emit for XY {
    fn emit(&self, g: &mut Got) {
        g.f(self.x());
        g.f(self.y());
    }
}
impl Emit for Pnt2d {
    fn emit(&self, g: &mut Got) {
        self.xy().emit(g);
    }
}
impl Emit for Vec2d {
    fn emit(&self, g: &mut Got) {
        self.xy().emit(g);
    }
}
impl Emit for Dir2d {
    fn emit(&self, g: &mut Got) {
        self.xy().emit(g);
    }
}
impl Emit for Ax2d {
    fn emit(&self, g: &mut Got) {
        self.location().emit(g);
        self.direction().emit(g);
    }
}
impl Emit for Ax22d {
    fn emit(&self, g: &mut Got) {
        self.location().emit(g);
        self.x_direction().emit(g);
        self.y_direction().emit(g);
    }
}
impl Emit for Lin2d {
    fn emit(&self, g: &mut Got) {
        self.position().emit(g);
    }
}
impl Emit for Circ2d {
    fn emit(&self, g: &mut Got) {
        self.position().emit(g);
        g.f(self.radius());
    }
}
impl Emit for Elips2d {
    fn emit(&self, g: &mut Got) {
        self.axis().emit(g);
        g.f(self.major_radius());
        g.f(self.minor_radius());
    }
}
impl Emit for Hypr2d {
    fn emit(&self, g: &mut Got) {
        self.axis().emit(g);
        g.f(self.major_radius());
        g.f(self.minor_radius());
    }
}
impl Emit for Parab2d {
    fn emit(&self, g: &mut Got) {
        self.axis().emit(g);
        g.f(self.focal());
    }
}
impl Emit for Mat2d {
    fn emit(&self, g: &mut Got) {
        for r in 1..=2 {
            for c in 1..=2 {
                g.f(self.value(r, c).expect("valid index"));
            }
        }
    }
}
impl Emit for Trsf2d {
    fn emit(&self, g: &mut Got) {
        for r in 1..=2 {
            for c in 1..=3 {
                g.f(self.value(r, c).expect("valid index"));
            }
        }
    }
}
'''

RS_GP_USES = '''use occt_port::gp::{
    self, Ax1, Ax2, Ax22d, Ax2d, Ax3, Circ, Circ2d, Cone, Cylinder, Dir, Dir2d, Elips, Elips2d, Hypr, Hypr2d, Lin, Lin2d,
    Mat, Mat2d, Parab, Parab2d, Pln, Pnt, Pnt2d, Quaternion, Sphere, Torus, Trsf, Trsf2d, Vec as GpVec, Vec2d, XY, XYZ,
};
use occt_port::plib::Array2;'''


RS_H_COMMON = r'''// ---------------------------------------------------------------------------------
// Handles of curves and surfaces (recipes of the reviewed judges tests/geom.rs and tests/geom2d.rs) and their snapshots
use std::sync::Arc;

impl From<occt_port::gp::ConstructionError> for Thrown {
    fn from(_: occt_port::gp::ConstructionError) -> Self {
        Thrown
    }
}
impl From<occt_port::geom::GeomError> for Thrown {
    fn from(_: occt_port::geom::GeomError) -> Self {
        Thrown
    }
}
impl From<occt_port::geom2d::Geom2dError> for Thrown {
    fn from(_: occt_port::geom2d::Geom2dError) -> Self {
        Thrown
    }
}

impl<T: Plain + ?Sized> Plain for Option<&T> {}
impl<T: Emit + ?Sized> Emit for Option<&T> {
    fn emit(&self, g: &mut Got) {
        match self {
            Some(x) => x.emit(g),
            None => g.i(0),
        }
    }
}

/// A parameter bound: 1 and the value when finite, 0 otherwise.
fn hb(g: &mut Got, x: f64) {
    if x.abs() < 1e50 {
        g.i(1);
        g.f(x);
    } else {
        g.i(0);
    }
}

/// The five parameters of a curve snapshot.
fn snap_params(f: f64, l: f64) -> [f64; 5] {
    if f.abs() < 1e50 && l.abs() < 1e50 {
        std::array::from_fn(|k| f + (l - f) * (k as f64 * 0.25))
    } else {
        [-2.0, -1.0, 0.25, 1.0, 2.0]
    }
}

/// The four (u, v) points of a surface snapshot.
fn snap_uv(u1: f64, u2: f64, v1: f64, v2: f64) -> ([f64; 4], [f64; 4]) {
    const K: [[f64; 2]; 4] = [[0.0, 0.0], [0.25, 0.75], [0.5, 0.5], [1.0, 1.0]];
    let fu = u1.abs() < 1e50 && u2.abs() < 1e50;
    let fv = v1.abs() < 1e50 && v2.abs() < 1e50;
    let u = std::array::from_fn(|k| if fu { u1 + (u2 - u1) * K[k][0] } else { -2.0 + k as f64 });
    let v = std::array::from_fn(|k| if fv { v1 + (v2 - v1) * K[k][1] } else { 2.0 - k as f64 });
    (u, v)
}

/// Emits a value that may be missing: an `Err` prints `x` (the oracle's getter / evaluation raised).
fn okx<T: Emit, E>(g: &mut Got, r: Result<T, E>) {
    match r {
        Ok(v) => g.put(&v),
        Err(_) => g.x(),
    }
}
'''

RS_H_C3 = r'''fn conc3<T: Clone + 'static>(c: &dyn occt_port::geom::Curve) -> Result<Arc<T>, Thrown> {
    use occt_port::geom::Geometry as _;
    c.as_any().downcast_ref::<T>().map(|x| Arc::new(x.clone())).ok_or(Thrown)
}
fn conc3s<T: Clone + 'static>(c: &dyn occt_port::geom::Surface) -> Result<Arc<T>, Thrown> {
    use occt_port::geom::Geometry as _;
    c.as_any().downcast_ref::<T>().map(|x| Arc::new(x.clone())).ok_or(Thrown)
}
fn conic3(b: Box<dyn occt_port::geom::Curve>) -> Result<Arc<dyn occt_port::geom::Conic>, Thrown> {
    use occt_port::geom::{Circle, Ellipse, Geometry as _, Hyperbola, Parabola};
    let a = b.as_any();
    if let Some(x) = a.downcast_ref::<Circle>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<Ellipse>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<Hyperbola>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<Parabola>() {
        return Ok(Arc::new(x.clone()));
    }
    Err(Thrown)
}
fn bounded3(b: Box<dyn occt_port::geom::Curve>) -> Result<Arc<dyn occt_port::geom::BoundedCurve>, Thrown> {
    use occt_port::geom::{BSplineCurve, BezierCurve, Geometry as _, TrimmedCurve};
    let a = b.as_any();
    if let Some(x) = a.downcast_ref::<BezierCurve>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<BSplineCurve>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<TrimmedCurve>() {
        return Ok(Arc::new(x.clone()));
    }
    Err(Thrown)
}
fn elementary3(b: Box<dyn occt_port::geom::Surface>) -> Result<Arc<dyn occt_port::geom::ElementarySurface>, Thrown> {
    use occt_port::geom::{ConicalSurface, CylindricalSurface, Geometry as _, Plane, SphericalSurface, ToroidalSurface};
    let a = b.as_any();
    if let Some(x) = a.downcast_ref::<Plane>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<CylindricalSurface>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<ConicalSurface>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<SphericalSurface>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<ToroidalSurface>() {
        return Ok(Arc::new(x.clone()));
    }
    Err(Thrown)
}
fn swept3(b: Box<dyn occt_port::geom::Surface>) -> Result<Arc<dyn occt_port::geom::SweptSurface>, Thrown> {
    use occt_port::geom::{Geometry as _, SurfaceOfLinearExtrusion, SurfaceOfRevolution};
    let a = b.as_any();
    if let Some(x) = a.downcast_ref::<SurfaceOfLinearExtrusion>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<SurfaceOfRevolution>() {
        return Ok(Arc::new(x.clone()));
    }
    Err(Thrown)
}
fn bounded3s(b: Box<dyn occt_port::geom::Surface>) -> Result<Arc<dyn occt_port::geom::BoundedSurface>, Thrown> {
    use occt_port::geom::{BSplineSurface, BezierSurface, Geometry as _, RectangularTrimmedSurface};
    let a = b.as_any();
    if let Some(x) = a.downcast_ref::<BezierSurface>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<BSplineSurface>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<RectangularTrimmedSurface>() {
        return Ok(Arc::new(x.clone()));
    }
    Err(Thrown)
}

/// The snapshot of a `Geom_Curve`: 1 (0 for a null handle), the bounds, periodicity, continuity, `EvalD0` and `EvalD1` at five
/// parameters. The evaluations print `x` when they fail.
fn snap_c3(g: &mut Got, k: &dyn occt_port::geom::Curve) {
    g.i(1);
    let (f, l) = (k.first_parameter(), k.last_parameter());
    hb(g, f);
    hb(g, l);
    g.b(k.is_periodic());
    g.i(k.continuity() as i32);
    let t = snap_params(f, l);
    for &u in &t {
        okx(g, k.eval_d0(u));
    }
    for &u in &t {
        match k.eval_d1(u) {
            Ok(r) => {
                g.put(&r.point);
                g.put(&r.d1);
            }
            Err(_) => g.x(),
        }
    }
}

/// The snapshot of a `Geom_Surface`: 1, the bounds, closed / periodic in U and V, continuity, `EvalD0` and `EvalD1` at four points.
fn snap_s3(g: &mut Got, k: &dyn occt_port::geom::Surface) {
    g.i(1);
    let (u1, u2, v1, v2) = k.bounds();
    hb(g, u1);
    hb(g, u2);
    hb(g, v1);
    hb(g, v2);
    g.b(k.is_u_closed());
    g.b(k.is_v_closed());
    g.b(k.is_u_periodic());
    g.b(k.is_v_periodic());
    g.i(k.continuity() as i32);
    let (u, v) = snap_uv(u1, u2, v1, v2);
    for i in 0..4 {
        okx(g, k.eval_d0(u[i], v[i]));
    }
    for i in 0..4 {
        match k.eval_d1(u[i], v[i]) {
            Ok(r) => {
                g.put(&r.point);
                g.put(&r.d1u);
                g.put(&r.d1v);
            }
            Err(_) => g.x(),
        }
    }
}
'''

RS_H_C2 = r'''fn conc2<T: Clone + 'static>(c: &dyn occt_port::geom2d::Curve2d) -> Result<Arc<T>, Thrown> {
    use occt_port::geom2d::Geometry2d as _;
    c.as_any().downcast_ref::<T>().map(|x| Arc::new(x.clone())).ok_or(Thrown)
}
fn conic2(b: Box<dyn occt_port::geom2d::Curve2d>) -> Result<Arc<dyn occt_port::geom2d::Conic2d>, Thrown> {
    use occt_port::geom2d::{Circle, Ellipse, Geometry2d as _, Hyperbola, Parabola};
    let a = b.as_any();
    if let Some(x) = a.downcast_ref::<Circle>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<Ellipse>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<Hyperbola>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<Parabola>() {
        return Ok(Arc::new(x.clone()));
    }
    Err(Thrown)
}
fn bounded2(b: Box<dyn occt_port::geom2d::Curve2d>) -> Result<Arc<dyn occt_port::geom2d::BoundedCurve2d>, Thrown> {
    use occt_port::geom2d::{BSplineCurve, BezierCurve, Geometry2d as _, TrimmedCurve};
    let a = b.as_any();
    if let Some(x) = a.downcast_ref::<BezierCurve>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<BSplineCurve>() {
        return Ok(Arc::new(x.clone()));
    }
    if let Some(x) = a.downcast_ref::<TrimmedCurve>() {
        return Ok(Arc::new(x.clone()));
    }
    Err(Thrown)
}

/// The snapshot of a `Geom2d_Curve` (as `snap_c3`).
fn snap_c2(g: &mut Got, k: &dyn occt_port::geom2d::Curve2d) {
    g.i(1);
    let (f, l) = (k.first_parameter(), k.last_parameter());
    hb(g, f);
    hb(g, l);
    g.b(k.is_periodic());
    g.i(k.continuity() as i32);
    let t = snap_params(f, l);
    for &u in &t {
        okx(g, k.eval_d0(u));
    }
    for &u in &t {
        match k.eval_d1(u) {
            Ok(r) => {
                g.put(&r.point);
                g.put(&r.d1);
            }
            Err(_) => g.x(),
        }
    }
}
'''

RS_H_AC3 = r'''/// The snapshot of an `Adaptor3d_Curve`.
fn snap_ad_c3(g: &mut Got, k: &dyn occt_port::adaptor3d::Curve3dAdaptor) {
    g.i(1);
    let (f, l) = (k.first_parameter().ok(), k.last_parameter().ok());
    for v in [f, l] {
        match v {
            Some(x) => hb(g, x),
            None => g.x(),
        }
    }
    okx(g, k.is_closed());
    okx(g, k.is_periodic());
    okx(g, k.continuity().map(|c| c as i32));
    let t = snap_params(f.unwrap_or(0.0), l.unwrap_or(0.0));
    for &u in &t {
        okx(g, k.d0(u));
    }
    for &u in &t {
        okx(g, k.d1(u));
    }
}
'''

RS_H_AC2 = r'''/// The snapshot of an `Adaptor2d_Curve2d`.
fn snap_ad_c2(g: &mut Got, k: &dyn occt_port::adaptor2d::Curve2dAdaptor) {
    g.i(1);
    let (f, l) = (k.first_parameter().ok(), k.last_parameter().ok());
    for v in [f, l] {
        match v {
            Some(x) => hb(g, x),
            None => g.x(),
        }
    }
    okx(g, k.is_closed());
    okx(g, k.is_periodic());
    okx(g, k.continuity().map(|c| c as i32));
    let t = snap_params(f.unwrap_or(0.0), l.unwrap_or(0.0));
    for &u in &t {
        okx(g, k.d0(u));
    }
    for &u in &t {
        okx(g, k.d1(u));
    }
}
'''

RS_H_AS3 = r'''/// The snapshot of an `Adaptor3d_Surface`.
fn snap_ad_s3(g: &mut Got, k: &dyn occt_port::adaptor3d::SurfaceAdaptor) {
    g.i(1);
    let b = [k.first_u_parameter().ok(), k.last_u_parameter().ok(), k.first_v_parameter().ok(), k.last_v_parameter().ok()];
    for v in b {
        match v {
            Some(x) => hb(g, x),
            None => g.x(),
        }
    }
    okx(g, k.is_u_closed());
    okx(g, k.is_v_closed());
    okx(g, k.is_u_periodic());
    okx(g, k.is_v_periodic());
    okx(g, k.u_continuity().map(|c| c as i32));
    okx(g, k.v_continuity().map(|c| c as i32));
    let (u, v) = snap_uv(b[0].unwrap_or(0.0), b[1].unwrap_or(0.0), b[2].unwrap_or(0.0), b[3].unwrap_or(0.0));
    for i in 0..4 {
        okx(g, k.d0(u[i], v[i]));
    }
    for i in 0..4 {
        match k.d1(u[i], v[i]) {
            Ok(r) => {
                g.put(&r.point);
                g.put(&r.d1u);
                g.put(&r.d1v);
            }
            Err(_) => g.x(),
        }
    }
}
'''

RS_H_MODS3 = r'''mod h3 {
    use super::{bits, Rd, Thrown};
    use occt_port::geom::{BSplineCurve, BSplineSurface, BezierCurve, BezierSurface, Circle, ConicalSurface, Curve, CylindricalSurface, Ellipse, Hyperbola, Line, OffsetCurve, OffsetSurface, Parabola, Plane, RectangularTrimmedSurface, SphericalSurface, Surface, SurfaceOfLinearExtrusion, SurfaceOfRevolution, ToroidalSurface, TrimmedCurve};
    use occt_port::gp::{Ax1, Ax2, Ax3, Circ, Cone, Cylinder, Dir, Elips, Hypr, Lin, Parab, Pln, Pnt, Sphere, Torus};
    use std::sync::Arc;

    type R<T = ()> = Result<T, Thrown>;

    /// `crv`: a curve recipe.
    pub(super) enum CurveSpec {
        /// Kinds 0..=4: line, circle, ellipse, hyperbola, parabola.
        Basic {
            kind: i32,
            ctor: i32,
            v: [f64; 9],
            a: f64,
            b: f64,
        },
        Bezier {
            poles: Vec<Pnt>,
            weights: Option<Vec<f64>>,
        },
        BSpline {
            degree: i32,
            periodic: bool,
            knots: Vec<f64>,
            mults: Vec<i32>,
            poles: Vec<Pnt>,
            weights: Option<Vec<f64>>,
        },
        Trimmed {
            u1: f64,
            u2: f64,
            sense: bool,
            adjust: bool,
            basis: Box<CurveSpec>,
        },
        Offset {
            offset: f64,
            not_check_c0: bool,
            dir: [f64; 3],
            basis: Box<CurveSpec>,
        },
    }

    /// `srf`: a surface recipe.
    pub(super) enum SurfSpec {
        Plane {
            ctor: i32,
            fr: [f64; 10],
            abcd: [f64; 4],
        },
        Cylinder {
            ctor: i32,
            fr: [f64; 10],
            r: f64,
        },
        Cone {
            ctor: i32,
            fr: [f64; 10],
            ang: f64,
            r: f64,
        },
        Sphere {
            ctor: i32,
            fr: [f64; 10],
            r: f64,
        },
        Torus {
            ctor: i32,
            fr: [f64; 10],
            major: f64,
            minor: f64,
        },
        Bezier {
            nu: usize,
            nv: usize,
            poles: Vec<Pnt>,
            weights: Option<Vec<f64>>,
        },
        BSpline {
            udeg: i32,
            vdeg: i32,
            uper: bool,
            vper: bool,
            uk: Vec<f64>,
            um: Vec<i32>,
            vk: Vec<f64>,
            vm: Vec<i32>,
            nu: usize,
            nv: usize,
            poles: Vec<Pnt>,
            weights: Option<Vec<f64>>,
        },
        Extrusion {
            dir: [f64; 3],
            curve: Box<CurveSpec>,
        },
        Revolution {
            axis: [f64; 6],
            curve: Box<CurveSpec>,
        },
        Trimmed {
            ctor: i32,
            t: [f64; 4],
            s1: bool,
            s2: bool,
            utrim: bool,
            basis: Box<SurfSpec>,
        },
        Offset {
            offset: f64,
            not_check_c0: bool,
            basis: Box<SurfSpec>,
        },
    }

    impl Rd<'_> {
        pub(super) fn fl3<const N: usize>(&mut self) -> [f64; N] {
            let mut a = [0.0; N];
            for v in &mut a {
                *v = self.f();
            }
            a
        }

        pub(super) fn pnts3_n(&mut self, n: usize) -> Vec<Pnt> {
            (0..n)
                .map(|_| {
                    let x = self.f();
                    let y = self.f();
                    let z = self.f();
                    Pnt::new(x, y, z)
                })
                .collect()
        }

        pub(super) fn pnts3(&mut self) -> Vec<Pnt> {
            let n = self.count();
            self.pnts3_n(n)
        }

        pub(super) fn wts3(&mut self) -> Option<Vec<f64>> {
            if self.i() == 0 {
                return None;
            }
            let n = self.count();
            Some((0..n).map(|_| self.f()).collect())
        }

        /// `crv`.
        pub(super) fn crv(&mut self) -> CurveSpec {
            let kind = self.i();
            match kind {
                0..=4 => {
                    let ctor = self.i();
                    let mut v = [0.0; 9];
                    let (mut a, mut b) = (0.0, 0.0);
                    if kind == 0 {
                        for x in v.iter_mut().take(6) {
                            *x = self.f();
                        }
                    } else {
                        for x in &mut v {
                            *x = self.f();
                        }
                        if kind == 1 || (kind == 4 && ctor != 2) {
                            a = self.f();
                        }
                        if kind == 2 || kind == 3 {
                            a = self.f();
                            b = self.f();
                        }
                    }
                    CurveSpec::Basic { kind, ctor, v, a, b }
                }
                5 => {
                    let poles = self.pnts3();
                    let weights = self.wts3();
                    CurveSpec::Bezier { poles, weights }
                }
                6 => {
                    let degree = self.i();
                    let periodic = self.i() != 0;
                    let nk = self.count();
                    let knots = (0..nk).map(|_| self.f()).collect();
                    let nm = self.count();
                    let mults = (0..nm).map(|_| self.i()).collect();
                    let poles = self.pnts3();
                    let weights = self.wts3();
                    CurveSpec::BSpline { degree, periodic, knots, mults, poles, weights }
                }
                7 => {
                    let u1 = self.f();
                    let u2 = self.f();
                    let sense = self.i() != 0;
                    let adjust = self.i() != 0;
                    CurveSpec::Trimmed { u1, u2, sense, adjust, basis: Box::new(self.crv()) }
                }
                8 => {
                    let offset = self.f();
                    let not_check_c0 = self.i() != 0;
                    let dir = self.fl3::<3>();
                    CurveSpec::Offset { offset, not_check_c0, dir, basis: Box::new(self.crv()) }
                }
                k => panic!("bad curve kind {k}"),
            }
        }

        /// `srf`.
        pub(super) fn srf(&mut self) -> SurfSpec {
            let kind = self.i();
            match kind {
                0 => {
                    let ctor = self.i();
                    let (mut fr, mut abcd) = ([0.0; 10], [0.0; 4]);
                    match ctor {
                        0 | 1 => fr = self.fl3::<10>(),
                        2 => {
                            let v = self.fl3::<6>();
                            fr[..6].copy_from_slice(&v);
                        }
                        _ => abcd = self.fl3::<4>(),
                    }
                    SurfSpec::Plane { ctor, fr, abcd }
                }
                1 => {
                    let ctor = self.i();
                    let fr = self.fl3::<10>();
                    SurfSpec::Cylinder { ctor, fr, r: self.f() }
                }
                2 => {
                    let ctor = self.i();
                    let fr = self.fl3::<10>();
                    let ang = self.f();
                    SurfSpec::Cone { ctor, fr, ang, r: self.f() }
                }
                3 => {
                    let ctor = self.i();
                    let fr = self.fl3::<10>();
                    SurfSpec::Sphere { ctor, fr, r: self.f() }
                }
                4 => {
                    let ctor = self.i();
                    let fr = self.fl3::<10>();
                    let major = self.f();
                    SurfSpec::Torus { ctor, fr, major, minor: self.f() }
                }
                5 => {
                    let nu = self.count();
                    let nv = self.count();
                    let poles = self.pnts3_n(nu * nv);
                    let weights = self.wts3();
                    SurfSpec::Bezier { nu, nv, poles, weights }
                }
                6 => {
                    let udeg = self.i();
                    let vdeg = self.i();
                    let uper = self.i() != 0;
                    let vper = self.i() != 0;
                    let nku = self.count();
                    let uk = (0..nku).map(|_| self.f()).collect();
                    let nmu = self.count();
                    let um = (0..nmu).map(|_| self.i()).collect();
                    let nkv = self.count();
                    let vk = (0..nkv).map(|_| self.f()).collect();
                    let nmv = self.count();
                    let vm = (0..nmv).map(|_| self.i()).collect();
                    let nu = self.count();
                    let nv = self.count();
                    let poles = self.pnts3_n(nu * nv);
                    let weights = self.wts3();
                    SurfSpec::BSpline { udeg, vdeg, uper, vper, uk, um, vk, vm, nu, nv, poles, weights }
                }
                7 => {
                    let dir = self.fl3::<3>();
                    SurfSpec::Extrusion { dir, curve: Box::new(self.crv()) }
                }
                8 => {
                    let axis = self.fl3::<6>();
                    SurfSpec::Revolution { axis, curve: Box::new(self.crv()) }
                }
                9 => {
                    let ctor = self.i();
                    let (t, s1, s2, utrim);
                    if ctor == 0 {
                        t = self.fl3::<4>();
                        s1 = self.i() != 0;
                        s2 = self.i() != 0;
                        utrim = true;
                    } else {
                        let (a, b) = (self.f(), self.f());
                        t = [a, b, 0.0, 0.0];
                        utrim = self.i() != 0;
                        s1 = self.i() != 0;
                        s2 = true;
                    }
                    SurfSpec::Trimmed { ctor, t, s1, s2, utrim, basis: Box::new(self.srf()) }
                }
                10 => {
                    let offset = self.f();
                    let not_check_c0 = self.i() != 0;
                    SurfSpec::Offset { offset, not_check_c0, basis: Box::new(self.srf()) }
                }
                k => panic!("bad surface kind {k}"),
            }
        }

    }

    pub(super) fn ax3_of(f: &[f64]) -> R<Ax3> {
        let g = |k: usize| f.get(k).copied().unwrap_or(0.0);
        let mut r = Ax3::new(Pnt::new(g(0), g(1), g(2)), Dir::new(g(3), g(4), g(5))?, Dir::new(g(6), g(7), g(8))?)?;
        if g(9).round() as i32 != 0 {
            r.y_reverse();
        }
        Ok(r)
    }

    fn ax2_v(v: &[f64; 9]) -> R<Ax2> {
        Ok(Ax2::new(Pnt::new(v[0], v[1], v[2]), Dir::new(v[3], v[4], v[5])?, &Dir::new(v[6], v[7], v[8])?)?)
    }

    /// `build` of the oracle.
    pub(super) fn build(c: &CurveSpec) -> R<Box<dyn Curve>> {
        Ok(match c {
            CurveSpec::Basic { kind, ctor, v, a, b } => {
                let o = Pnt::new(v[0], v[1], v[2]);
                match *kind {
                    0 => {
                        let d = Dir::new(v[3], v[4], v[5])?;
                        match ctor {
                            0 => Box::new(Line::from_ax1(Ax1::new(o, d))),
                            1 => Box::new(Line::from_lin(Lin::from_axis(Ax1::new(o, d)))),
                            _ => Box::new(Line::new(o, d)),
                        }
                    }
                    1 => {
                        let ax = ax2_v(v)?;
                        if *ctor == 0 {
                            Box::new(Circle::from_circ(Circ::new(ax, *a)?))
                        } else {
                            Box::new(Circle::new(ax, *a)?)
                        }
                    }
                    2 => {
                        let ax = ax2_v(v)?;
                        if *ctor == 0 {
                            Box::new(Ellipse::from_elips(Elips::new(ax, *a, *b)?))
                        } else {
                            Box::new(Ellipse::new(ax, *a, *b)?)
                        }
                    }
                    3 => {
                        let ax = ax2_v(v)?;
                        if *ctor == 0 {
                            Box::new(Hyperbola::from_hypr(Hypr::new(ax, *a, *b)?))
                        } else {
                            Box::new(Hyperbola::new(ax, *a, *b)?)
                        }
                    }
                    _ => match ctor {
                        0 => Box::new(Parabola::from_parab(Parab::new(ax2_v(v)?, *a)?)),
                        1 => Box::new(Parabola::new(ax2_v(v)?, *a)?),
                        _ => Box::new(Parabola::from_directrix_focus(
                            Ax1::new(o, Dir::new(v[3], v[4], v[5])?),
                            Pnt::new(v[6], v[7], v[8]),
                        )?),
                    },
                }
            }
            CurveSpec::Bezier { poles, weights } => match weights {
                None => Box::new(BezierCurve::new(poles)?),
                Some(w) => Box::new(BezierCurve::with_weights(poles, w)?),
            },
            CurveSpec::BSpline { degree, periodic, knots, mults, poles, weights } => match weights {
                None => Box::new(BSplineCurve::new(poles, knots, mults, *degree, *periodic)?),
                Some(w) => Box::new(BSplineCurve::with_weights(poles, w, knots, mults, *degree, *periodic)?),
            },
            CurveSpec::Trimmed { u1, u2, sense, adjust, basis } => {
                let b: Arc<dyn Curve> = Arc::from(build(basis)?);
                Box::new(TrimmedCurve::new(b, *u1, *u2, *sense, *adjust)?)
            }
            CurveSpec::Offset { offset, not_check_c0, dir, basis } => {
                let b: Arc<dyn Curve> = Arc::from(build(basis)?);
                Box::new(OffsetCurve::new(b, *offset, Dir::new(dir[0], dir[1], dir[2])?, *not_check_c0)?)
            }
        })
    }

    /// Poles (and weights) of a surface recipe as rows (the oracle fills the weights that are missing with 0).
    fn rows_p(nu: usize, nv: usize, poles: &[Pnt]) -> Vec<Vec<Pnt>> {
        (0..nu).map(|i| (0..nv).map(|j| poles[i * nv + j]).collect()).collect()
    }

    fn rows_w(nu: usize, nv: usize, w: &[f64]) -> Vec<Vec<f64>> {
        (0..nu).map(|i| (0..nv).map(|j| w.get(i * nv + j).copied().unwrap_or(0.0)).collect()).collect()
    }

    pub(super) fn build_surface(s: &SurfSpec) -> R<Box<dyn Surface>> {
        Ok(match s {
            SurfSpec::Plane { ctor, fr, abcd } => match ctor {
                0 => Box::new(Plane::from_ax3(ax3_of(fr)?)),
                1 => Box::new(Plane::from_pln(Pln::new(ax3_of(fr)?))),
                2 => Box::new(Plane::from_point_normal(Pnt::new(fr[0], fr[1], fr[2]), Dir::new(fr[3], fr[4], fr[5])?)),
                _ => Box::new(Plane::from_coefficients(abcd[0], abcd[1], abcd[2], abcd[3])?),
            },
            SurfSpec::Cylinder { ctor, fr, r } => {
                if *ctor == 0 {
                    Box::new(CylindricalSurface::new(ax3_of(fr)?, *r)?)
                } else {
                    Box::new(CylindricalSurface::from_cylinder(Cylinder::new(ax3_of(fr)?, *r)?))
                }
            }
            SurfSpec::Cone { ctor, fr, ang, r } => {
                if *ctor == 0 {
                    Box::new(ConicalSurface::new(ax3_of(fr)?, *ang, *r)?)
                } else {
                    Box::new(ConicalSurface::from_cone(Cone::new(ax3_of(fr)?, *ang, *r)?))
                }
            }
            SurfSpec::Sphere { ctor, fr, r } => {
                if *ctor == 0 {
                    Box::new(SphericalSurface::new(ax3_of(fr)?, *r)?)
                } else {
                    Box::new(SphericalSurface::from_sphere(Sphere::new(ax3_of(fr)?, *r)?))
                }
            }
            SurfSpec::Torus { ctor, fr, major, minor } => {
                if *ctor == 0 {
                    Box::new(ToroidalSurface::new(ax3_of(fr)?, *major, *minor)?)
                } else {
                    Box::new(ToroidalSurface::from_torus(Torus::new(ax3_of(fr)?, *major, *minor)?))
                }
            }
            SurfSpec::Bezier { nu, nv, poles, weights } => {
                let p = rows_p(*nu, *nv, poles);
                match weights {
                    None => Box::new(BezierSurface::new(&p)?),
                    Some(w) => Box::new(BezierSurface::with_weights(&p, &rows_w(*nu, *nv, w))?),
                }
            }
            SurfSpec::BSpline { udeg, vdeg, uper, vper, uk, um, vk, vm, nu, nv, poles, weights } => {
                let p = rows_p(*nu, *nv, poles);
                match weights {
                    None => Box::new(BSplineSurface::new(&p, uk, vk, um, vm, *udeg, *vdeg, *uper, *vper)?),
                    Some(w) => Box::new(BSplineSurface::with_weights(
                        &p,
                        &rows_w(*nu, *nv, w),
                        uk,
                        vk,
                        um,
                        vm,
                        *udeg,
                        *vdeg,
                        *uper,
                        *vper,
                    )?),
                }
            }
            SurfSpec::Extrusion { dir, curve } => {
                let c: Arc<dyn Curve> = Arc::from(build(curve)?);
                Box::new(SurfaceOfLinearExtrusion::new(c, Dir::new(dir[0], dir[1], dir[2])?)?)
            }
            SurfSpec::Revolution { axis, curve } => {
                let c: Arc<dyn Curve> = Arc::from(build(curve)?);
                let a = Ax1::new(Pnt::new(axis[0], axis[1], axis[2]), Dir::new(axis[3], axis[4], axis[5])?);
                Box::new(SurfaceOfRevolution::new(c, a)?)
            }
            SurfSpec::Trimmed { ctor, t, s1, s2, utrim, basis } => {
                let b: Arc<dyn Surface> = Arc::from(build_surface(basis)?);
                if *ctor == 0 {
                    Box::new(RectangularTrimmedSurface::new(b, t[0], t[1], t[2], t[3], *s1, *s2)?)
                } else {
                    Box::new(RectangularTrimmedSurface::new_param(b, t[0], t[1], *utrim, *s1)?)
                }
            }
            SurfSpec::Offset { offset, not_check_c0, basis } => {
                let b: Arc<dyn Surface> = Arc::from(build_surface(basis)?);
                Box::new(OffsetSurface::new(b, *offset, *not_check_c0)?)
            }
        })
    }

}

'''

RS_H_MODS2 = r'''mod h2 {
    use super::{bits, Rd, Thrown};
    use occt_port::geom2d::{BSplineCurve, BezierCurve, Circle, Curve2d, Ellipse, Hyperbola, Line, OffsetCurve, Parabola, TrimmedCurve};
    use occt_port::gp::{Ax1, Ax22d, Ax2d, Circ2d, Dir2d, Elips2d, Hypr2d, Lin2d, Parab2d, Pnt2d};
    use std::sync::Arc;

    type R<T = ()> = Result<T, Thrown>;

    /// `crv`: a curve recipe.
    pub(super) enum CurveSpec {
        /// Kinds 0..=4: line, circle, ellipse, hyperbola, parabola.
        Basic {
            kind: i32,
            ctor: i32,
            o: [f64; 2],
            d: [f64; 2],
            sense: bool,
            a: f64,
            b: f64,
            focus: [f64; 2],
        },
        Bezier {
            poles: Vec<Pnt2d>,
            weights: Option<Vec<f64>>,
        },
        BSpline {
            degree: i32,
            periodic: bool,
            knots: Vec<f64>,
            mults: Vec<i32>,
            poles: Vec<Pnt2d>,
            weights: Option<Vec<f64>>,
        },
        Trimmed {
            u1: f64,
            u2: f64,
            sense: bool,
            adjust: bool,
            basis: Box<CurveSpec>,
        },
        Offset {
            offset: f64,
            not_check_c0: bool,
            basis: Box<CurveSpec>,
        },
    }

    impl Rd<'_> {
        pub(super) fn pnts2(&mut self) -> Vec<Pnt2d> {
            let n = self.count();
            (0..n)
                .map(|_| {
                    let x = self.f();
                    let y = self.f();
                    Pnt2d::new(x, y)
                })
                .collect()
        }

        pub(super) fn wts2(&mut self) -> Option<Vec<f64>> {
            if self.i() == 0 {
                return None;
            }
            let n = self.count();
            Some((0..n).map(|_| self.f()).collect())
        }

        /// `crv`.
        pub(super) fn crv2d(&mut self) -> CurveSpec {
            let kind = self.i();
            match kind {
                0..=4 => {
                    let ctor = self.i();
                    let o = [self.f(), self.f()];
                    let d = [self.f(), self.f()];
                    let sense = self.i() != 0;
                    let (mut a, mut b, mut focus) = (0.0, 0.0, [0.0; 2]);
                    match kind {
                        1 | 4 => a = self.f(),
                        2 | 3 => {
                            a = self.f();
                            b = self.f();
                        }
                        _ => {}
                    }
                    if kind == 4 && ctor == 3 {
                        focus = [self.f(), self.f()];
                    }
                    CurveSpec::Basic { kind, ctor, o, d, sense, a, b, focus }
                }
                5 => {
                    let poles = self.pnts2();
                    let weights = self.wts2();
                    CurveSpec::Bezier { poles, weights }
                }
                6 => {
                    let degree = self.i();
                    let periodic = self.i() != 0;
                    let nk = self.count();
                    let knots = (0..nk).map(|_| self.f()).collect();
                    let nm = self.count();
                    let mults = (0..nm).map(|_| self.i()).collect();
                    let poles = self.pnts2();
                    let weights = self.wts2();
                    CurveSpec::BSpline { degree, periodic, knots, mults, poles, weights }
                }
                7 => {
                    let u1 = self.f();
                    let u2 = self.f();
                    let sense = self.i() != 0;
                    let adjust = self.i() != 0;
                    CurveSpec::Trimmed { u1, u2, sense, adjust, basis: Box::new(self.crv2d()) }
                }
                8 => {
                    let offset = self.f();
                    let not_check_c0 = self.i() != 0;
                    CurveSpec::Offset { offset, not_check_c0, basis: Box::new(self.crv2d()) }
                }
                k => panic!("bad curve kind {k}"),
            }
        }

    }

    fn ax_of(o: &[f64; 2], d: &[f64; 2]) -> R<Ax2d> {
        Ok(Ax2d::new(Pnt2d::new(o[0], o[1]), Dir2d::new(d[0], d[1])?))
    }

    /// `build` of the oracle.
    pub(super) fn build(c: &CurveSpec) -> R<Box<dyn Curve2d>> {
        Ok(match c {
            CurveSpec::Basic { kind, ctor, o, d, sense, a, b, focus } => {
                let ax = ax_of(o, d)?;
                let s = *sense;
                match (*kind, *ctor) {
                    (0, 0) => Box::new(Line::from_ax2d(ax)),
                    (0, 1) => Box::new(Line::from_lin2d(Lin2d::from_ax2d(ax))),
                    (0, _) => Box::new(Line::new(*ax.location(), *ax.direction())),
                    (1, 0) => Box::new(Circle::from_circ2d(Circ2d::new(ax, *a, s)?)),
                    (1, 1) => Box::new(Circle::new(ax, *a, s)?),
                    (1, _) => Box::new(Circle::from_ax22d(Ax22d::from_ax2d(ax, s), *a)?),
                    (2, 0) => Box::new(Ellipse::from_elips2d(Elips2d::new(ax, *a, *b, s)?)),
                    (2, 1) => Box::new(Ellipse::new(ax, *a, *b, s)?),
                    (2, _) => Box::new(Ellipse::from_ax22d(Ax22d::from_ax2d(ax, s), *a, *b)?),
                    (3, 0) => Box::new(Hyperbola::from_hypr2d(Hypr2d::new(ax, *a, *b, s)?)),
                    (3, 1) => Box::new(Hyperbola::new(ax, *a, *b, s)?),
                    (3, _) => Box::new(Hyperbola::from_ax22d(Ax22d::from_ax2d(ax, s), *a, *b)?),
                    (_, 0) => Box::new(Parabola::from_parab2d(Parab2d::new(ax, *a, s)?)),
                    (_, 1) => Box::new(Parabola::new(ax, *a, s)?),
                    (_, 2) => Box::new(Parabola::from_ax22d(Ax22d::from_ax2d(ax, s), *a)?),
                    (_, _) => Box::new(Parabola::from_directrix_focus(ax, Pnt2d::new(focus[0], focus[1]))),
                }
            }
            CurveSpec::Bezier { poles, weights } => match weights {
                None => Box::new(BezierCurve::new(poles)?),
                Some(w) => Box::new(BezierCurve::with_weights(poles, w)?),
            },
            CurveSpec::BSpline { degree, periodic, knots, mults, poles, weights } => match weights {
                None => Box::new(BSplineCurve::new(poles, knots, mults, *degree, *periodic)?),
                Some(w) => Box::new(BSplineCurve::with_weights(poles, w, knots, mults, *degree, *periodic)?),
            },
            CurveSpec::Trimmed { u1, u2, sense, adjust, basis } => {
                let b: Arc<dyn Curve2d> = Arc::from(build(basis)?);
                Box::new(TrimmedCurve::new(b, *u1, *u2, *sense, *adjust)?)
            }
            CurveSpec::Offset { offset, not_check_c0, basis } => {
                let b: Arc<dyn Curve2d> = Arc::from(build(basis)?);
                Box::new(OffsetCurve::new(b, *offset, *not_check_c0)?)
            }
        })
    }

}
'''

def mname(word):
    # Nested names (`Class::Enum`) would leave `__`, which rustc's snake-case lint rejects.
    return re.sub(r'_+', '_', re.sub(r'[^A-Za-z0-9]', '_', word)).lower()


# --------------------------------------------------------------------------------------------------------------
# Rust arms, readers, API doc, stub

class RsCtx:
    def __init__(self, m, rustmod, names):
        self.m = m
        self.rustmod = rustmod
        self.names = names
        # a class of the package named like a gp type (GeomGridEval_Cone): the gp type is written `gp::Cone` in signatures
        # a type of the package named `Result` (ExtremaPC::Result) hides the std one in the signatures of the doc and the stub
        self.result_name = 'std::result::Result' if ('Result' in {c.short for c in m.ctx.classes.values()} or
                                                      any(n.split('::')[-1] == 'Result' for n in m.ctx.structs)) else 'Result'
        self.clash = {c.short for c in m.ctx.classes.values() if c.name not in GP_BY_CLASS} & {v[1] for v in GP.values()}

    def enum_path(self, e):
        pkg = e.split('::')[0] if ('::' in e and '_' not in e.split('::')[0]) else e.split('_')[0]
        short = enum_short(e, self.names)
        if '::' in short and '::' not in e:
            return short            # names.tsv: `enum GeomAbs_Shape<TAB>occt_port::plib::GeomAbsShape` gives the full path
        mod = self.rustmod if pkg == self.m.pkg else pkg.lower()
        return ('gp::' if mod == 'gp' else 'occt_port::%s::' % mod) + short

    def enum_variants(self, e):
        return [enum_variant(self.m, e, c, self.names) for c in self.m.ctx.enums[e]]

    def cls_path(self, c):
        if c.name in GP_BY_CLASS:
            return GP[GP_BY_CLASS[c.name]][1]
        return 'occt_port::%s::%s' % (self.rustmod, c.short)

    def fn_path(self, plan):
        f = plan.fn
        if f.kind == 'free' or (plan.cls is not None and plan.cls.name == self.m.pkg):
            return 'occt_port::%s::%s' % (self.rustmod, f.rust)
        return '%s::%s' % (self.cls_path(plan.cls), f.rust)


def rs_in_type(rc, p):
    k = p.kind
    t = k[0]
    if t == 'f':
        return 'f64'
    if t == 'i':
        return 'i32'
    if t == 'b':
        return 'bool'
    if t == 'enum':
        return enum_short(k[1], rc.names)
    if t == 'gp':
        n = 'gp::Vec' if k[1] == 'vec' else GP[k[1]][1]
        if rc is not None and n in rc.clash:
            n = 'gp::' + n
        return ('&' + n) if p.info['ref'] else n
    if t == 'arr':
        e = k[1]
        return '&[%s]' % rs_elem(e)
    if t == 'arr2':
        return '&Array2<%s>' % rs_elem(k[1])
    if t == 'cls':
        return ('&' if p.info['ref'] else '') + strip_pkg(k[1])
    if t == 'bx':
        return ('&mut %s' if p.mutref else '&%s') % BX_RS[k[1]]
    if t == 'dom':
        return ('&' if p.info['ref'] else '') + DOMAINS[k[1]][0]
    if t == 'h':
        return 'Arc<%s>' % H_INFO[k[1]][1]
    if t == 'ad':
        return ('Arc<%s>' if p.info.get('handle') else '&%s') % AD_INFO[k[1]][1]
    return '?'


def rs_elem(e):
    return {'f': 'f64', 'i': 'i32'}.get(e) or ('gp::Vec' if e == 'vec' else GP[e][1])


def rs_out_type(k, rc=None):
    t = k[0]
    if t == 'f':
        return 'f64'
    if t == 'i':
        return 'i32'
    if t == 'b':
        return 'bool'
    if t == 'enum':
        return enum_short(k[1], rc.names if rc else {})
    if t == 'gp':
        n = 'gp::Vec' if k[1] == 'vec' else GP[k[1]][1]
        return 'gp::' + n if (rc is not None and n in rc.clash) else n
    if t == 'arr':
        return 'Vec<%s>' % rs_elem(k[1])
    if t == 'arr2':
        return 'Array2<%s>' % rs_elem(k[1])
    if t == 'u':
        return 'usize'
    if t == 'dom':
        return DOMAINS[k[1]][0]
    if t == 'ns':
        return 'occt_port::%s::%s' % (rc.rustmod if rc else 'pkg', k[1].split('::')[-1])
    if t == 'st':
        return STRUCTS[k[1]][0]
    if t == 'sarr':
        return ('Vec<%s>' if k[2] == 1 else 'Array2<%s>') % STRUCTS[k[1]][0]
    if t == 'h':
        return 'Arc<%s>' % H_INFO[k[1]][1]
    if t == 'ad':
        return ('Arc<%s>' if k[2] else '&%s') % AD_INFO[k[1]][1]
    return '()'


def ret_type(plan, rc=None):
    f = plan.fn
    parts = []
    if f.ret and f.ret[0] != 'void':
        parts.append(rs_out_type(f.ret, rc))
    for p in f.params:
        if p.mode == 'out':
            parts.append(rs_out_type(p.kind, rc))
    if plan.kind == 'ctor_gp':
        t = 'Self'
    elif not parts:
        t = '()'
    elif len(parts) == 1:
        t = parts[0]
    else:
        t = '(' + ', '.join(parts) + ')'
    if plan.can_throw:
        t = '%s<%s, Error>' % (rc.result_name if rc else 'Result', t)
    return t


def pname(p, j):
    n = p.name
    if re.match(r'^the[A-Z]', n):
        n = n[3:]
    return rust_ident(snake(n) or 'a%d' % j)


def signature(plan, rc, with_pub=True):
    f = plan.fn
    ps = []
    if f.kind == 'method':
        ps.append('&self' if f.const else '&mut self')
    seen = set()
    for j, p in enumerate(f.params):
        if p.mode == 'in' or j in plan.inout:
            n = pname(p, j)
            while n in seen:
                n += '_'
            seen.add(n)
            ps.append('%s: %s' % (n, rs_in_type(rc, p)))
    if f.rust.startswith('='):
        return '// adapter %s -> %s   (C++ %s)' % (f.rust, ret_type(plan, rc), f.cname)
    return '%sfn %s(%s) -> %s;' % ('pub ' if with_pub else '', f.rust, ', '.join(ps), ret_type(plan, rc))


def pass_expr(p, j, byval=()):
    k = p.kind
    v = 'a%d' % j
    t = k[0]
    if t in ('f', 'i', 'b', 'enum'):
        return v
    if t == 'bx':
        return ('&mut ' if p.mutref else '&') + v
    if t == 'dom':
        return ('&' + v) if p.info['ref'] else v
    if t == 'h':
        return v + '.clone()'
    if t == 'ad':
        if p.info.get('handle'):
            return v + '.clone()'
        return ('&' + v) if AD_INFO[k[1]][2] else '&*' + v
    if j in byval:
        return v + '.clone()'
    if t == 'gp':
        return ('&' + v) if p.info['ref'] else v + '.clone()'
    if t in ('arr', 'arr2'):
        return '&' + v
    if t == 'cls':
        return ('&' + v) if p.info['ref'] else v
    return v


def tmpl(name, recv, args):
    """A Rust name starting with `=` in names.tsv is an expression template: {s} is the receiver, {0}.. the arguments."""
    t = name[1:]
    t = t.replace('{s}', recv or '')
    for i, a in enumerate(args):
        t = t.replace('{%d}' % i, a)
    return t


def rs_arm(plan, rc):
    f = plan.fn
    L = ['        "%s" => {' % plan.name]
    mut_recv = plan.mutates
    reads = []
    if plan.kind == 'ctor_obj':
        L.append('            let s = r.%s();' % mname('obj_' + plan.cls.name))
        reads.append(('s', True))
    else:
        if plan.recv:
            L.append('            let s = r.%s();' % mname(word_of_recv(plan.recv)))
            reads.append(('s', mut_recv))
        for j, p in enumerate(f.params):
            if p.mode == 'in' or j in plan.inout:
                L.append('            let a%d = r.%s();' % (j, mname(word_of(p.kind))))
                if p.kind[0] not in ('f', 'i', 'b'):
                    reads.append(('a%d' % j, p.mutref))
    for v, mu in reads:
        L.append('            let %s%s = %s?;' % ('mut ' if mu else '', v, v))
    arglist = [pass_expr(p, j, f.byval) for j, p in enumerate(f.params) if p.mode == 'in' or j in plan.inout] if f else []
    args = ', '.join(arglist)
    if plan.kind == 'ctor_obj':
        pass
    elif plan.kind == 'ctor_gp':
        if f.rust.startswith('='):
            call = tmpl(f.rust, None, arglist)
        else:
            call = '%s(%s)' % (rc.fn_path(plan), args)
        L.append('            let v = %s.lift()?;' % call)
        L.append('            g.put(&v);')
    else:
        if f.rust.startswith('='):
            call = tmpl(f.rust, 's', arglist)
        elif f.kind in ('static', 'free'):
            call = '%s(%s)' % (rc.fn_path(plan), args)
        else:
            call = 's.%s(%s)' % (f.rust, args)
        L.append('            let v = %s.lift()?;' % call)
        L.append('            g.put(&v);')
        if plan.mutates and plan.recv[0] == 'gp':
            L.append('            g.put(&s);')
        for j, p in enumerate(f.params):
            if p.mutref:
                L.append('            g.put(&a%d);' % j)
    for gt in plan.getters:
        L.append('            match s.%s().lift() {' % gt.rust)
        L.append('                Ok(v) => g.put(&v),')
        L.append('                Err(_) => g.x(),')
        L.append('            }')
    L.append('        }')
    return '\n'.join(L)


def rs_readers(m, rc, plans):
    """Readers for arrays, enums and objects used by the kept plans."""
    ctx = m.ctx
    out = []
    arrs, enums, objs = set(), set(), set()
    hs, ads, sts, bxs, doms, nss = set(), set(), set(), set(), set(), []

    def see(k):
        if k is None:
            return
        if k[0] in ('st', 'sarr'):
            sts.add(k[1])
        if k[0] == 'bx':
            bxs.add(k[1])
        if k[0] == 'dom':
            doms.add(k[1])
        if k[0] == 'ns':
            for sn_ in ctx.used_ns:
                if sn_ not in nss:
                    nss.append(sn_)
                    for fname_, qt_ in ctx.structs[sn_]:
                        fk_ = ctx.classify(qt_)[0]
                        if fk_ and fk_[0] == 'enum':
                            enums.add(fk_[1])
        if k[0] == 'h':
            hs.add(k[1])
        elif k[0] == 'ad':
            ads.add(k[1])
        if k[0] in ('arr', 'arr2'):
            arrs.add(k)
        elif k[0] == 'enum':
            enums.add(k[1])
        elif k[0] == 'cls':
            objs.add(k[1])
    stack = []
    for p in plans:
        if p.fn:
            for q in p.fn.params:
                see(q.kind)
            see(p.fn.ret)
        if p.recv and p.recv[0] == 'cls':
            objs.add(p.recv[1].name)
        if p.kind == 'ctor_obj':
            objs.add(p.cls.name)
        for gt in p.getters:
            see(gt.ret)
    # classes reachable through constructor arguments
    changed = True
    while changed:
        changed = False
        for n in list(objs):
            c = ctx.classes[n]
            for f in c.drv_ctors:
                for q in (f.params if f else []):
                    k = q.kind
                    before = (len(arrs), len(enums), len(objs), len(hs), len(ads), len(bxs), len(doms))
                    see(k)
                    if (len(arrs), len(enums), len(objs), len(hs), len(ads), len(bxs), len(doms)) != before:
                        changed = True
    fallible = {'dir', 'dir2d'}
    for k in sorted(arrs):
        e = k[1]
        fn = mname(('arr_' if k[0] == 'arr' else 'arr2_') + e)
        et = rs_elem(e)
        if k[0] == 'arr':
            if e in ('f', 'i'):
                body = 'let n = self.count();\n        Ok((0..n).map(|_| self.%s()).collect())' % e
            else:
                body = ('let n = self.count();\n        let v: Vec<Result<%s, Thrown>> = (0..n).map(|_| self.%s()).collect();\n'
                        '        v.into_iter().collect()') % (et, e)
            out.append('    fn %s(&mut self) -> Result<Vec<%s>, Thrown> {\n        %s\n    }' % (fn, et, body))
        else:
            et2 = et
            if e == 'f':
                rd = 'Ok(self.f())'
                body = 'let nr = self.count();\n        let nc = self.count();\n        let data: Vec<%s> = (0..nr * nc).map(|_| self.f()).collect();\n        Array2::new(nr, nc, data).lift()' % et2
            else:
                body = ('let nr = self.count();\n        let nc = self.count();\n        let v: Vec<Result<%s, Thrown>> = (0..nr * nc).map(|_| self.%s()).collect();\n'
                        '        let data = v.into_iter().collect::<Result<Vec<_>, _>>()?;\n        Array2::new(nr, nc, data).lift()') % (et2, e)
            out.append('    fn %s(&mut self) -> Result<Array2<%s>, Thrown> {\n        %s\n    }' % (fn, et2, body))
    for e in sorted(enums):
        vs = rc.enum_variants(e)
        path = rc.enum_path(e)
        arms = '\n'.join('            %d => Ok(%s::%s),' % (i, path, v) for i, v in enumerate(vs))
        out.append('    fn %s(&mut self) -> Result<%s, Thrown> {\n        match self.i() {\n%s\n            k => panic!("bad ordinal {k} of %s"),\n        }\n    }' % (
            mname('e_' + e), path, arms, e))
    for n in sorted(objs):
        c = ctx.classes[n]
        arms = []
        for i, f in enumerate(c.drv_ctors):
            body = []
            args = []
            for j, q in enumerate(f.params if f else []):
                body.append('                let a%d = self.%s();' % (j, mname(word_of(q.kind))))
            for j, q in enumerate(f.params if f else []):
                if q.kind[0] not in ('f', 'i', 'b'):
                    body.append('                let a%d = a%d?;' % (j, j))
                args.append(pass_expr(q, j, f.byval if f else ()))
            if f and f.rust.startswith('='):
                call = tmpl(f.rust, None, args)
            else:
                call = '%s(%s)' % (('%s::%s' % (rc.cls_path(c), f.rust)) if f else '%s::new' % rc.cls_path(c), ', '.join(args))
            arms.append('            %d => {\n%s\n                %s.lift()\n            }' % (
                i, '\n'.join(body), call))
        out.append('    fn %s(&mut self) -> Result<%s, Thrown> {\n        let k = self.i();\n        match k {\n%s\n            _ => panic!("bad constructor index {k}"),\n        }\n    }' % (
            mname('obj_' + n), rc.cls_path(c), '\n'.join(arms)))
    emits = []
    for n in sorted(objs):
        emits.append('impl Plain for %s {}' % rc.cls_path(ctx.classes[n]))
    for e in sorted(enums):
        path = rc.enum_path(e)
        emits.append('impl Plain for %s {}\nimpl Emit for %s {\n    fn emit(&self, g: &mut Got) {\n        g.i(*self as i32);\n    }\n}' % (path, path))
    hr, he = handle_rs(rc, hs, ads)
    for dn in sorted(doms):
        ty, rfs, cfs = DOMAINS[dn]
        n_ = len(rfs)
        hr.append('    fn %s(&mut self) -> Result<%s, Thrown> {\n        let v = [%s];\n        Ok(%s::new(%s))\n    }' % (
            mname('dom_' + dn.replace('::', '_')), ty, ', '.join('self.f()' for _ in range(n_)), ty,
            ', '.join('v[%d]' % i for i in range(n_))))
        emits.append('impl Plain for %s {}\nimpl Emit for %s {\n    fn emit(&self, g: &mut Got) {\n%s\n    }\n}' % (
            ty, ty, '\n'.join('        g.f(self.%s);' % f_ for f_ in rfs)))
    for sn in nss:
        ty = 'occt_port::%s::%s' % (rc.rustmod, sn.split('::')[-1])
        lines_ = []
        for fname, qt in ctx.structs[sn]:
            lines_.append('        g.put(&self.%s);' % snake(fname))
        emits.append('impl Plain for %s {}\nimpl Emit for %s {\n    fn emit(&self, g: &mut Got) {\n%s\n    }\n}' % (ty, ty, '\n'.join(lines_)))
    for bn in sorted(bxs):
        d3 = bn == 'Bnd_Box'
        ty = BX_RS[bn]
        hr.append('    fn %s(&mut self) -> Result<%s, Thrown> {\n        let mut b = %s::new();\n        if self.i() != 0 {\n            %s\n        }\n        b.set_gap(self.f());\n        Ok(b)\n    }' % (
            mname('bx_' + bn), ty, ty,
            'let (a, b2, c, d, e, f) = (self.f(), self.f(), self.f(), self.f(), self.f(), self.f());\n            b.update(a, b2, c, d, e, f);' if d3 else
            'let (a, b2, c, d) = (self.f(), self.f(), self.f(), self.f());\n            b.update(a, b2, c, d);'))
        emits.append('impl Plain for %s {}\nimpl Emit for %s {\n    fn emit(&self, g: &mut Got) {\n        g.b(self.is_void());\n'
                     '        if let Ok(v) = self.get() {\n            for x in v {\n                g.f(x);\n            }\n            g.f(self.gap());\n        }\n    }\n}' % (ty, ty))
    for sn in sorted(sts):
        ty, flds = STRUCTS[sn]
        emits.append('impl Plain for %s {}\nimpl Emit for %s {\n    fn emit(&self, g: &mut Got) {\n%s\n    }\n}' % (
            ty, ty, '\n'.join('        g.put(&self.%s);' % rf for cf, rf, w in flds)))
    # methods inherited from Geom_* / Adaptor* bases are trait methods of the translation
    traits = set()
    for c in ctx.classes.values():
        for b in c.bases:
            bn = split_type(b)['base']
            if bn in AD_INFO:
                traits.add(AD_INFO[bn][1].replace('dyn ', ''))
            elif bn in H_INFO:
                tyb = H_INFO[bn][1].replace('dyn ', '')
                mod_ = tyb.rsplit('::', 1)[0]
                fam = H_INFO[bn][0]
                names_ = {'c3': ['Curve', 'BoundedCurve', 'Conic', 'Geometry'], 's3': ['Surface', 'ElementarySurface', 'SweptSurface', 'BoundedSurface', 'Geometry'],
                          'c2': ['Curve2d', 'BoundedCurve2d', 'Conic2d', 'Geometry2d']}[fam]
                for n_ in names_:
                    traits.add(mod_ + '::' + n_)
    if traits:
        he = '\n'.join('use %s as _;' % t for t in sorted(traits)) + '\n' + (he or '')
    return '\n'.join(out + hr), '\n'.join(emits + [he] if he else emits)


def handle_rs(rc, hs, ads):
    """Readers (methods of Rd) and the static / Emit text for the handle and adaptor words used."""
    if not hs and not ads:
        return [], ''
    fams = {H_INFO[h][0] for h in hs}
    afams = {AD_INFO[a][0] for a in ads}
    # an adaptor loads the curve / surface recipe of its family
    if 'ac3' in afams:
        fams.add('c3')
    if 'ac2' in afams:
        fams.add('c2')
    if 'as3' in afams:
        fams.add('s3')
    rd = []
    em = [RS_H_COMMON]
    if 'c3' in fams or 's3' in fams:
        em.append(RS_H_MODS3)
        em.append(RS_H_C3)
    if 'c2' in fams:
        em.append(RS_H_MODS2)
        em.append(RS_H_C2)
    if 'ac3' in afams:
        em.append(RS_H_AC3)
    if 'ac2' in afams:
        em.append(RS_H_AC2)
    if 'as3' in afams:
        em.append(RS_H_AS3)
    snapfn = {'c3': 'snap_c3', 'c2': 'snap_c2', 's3': 'snap_s3', 'ac3': 'snap_ad_c3', 'ac2': 'snap_ad_c2', 'as3': 'snap_ad_s3'}
    done_ty = set()

    def emit_impls(ty, fn):
        if ty in done_ty:
            return
        done_ty.add(ty)
        em.append('impl Plain for Arc<%s> {}\nimpl Emit for Arc<%s> {\n    fn emit(&self, g: &mut Got) {\n        %s(g, &**self);\n    }\n}' % (ty, ty, fn))
        em.append('impl Plain for Option<Arc<%s>> {}\nimpl Emit for Option<Arc<%s>> {\n    fn emit(&self, g: &mut Got) {\n'
                  '        match self {\n            Some(a) => %s(g, &**a),\n            None => g.i(0),\n        }\n    }\n}' % (ty, ty, fn))
        if ty.startswith('dyn '):
            em.append('impl Plain for %s {}\nimpl Emit for %s {\n    fn emit(&self, g: &mut Got) {\n        %s(g, self);\n    }\n}' % (ty, ty, fn))
    for h in sorted(hs):
        fam, ty, kinds, conc = H_INFO[h]
        spec, bld = {'c3': ('self.crv()', 'h3::build'), 'c2': ('self.crv2d()', 'h2::build'), 's3': ('self.srf()', 'h3::build_surface')}[fam]
        if conc is None:
            conv = 'Ok(Arc::from(b))'
        elif conc in ('bounded', 'conic', 'elementary', 'swept'):
            conv = '%s%s(b)' % (conc, {'c3': '3', 'c2': '2', 's3': '3' + ('s' if conc == 'bounded' else '')}[fam])
        else:
            conv = '%s(&*b)' % ('conc3s::<%s>' if fam == 's3' else ('conc3::<%s>' if fam == 'c3' else 'conc2::<%s>')) % ty
        rd.append('    fn %s(&mut self) -> Result<Arc<%s>, Thrown> {\n        let spec = %s;\n        let b = %s(&spec)?;\n        %s\n    }' % (
            mname('h_' + h), ty, spec, bld, conv))
        emit_impls(ty, snapfn[fam])
    for a in sorted(ads):
        fam, ty, conc = AD_INFO[a]
        base = {'ac3': 'occt_port::geomadaptor::Curve', 'ac2': 'occt_port::geom2dadaptor::Curve', 'as3': 'occt_port::geomadaptor::Surface'}[fam]
        rname = mname('ad_' + a)
        if conc:
            rd.append(AD_READER[fam].replace('@@NAME@@', rname).replace('@@TY@@', ty))
            if ty not in done_ty:
                done_ty.add(ty)
                em.append('impl Plain for %s {}\nimpl Emit for %s {\n    fn emit(&self, g: &mut Got) {\n        %s(g, self);\n    }\n}' % (ty, ty, snapfn[fam]))
        else:
            rd.append('    fn %s(&mut self) -> Result<Arc<%s>, Thrown> {\n        Ok(Arc::new(self.%s()?))\n    }' % (rname, ty, mname('ad_' + {'ac3': 'GeomAdaptor_Curve', 'ac2': 'Geom2dAdaptor_Curve', 'as3': 'GeomAdaptor_Surface'}[fam])))
            if 'ad_' + {'ac3': 'GeomAdaptor_Curve', 'ac2': 'Geom2dAdaptor_Curve', 'as3': 'GeomAdaptor_Surface'}[fam] not in ' '.join(ads_names(ads)):
                a2 = {'ac3': 'GeomAdaptor_Curve', 'ac2': 'Geom2dAdaptor_Curve', 'as3': 'GeomAdaptor_Surface'}[fam]
                rd.append(AD_READER[fam].replace('@@NAME@@', mname('ad_' + a2)).replace('@@TY@@', AD_INFO[a2][1]))
            emit_impls(ty, snapfn[fam])
    return rd, '\n\n'.join(em)


def ads_names(ads):
    return ['ad_' + a for a in ads if AD_INFO[a][2]]


AD_READER = {
    'ac3': '''    fn @@NAME@@(&mut self) -> Result<@@TY@@, Thrown> {
        let use_range = self.i() != 0;
        let (f, l) = (self.f(), self.f());
        let spec = self.crv();
        let c: Arc<dyn occt_port::geom::Curve> = Arc::from(h3::build(&spec)?);
        if use_range {
            @@TY@@::with_range(c, f, l).map_err(|_| Thrown)
        } else {
            @@TY@@::with_curve(c).map_err(|_| Thrown)
        }
    }''',
    'ac2': '''    fn @@NAME@@(&mut self) -> Result<@@TY@@, Thrown> {
        let use_range = self.i() != 0;
        let (f, l) = (self.f(), self.f());
        let spec = self.crv2d();
        let c: Arc<dyn occt_port::geom2d::Curve2d> = Arc::from(h2::build(&spec)?);
        if use_range {
            @@TY@@::with_range(c, f, l).map_err(|_| Thrown)
        } else {
            @@TY@@::with_curve(c).map_err(|_| Thrown)
        }
    }''',
    'as3': '''    fn @@NAME@@(&mut self) -> Result<@@TY@@, Thrown> {
        let use_range = self.i() != 0;
        let (u1, u2, v1, v2) = (self.f(), self.f(), self.f(), self.f());
        let spec = self.srf();
        let s: Arc<dyn occt_port::geom::Surface> = Arc::from(h3::build_surface(&spec)?);
        if use_range {
            @@TY@@::with_range(s, u1, u2, v1, v2, 0.0, 0.0).map_err(|_| Thrown)
        } else {
            @@TY@@::with_surface(s).map_err(|_| Thrown)
        }
    }''',
}


# --------------------------------------------------------------------------------------------------------------
# API doc and stub

def used_object_classes(plans, ctx):
    objs = set()

    def see(k):
        if k and k[0] == 'cls':
            objs.add(k[1])
    for p in plans:
        if p.fn:
            for q in p.fn.params:
                see(q.kind)
        if p.recv and p.recv[0] == 'cls':
            objs.add(p.recv[1].name)
        if p.kind == 'ctor_obj':
            objs.add(p.cls.name)
    changed = True
    while changed:
        changed = False
        for n in list(objs):
            for f in ctx.classes[n].drv_ctors:
                for q in (f.params if f else []):
                    if q.kind and q.kind[0] == 'cls' and q.kind[1] not in objs:
                        objs.add(q.kind[1])
                        changed = True
    return objs


CHECKS_NOTE = ['']


def doc_entries(plans, m, ctor_throws=None):
    """kept plans -> list of (class or None, signature plan)"""
    out = []
    ctor_throws = ctor_throws or {}
    have_ctor = set()
    for p in plans:
        if p.kind == 'ctor_obj':
            c = p.cls
            have_ctor.add(c.name)
            for i, f in enumerate(c.drv_ctors):
                q = Plan(p.name, f, 'ctor_gp')
                q.cls = c
                q.can_throw = i in ctor_throws.get(p.name, ()) or bool(f and f.raises)
                out.append((c, q))
        elif p.kind == 'ctor_gp':
            out.append((p.cls, p))
        elif p.fn.kind in ('free',) or (p.cls is not None and p.cls.name == m.pkg):
            out.append((None, p))
        else:
            c = p.recv[1] if (p.recv and p.recv[0] == 'cls') else p.cls
            out.append((c, p))
    # getters read by the snapshot of an object must exist even when their own family was dropped
    present = {(c.name if c else None, id(q.fn)) for c, q in out if q.fn is not None}
    for p in plans:
        rc_ = p.cls if p.kind == 'ctor_obj' else (p.recv[1] if (p.recv and p.recv[0] == 'cls') else None)
        for gt in p.getters:
            if rc_ is not None and (rc_.name, id(gt)) not in present:
                present.add((rc_.name, id(gt)))
                q = Plan(gt.cname, gt, 'call')
                q.cls = rc_
                out.append((rc_, q))
    for n in sorted(used_object_classes(plans, m.ctx)):
        c = m.ctx.classes[n]
        if n not in have_ctor:
            for f in c.drv_ctors:
                q = Plan(n + '_new', f, 'ctor_gp')
                q.cls = c
                q.can_throw = bool(f and f.raises)
                out.append((c, q))
    return out


def struct_decl(ctx, sn, rc):
    """The Rust shape of a namespace struct the judge reads fields of."""
    flds = []
    for fname, qt in ctx.structs[sn]:
        ma = re.match(r'^NCollection_(?:DynamicArray|Array1|Vector)<\s*([\w:]+)\s*>$', qt.strip())
        if ma:
            ty = 'Vec<%s>' % ma.group(1).split('::')[-1]
        elif split_type(qt)['base'] in ctx.structs:
            ty = split_type(qt)['base'].split('::')[-1]
        else:
            ty = rs_out_type(ctx.classify(qt)[0], rc)
        flds.append('pub %s: %s' % (snake(fname), ty))
    return '#[derive(Debug, Clone, PartialEq)]\npub struct %s { %s }' % (sn.split('::')[-1], ', '.join(flds))


def sig_of(q, rc):
    if q.fn is None:
        return 'pub fn new() -> %s;' % ('%s<Self, Error>' % rc.result_name if q.can_throw else 'Self')
    return signature(q, rc)


def write_doc(path, m, plans, notgen, rc, args, cov, shown_names, pmod=None, with_gaps=True, parts=None, ctor_throws=None):
    pkg, mod = m.pkg, (pmod or args.module)
    L = []
    L.append('# `%s` Rust API contract (generated by `tools/judgegen.py`, module `%s`)\n' % (pkg, mod))
    L.append('This is the API the generated judge `tests/%s.rs` replays; the translator must provide exactly these items in '
             '`occt_port::%s` (C++ names mapped by the rule below; `docs/api-%s.names.tsv` lists every Rust name '
             'used). Everything is derived mechanically from the OCCT 8.0.1 headers of the package.\n' % (
                 mod if not parts else mod + '_<part>', rc.rustmod, args.module))
    L.append('## Conventions\n')
    L.append('* Scalars are `f64`, `i32`, `bool`; a C++ enum is a fieldless Rust enum in declaration order '
             '(`*self as i32` is the C++ ordinal; variant = constant without the package prefix, CamelCase); gp value types are '
             '`occt_port::gp` types (`gp::Vec` is `gp_Vec`); a `TColgp_Array1OfPnt` / `TColStd_Array1OfReal` ... input is '
             '`&[T]` and an output is `Vec<T>` (lower bound 1); a two-dimensional array is `plib::Array2<T>`.')
    L.append('* A C++ `const T&` parameter is `&T`, a by-value parameter is `T`. Non-const reference parameters are '
             'out-parameters: they are not parameters in Rust, the function returns them after the return value, in order '
             '(`(ret, out1, out2)`; a single value is returned bare, none is `()`). A non-const method takes `&mut self` '
             'and the harness prints the object again after the call; a const method takes `&self`.')
    L.append('* Methods that can raise `Standard_Failure` return `Result<_, Error>`; this list shows `Result` where the oracle '
             'saw a `throws` line or the header documents a raise. The harness accepts a plain value or a `Result` from every call; a '
             '`throws` line requires an `Err`, so a method that can raise must return `Result`. Constructors of '
             'classes of the package are associated functions (`new`, overloads `new_<suffix>`). Static methods of the '
             'class `%s` and free functions are functions of the module.' % pkg)
    L.append('* Rust has no overloading. The name of an overload is `snake_case(C++ name)` plus: `_<count>` when the '
             'overloads differ in parameter count, then `_<types>` of the parameters whose type differs among overloads '
             'with the same count (`lin`, `circ2d`, `real`, `int`, `arr_pnt`...), `_mut` for the non-const twin of a const '
             'method with the same parameters, `_b`, `_c` for a remaining clash. `names.tsv` overrides: '
             '`Class::Method(sig)<TAB>name` or `Class::Method/N<TAB>name`.')
    L.append('* ' + CHECKS_NOTE[0])
    L.append('* Objects of the package\'s classes are checked by their *snapshot*: after the call the harness reads every '
             'zero-argument const getter of the class (listed per class below) and compares the values; a getter that raises '
             'must return `Err` (printed `x`).')
    L.append('* The harness builds gp inputs with these `occt_port::gp` calls (all part of the already translated gp API): '
             '`Pnt::new`, `Dir::new(x,y,z)` (normalizes, fails on zero), `Ax1::new(Pnt, Dir)`, `Ax2::new(Pnt, Dir, &Dir)`, '
             '`Ax3::from_ax2(&Ax2)` + `x_reverse/y_reverse`, `Lin::from_axis`, `Circ/Elips/Hypr/Parab::new(Ax2, ..)`, '
             '`Pln::new(Ax3)`, `Cylinder/Cone/Sphere/Torus::new(Ax3, ..)`, `Mat::from_values`, `Quaternion::from_components`, '
             '`Trsf::new` + `set_rotation/set_scale/set_translation/set_mirror`, and the 2D forms (`Ax22d::from_dirs`, '
             '`Circ2d::from_ax22d` ..., `Lin2d::from_ax2d`, `Mat2d::from_columns`, `Trsf2d::new` + setters). Outputs are read '
             'through `x()/y()/z()`, `location()/direction()/x_direction()/y_direction()`, `position()/axis()`, `radius()` ..., '
             '`value(r, c)` (see the `Emit` impls in the test).\n')
    if m.ctx.saw_handle or m.ctx.saw_bx:
        L.append('* **Handles, adaptors, boxes.** A `Handle(Geom_*)` / `Handle(Geom2d_*)` argument is `Arc<dyn Curve>` / `Arc<dyn Surface>` '
                 '(`Arc<Circle>` ... for a concrete class; `Arc<dyn Conic>` / `BoundedCurve` / `ElementarySurface` ... for the '
                 'intermediate classes) built from a printed recipe by the builders copied from tests/geom.rs and tests/geom2d.rs '
                 '(`Arc::from(Box)`); passed as `a.clone()`. An adaptor argument is built on such a curve / surface: '
                 '`geomadaptor::Curve::with_curve / with_range(c, f, l)`, `geomadaptor::Surface::with_surface / '
                 'with_range(s, u1, u2, v1, v2, 0.0, 0.0)`, `geom2dadaptor::Curve::with_curve / with_range` (the traits '
                 '`adaptor3d::Curve3dAdaptor` / `SurfaceAdaptor`, `adaptor2d::Curve2dAdaptor`); a reference parameter takes '
                 '`&dyn Trait` (`&*a`) or `&Struct`. A returned handle / adaptor is compared through its *snapshot* '
                 '(null flag, bounds, continuity, D0 / D1 at fixed parameters; the module `geom` / `geom2d` / adaptor traits '
                 'of the translation provide them). A `Bnd_Box` argument is `&BndBox` / `&mut BndBox` (`occt_port::bnd`); a '
                 '`Bnd_Box&` is filled by the call and compared afterwards. `GeomAbs_*` enumerations are looked up in '
                 '`occt_port::geomabs` unless names.tsv gives the full path (`enum GeomAbs_Shape<TAB>occt_port::plib::GeomAbsShape`).\n')
    if with_gaps:
        L.append('## Coverage\n')
        L.append('| | callables (ctors, methods, statics, free functions) |\n|---|---|')
        L.append('| generated | %d |' % cov['gen'])
        L.append('| public API | %d |' % cov['total'])
        L.append('| fraction | %.1f %% |' % (100.0 * cov['gen'] / max(1, cov['total'])))
        if 'enums' in cov:
            L.append('| enumerations used by the families | %d of %d |' % cov['enums'])
        L.append('')
    if parts:
        L.append('## Parts\n')
        L.append('The judge is split in parts to keep every `.txt` under the size limit; each part is a standalone judge '
                 '(`oracle/<part>.cpp`, `tests/<part>.rs`, `tests/<part>.txt`, `docs/api-<part>.md` with its signatures): '
                 + ', '.join('`%s`' % p for p in parts) + '.\n')
    entries = doc_entries(plans, m, ctor_throws)
    free = [(c, q) for c, q in entries if c is None]
    if free:
        L.append('## Module functions\n')
        L.append('```rust')
        for c, q in free:
            L.append(sig_of(q, rc))
        L.append('```\n')
    byclass = collections.OrderedDict()
    for c, q in entries:
        if c is not None:
            byclass.setdefault(c, []).append(q)
    for c, qs in byclass.items():
        L.append('## `%s`\n' % c.short)
        if c.name in GP_BY_CLASS:
            L.append('gp value type `%s` (Rust `%s`).\n' % (c.name, GP[GP_BY_CLASS[c.name]][1]))
        if c.bases:
            L.append('C++ bases: %s (their public methods are available on `%s`).\n' % (', '.join(c.bases), c.short))
        L.append('```rust\nimpl %s {' % GP[GP_BY_CLASS[c.name]][1] if c.name in GP_BY_CLASS else '```rust\nimpl %s {' % c.short)
        for q in qs:
            L.append('    ' + sig_of(q, rc))
        L.append('}\n```\n')
        gp_ = [p for p in plans if p.kind == 'ctor_obj' and p.cls is c]
        if gp_ and gp_[0].getters:
            L.append('Snapshot getters: ' + ', '.join('`%s`' % g.rust for g in gp_[0].getters) + '.\n')
    if m.enums:
        L.append('## Enumerations\n')
        for e, cs in m.enums.items():
            if m.ctx.enums.get(e) is None:
                continue
            L.append('```rust\n#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub enum %s { %s }\n```\n' % (
                enum_short(e, rc.names), ', '.join(rc.enum_variants(e))))
    if m.ctx.used_ns:
        L.append('## Result structs\n')
        L.append('Plain data structs of the namespace `%s` that the families read field by field (public fields, in this order):\n' % m.pkg)
        L.append('```rust')
        for sn in m.ctx.used_ns:
            L.append(struct_decl(m.ctx, sn, rc))
        L.append('```\n')
    ext = [e for e in sorted(m.ctx.used_enums) if e not in m.enums]
    if ext:
        L.append('Enumerations of other packages used by the families (must exist in their modules, variants as above): ' +
                 ', '.join('`%s` = %s' % (e, ', '.join(rc.enum_variants(e))) for e in ext) + '.\n')
    if m.aliases:
        L.append('## Aliases\n')
        L.append('These names of the package are only (deprecated) aliases of classes of another package; a judge for the '
                 'target package covers them.\n')
        L.append('| name | alias of |\n|---|---|')
        for a_, t_ in m.aliases.items():
            L.append('| `%s` | `%s` |' % (a_, t_))
        L.append('')
    if with_gaps:
        L.append('## Not generated (needs a hand-written family)\n')
        L.append('Each item below is part of the public API but was not driven mechanically; the reviewer must add a family '
                 'for it or accept the gap.\n')
        L.append('| C++ declaration | reason |\n|---|---|')
        for txt, why, fn in sorted(notgen or [], key=lambda x: x[0]):
            L.append('| `%s` | %s |' % (txt.replace('|', '\\|'), why.replace('|', '\\|')))
        L.append('')
    open(path, 'w').write('\n'.join(L))


def write_stub(path, m, plans, rc, ctor_throws=None):
    """A signature-only Rust module (bodies are `unimplemented!()`), used to check that the generated test compiles.
    Enums of other packages are written to <dir>/<package lower case>.rs (add `pub mod <pkg>;` for each)."""
    ext = collections.OrderedDict()
    for e in sorted(m.ctx.used_enums):
        if e not in m.enums and m.ctx.enums.get(e) and '::' not in enum_short(e, rc.names):
            ext.setdefault(e.split('_')[0].lower(), []).append(e)
    L = ['//! STUB generated by tools/judgegen.py: signatures only, to check that tests compile.',
         '#![allow(unused_variables, dead_code, clippy::all, unreachable_pub, missing_debug_implementations)]',
         '#![allow(unused_imports)]',
         'use crate::gp::{self, %s};' % ', '.join(n_ for n_ in ('Ax1', 'Ax2', 'Ax22d', 'Ax2d', 'Ax3', 'Circ', 'Circ2d', 'Cone', 'Cylinder', 'Dir', 'Dir2d',
                                                    'Elips', 'Elips2d', 'Hypr', 'Hypr2d', 'Lin', 'Lin2d', 'Mat', 'Mat2d', 'Parab', 'Parab2d',
                                                    'Pln', 'Pnt', 'Pnt2d', 'Quaternion', 'Sphere', 'Torus', 'Trsf', 'Trsf2d', 'Vec2d', 'XY',
                                                    'XYZ') if n_ not in rc.clash),
         'use crate::plib::Array2;', 'use std::sync::Arc;', 'use crate as occt_port;']
    for mod_, es in ext.items():
        L.append('use crate::%s::{%s};' % (mod_, ', '.join(enum_short(e, rc.names) for e in es)))
    L += ['', '#[derive(Debug)]', 'pub struct Error;', '']
    for mod_, es in ext.items():
        with open(os.path.join(os.path.dirname(path), mod_ + '.rs'), 'w') as f:
            f.write('//! STUB generated by tools/judgegen.py: enumerations of package %s used by the generated tests.\n' % mod_)
            for e in es:
                f.write('#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub enum %s { %s }\n\n' % (
                    enum_short(e, rc.names), ', '.join(rc.enum_variants(e))))
    for e, cs in m.enums.items():
        if m.ctx.enums.get(e) is None:
            continue
        L.append('#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub enum %s { %s }\n' % (
            enum_short(e, rc.names), ', '.join(rc.enum_variants(e))))
    for sn in m.ctx.used_ns:
        L.append(struct_decl(m.ctx, sn, rc))
        L.append('')
    entries = doc_entries(plans, m, ctor_throws)
    classes = collections.OrderedDict()
    for c, q in entries:
        if c is not None:
            classes.setdefault(c, []).append(q)
    # methods of a base class are also available on its derived classes
    def ancestors(c):
        out_ = []
        for b in c.bases:
            bc = m.ctx.classes.get(split_type(b)['base'])
            if bc:
                out_.append(bc)
                out_ += ancestors(bc)
        return out_
    for c in list(m.ctx.classes.values()):
        for a in ancestors(c):
            for q in list(classes.get(a, [])):
                if q.kind != 'ctor_gp' and q.fn is not None and q.fn.kind == 'method':
                    classes.setdefault(c, []).append(q)
    for c in m.ctx.classes.values():
        if c.name in GP_BY_CLASS or c.name == m.pkg:
            continue
        L.append('#[derive(Debug)]\npub struct %s;' % c.short)
    L.append('')
    body = lambda s: s.rstrip(';') + ' {\n        unimplemented!()\n    }'
    for c, qs in classes.items():
        if c.name in GP_BY_CLASS:
            continue
        L.append('impl %s {' % c.short)
        seen = set()
        for q in qs:
            s = sig_of(q, rc).replace('Self', c.short)
            key = re.sub(r'\(.*', '', s)
            if key in seen or s.startswith('//'):
                continue
            seen.add(key)
            L.append('    ' + body(s))
        L.append('}\n')
    for c, q in entries:
        if c is None and not sig_of(q, rc).startswith('//'):
            L.append(body(sig_of(q, rc)).replace('\n        ', '\n    ').replace('\n    }', '\n}'))
    open(path, 'w').write('\n'.join(L).replace('Result<', 'Result<') + '\n')


# --------------------------------------------------------------------------------------------------------------
# driver

def parse_errors(stderr, cpp_path, ranges, obj_ranges):
    """-> (set of family names, set of class names, first message) implicated by compiler errors."""
    fams, objs = set(), set()
    first = None
    in_err = False
    base = os.path.basename(cpp_path)
    rx = re.compile(r'^(.*?):(\d+):(\d+): (error|note|warning|fatal error)')
    for line in stderr.splitlines():
        m_ = rx.match(line)
        if not m_:
            continue
        kind = m_.group(4)
        if kind in ('error', 'fatal error'):
            in_err = True
            if first is None:
                first = line.strip()
        elif kind == 'warning':
            in_err = False
        if in_err and os.path.basename(m_.group(1)) == base:
            ln = int(m_.group(2))
            for n, (a, b) in ranges.items():
                if a <= ln <= b:
                    fams.add(n)
            for n, (a, b) in obj_ranges.items():
                if a <= ln <= b:
                    objs.add(n)
    return fams, objs, first


class LinkError(Exception):
    pass


def parse_link_errors(stderr, plan_names):
    """-> (families, classes, symbols) whose code references a symbol the linker cannot find."""
    fams, objs, syms = set(), set(), set()
    cur = None
    for line in stderr.splitlines():
        mm = re.search(r"in function `(.*)'", line)
        if mm:
            cur = mm.group(1)
        mu = re.search(r"undefined reference to `(.*)'", line)
        if mu and cur is not None:
            syms.add(re.sub(r'\(.*$', '', mu.group(1)))
            mf = re.search(r'\bfam_(\d+)\(\)', cur)
            mo = re.search(r'\b(?:mk|gen)_obj_(\w+?)\(', cur)
            if mf and int(mf.group(1)) < len(plan_names):
                fams.add(plan_names[int(mf.group(1))])
            elif mo:
                objs.add(mo.group(1))
    return fams, objs, syms


def read_meta(path):
    meta = {}
    if os.path.exists(path):
        for line in open(path):
            p = line.rstrip('\n').split('\t')
            if len(p) == 5:
                meta[p[0]] = dict(lines=int(p[1]), libm=p[2] == '1', throws=int(p[3]), dropped=int(p[4]))
    return meta


def rustfmt_file(path, config, outdir):
    """Formats a generated Rust file with rustfmt (edition 2024) if rustfmt is installed."""
    if not shutil.which('rustfmt'):
        return
    cfg = config
    if not cfg:
        for start in (os.getcwd(), outdir):
            d = os.path.abspath(start)
            while True:
                if os.path.exists(os.path.join(d, 'rustfmt.toml')):
                    cfg = os.path.join(d, 'rustfmt.toml')
                    break
                if os.path.dirname(d) == d:
                    break
                d = os.path.dirname(d)
            if cfg:
                break
    cmd = ['rustfmt', '--edition', '2024'] + (['--config-path', cfg] if cfg else []) + [path]
    r = run(cmd)
    if r.returncode:
        print('   WARNING: rustfmt failed on %s: %s' % (path, r.stderr.strip()[:200]), flush=True)


def obj_const(ctx, cname):
    """True if every constructor of the class that can be driven has no parameter at all."""
    return all(f is None or not f.params for f in ctx.classes[cname].drv_ctors)


def single_line(plan, ctx):
    return bool(plan.words) and all(w.startswith('obj_') and obj_const(ctx, w[4:]) for w in plan.words)


def small_domain(plan, ctx):
    n = 1
    for w in plan.words:
        if w == 'b':
            n *= 2
        elif w.startswith('e_'):
            n *= len(ctx.enums[w[2:]])
        else:
            return None
    return n if n < 16 else None


def main():
    ap = argparse.ArgumentParser(description='generate OCCT -> Rust translation judges mechanically')
    ap.add_argument('package')
    ap.add_argument('module')
    ap.add_argument('outdir')
    ap.add_argument('--libs', nargs='*', default=['TKMath', 'TKernel'], help='OCCT toolkits to link (without lib / -l)')
    ap.add_argument('--names', help='names.tsv with renames')
    ap.add_argument('--occt', default=os.environ.get('OCCT') or os.environ.get('OCCT_DIR'),
                    help='OCCT install prefix (include/opencascade, lib); default $OCCT')
    ap.add_argument('--src', default=os.environ.get('OCCT_SRC'),
                    help='OCCT source tree (the directory that holds src/); default $OCCT_SRC or <occt>/../occt-src. The sources '
                         'of the package are compiled into the oracle WITHOUT -DNo_Exception, so that the range / domain checks '
                         'that the library build compiled out are active (an "unchecked" call raises like the documentation says)')
    ap.add_argument('--rustfmt-config', help='rustfmt.toml used to format the generated test (default: the first rustfmt.toml '
                                              'found above the current directory or the output directory)')
    ap.add_argument('--no-checks', action='store_true', help='link the library as built (checks compiled out) instead')
    ap.add_argument('--cases', type=int, default=24, help='cases per family (at least 16)')
    ap.add_argument('--rust-mod', help='Rust module of the translation (default: package in lower case)')
    ap.add_argument('--stub', action='store_true', help='also write <outdir>/src/<rust-mod>.rs, a signature stub')
    ap.add_argument('--keep-build', action='store_true', help='keep <outdir>/.build (needed for --rust-only)')
    ap.add_argument('--rust-only', action='store_true',
                    help='rewrite only tests/<module>.rs and the docs (new names.tsv) from the state of an earlier --keep-build run')
    ap.add_argument('--max-bytes', type=int, default=1_500_000, help='size limit of one .txt; larger judges are split in parts')
    args = ap.parse_args()
    if not args.occt:
        sys.exit('give --occt DIR (or set $OCCT)')
    pkg, mod = args.package, args.module
    out = os.path.abspath(args.outdir)
    rustmod = args.rust_mod or pkg.lower()
    for d in ('oracle', 'tests', 'docs', '.build'):
        os.makedirs(os.path.join(out, d), exist_ok=True)
    tmp = os.path.join(out, '.build')
    inc = os.path.join(args.occt, 'include', 'opencascade')
    libdir = os.path.join(args.occt, 'lib')
    cxx = os.environ.get('CXX', 'c++')
    state_path = os.path.join(tmp, 'state.json')

    srcroot = args.src or os.environ.get('OCCT_SRC') or os.path.join(os.path.dirname(os.path.abspath(args.occt)), 'occt-src')
    pkgdir = None
    for mod_ in sorted(glob.glob(os.path.join(srcroot, 'src', '*', '*', pkg))):
        if os.path.isdir(mod_):
            pkgdir = mod_
            break
    if pkgdir:
        EXTRA_INC.append('-I' + pkgdir)
    print('[1/6] reading the AST of package %s' % pkg, flush=True)
    hdrs, decls = collect(pkg, None, args.occt, tmp)
    names = load_names(args.names)
    bad_classes = set()
    excluded = {}                      # family -> reason
    cpp_path = os.path.join(out, 'oracle', mod + '.cpp')

    def prepare():
        ctx = Ctx(pkg, args.occt)
        m = build_model(pkg, decls, ctx, hdrs)
        for b in bad_classes:
            if b in ctx.classes:
                ctx.classes[b].abstract = True       # not drivable any more
        finish_model(m)
        for b in bad_classes:
            ctx.drivable.discard(b)
        assign_names(m, names)
        for e, cs in ctx.enums.items():
            if cs:
                ENUM_N[e] = cs
        plans, notgen = plan_all(m)
        return ctx, m, plans, notgen

    src_objs = []
    src_files = []
    src_skipped = []
    if not args.no_checks:
        if pkgdir is None:
            print('WARNING: no source directory for %s under %s: the library is linked as built (checks compiled out)' % (
                pkg, srcroot), flush=True)
        else:
            cxxs = sorted(glob.glob(os.path.join(pkgdir, '*.cxx')))
            objdir = os.path.join(tmp, 'obj')
            os.makedirs(objdir, exist_ok=True)

            srcdir = os.path.join(objdir, 'src')
            os.makedirs(srcdir, exist_ok=True)

            def build_one(c_):
                o_ = os.path.join(objdir, os.path.basename(c_)[:-4] + '.o')
                # some sources switch single checks off with `#define No_Standard_OutOfRange` (a speed decision of the
                # library); the copy that is compiled drops those lines so that the documented checks fire
                txt_ = open(c_, errors='replace').read()
                txt_ = re.sub(r'^[ \t]*#[ \t]*define[ \t]+No_Standard_\w+[ \t]*$', '', txt_, flags=re.M)
                tmp_ = os.path.join(srcdir, os.path.basename(c_))
                open(tmp_, 'w').write(txt_)
                r_ = run([cxx, '-std=c++17', '-O2', '-DNDEBUG', '-w', '-c', tmp_, '-I' + inc, '-I' + pkgdir, '-o', o_])
                return c_, o_, r_.returncode == 0
            import concurrent.futures
            print('   compiling %d sources of %s with the checks enabled' % (len(cxxs), pkg), flush=True)
            with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, os.cpu_count() or 2)) as ex:
                for c_, o_, ok_ in ex.map(build_one, cxxs):
                    if ok_:
                        src_objs.append(o_)
                        src_files.append(c_)
                    else:
                        src_skipped.append(os.path.basename(c_))
            if src_skipped:
                print('   WARNING: not rebuilt with checks (compile error): %s' % ', '.join(src_skipped), flush=True)

    if src_files:
        BUILD_NOTE[0] = ('Build: c++ -std=c++17 -O2 <this file> %s -I$OCCT/include/opencascade -I<package dir> -L$OCCT/lib %s -ldl; the '
                         'package sources are compiled WITHOUT -DNo_Exception, so the range / domain checks that the library build '
                         'compiled out are active (an out-of-range index raises as documented instead of reading garbage; `#define No_Standard_*` lines of the sources are dropped)' % (
                             '$OCCT_SRC/src/*/*/%s/*.cxx' % pkg, ' '.join('-l' + l for l in args.libs)))
        CHECKS_NOTE[0] = ('The oracle links the sources of the package compiled **with the checks enabled** (the shipped OCCT '
                          'libraries are built with `-DNo_Exception`, which compiles every `Raise_if` out: a call that the '
                          'documentation says raises would read garbage there). A `throws` line therefore means "the documented '
                          'check fires"; the Rust API must return `Err`.' + (
                              ' Sources that did not compile standalone and use the library as built: %s.' % ', '.join(src_skipped)
                              if src_skipped else ''))
    else:
        BUILD_NOTE[0] = ('Build: c++ -std=c++17 -O2 <this file> -I$OCCT/include/opencascade -L$OCCT/lib %s -ldl; the library '
                         'is built with -DNo_Exception (range / domain checks compiled out)' % ' '.join('-l' + l for l in args.libs))
        CHECKS_NOTE[0] = ('The oracle links the OCCT libraries as built, with `-DNo_Exception`: checks that the documentation '
                          'describes may be compiled out (`unchecked`: the call returns garbage instead of raising), so a '
                          'value line can be an out-of-contract input; use `--src` to rebuild the package with the checks.')

    def compile_run_cpp(path, optimize):
        exe = os.path.join(tmp, 'oracle-bin')
        cmd = [cxx, '-std=c++17', optimize, '-w', path] + src_objs + ['-I' + inc] + EXTRA_INC + ['-L' + libdir] + \
              ['-l' + l for l in args.libs] + ['-ldl', '-o', exe]
        r = run(cmd)
        if r.returncode:
            raise LinkError(r.stderr)
        return exe

    def run_oracle(exe, env_extra, out_txt, meta_path):
        env = dict(os.environ)
        env['LD_LIBRARY_PATH'] = libdir
        env.update(env_extra)
        env['JG_META'] = meta_path
        with open(out_txt, 'w') as fo:
            r = subprocess.run([exe], stdout=fo, stderr=subprocess.PIPE, text=True, env=env)
        if r.returncode:
            sys.exit('oracle failed (%d):\n%s' % (r.returncode, r.stderr[-3000:]))
        return r.stderr

    parts_spec = collections.OrderedDict()      # part module -> dict(names, meta, nlines, fsize)
    if args.rust_only:
        st = json.load(open(state_path))
        excluded = st['excluded']
        bad_classes = set(st['bad'])
        inout = set(st['inout'])
        cases = st['cases']
        ctx, m, plans, notgen = prepare()
        enum_hdrs = {e for e in ctx.used_enums if e not in m.enums}
        for p in plans:
            p.inout = {int(x.rsplit(':', 1)[1]) for x in inout if x.rsplit(':', 1)[0] == p.name}
            p.words = plan_words(p)
        parts_spec = collections.OrderedDict(st['parts'])
    else:
        print('[2/6] planning and checking that the oracle compiles', flush=True)
        for it in range(15):
            ctx, m, plans, notgen = prepare()
            if ctx.saw_handle:
                for l_ in H_LIBS:
                    if l_ not in args.libs:
                        args.libs.append(l_)
            enum_hdrs = {e for e in ctx.used_enums if e not in m.enums}
            text, ranges, obj_ranges = build_cpp_full(m, plans, set(excluded), args.cases, set(), hdrs, enum_hdrs)
            open(cpp_path, 'w').write(text)
            r = run([cxx, '-std=c++17', '-fsyntax-only', '-w', '-I' + inc] + EXTRA_INC + [cpp_path])
            if r.returncode == 0:
                break
            fams, objs, first = parse_errors(r.stderr, cpp_path, ranges, obj_ranges)
            if not fams and not objs:
                sys.exit('compile error outside any family:\n' + r.stderr[-3000:])
            for n in fams:
                excluded[n] = 'does not compile against the headers (%s)' % (first or '?')[-160:].split('error: ')[-1]
            bad_classes |= objs
            print('   excluded %d families, %d classes after compile errors' % (len(fams), len(objs)), flush=True)
        else:
            sys.exit('could not get a compiling oracle')

        print('[3/6] exploratory run', flush=True)
        for it in range(12):
            try:
                exe = compile_run_cpp(cpp_path, '-O1')
                break
            except LinkError as e_:
                # a method that the headers declare but the library does not define (e.g. AppDef_MultiLine::SetParameter)
                fams, objs, syms = parse_link_errors(e_.args[0], [p.name for p in plans])
                if not fams and not objs:
                    sys.exit('oracle does not link:\n' + e_.args[0][-3000:])
                for n in fams:
                    excluded[n] = 'not defined in the OCCT library (declared only: %s)' % ', '.join(sorted(syms))[:160]
                bad_classes |= objs
                print('   undefined symbols: excluded %d families, %d classes (%s)' % (
                    len(fams), len(objs), ', '.join(sorted(syms))[:200]), flush=True)
                ctx, m, plans, notgen = prepare()
                enum_hdrs = {e for e in ctx.used_enums if e not in m.enums}
                text, ranges, obj_ranges = build_cpp_full(m, plans, set(excluded), args.cases, set(), hdrs, enum_hdrs)
                open(cpp_path, 'w').write(text)
        else:
            sys.exit('could not get a linking oracle')
        inout = set()
        if any(p.cands for p in plans if p.name not in excluded):
            probe_out = os.path.join(tmp, 'probe.txt')
            env = dict(os.environ)
            env['LD_LIBRARY_PATH'] = libdir
            env['JG_PROBE'] = probe_out
            r = subprocess.run([exe], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, env=env)
            if r.returncode:
                sys.exit('probe run failed:\n' + r.stderr[-3000:])
            inout = {l.strip() for l in open(probe_out) if l.strip()}
            print('   in-out parameters found by probing: %d (%s)' % (len(inout), ', '.join(sorted(inout))[:200]), flush=True)
        for p in plans:
            p.inout = {int(x.rsplit(':', 1)[1]) for x in inout if x.rsplit(':', 1)[0] == p.name}
            p.words = plan_words(p)
        txt1 = os.path.join(tmp, 'explore.txt')
        meta_path = os.path.join(tmp, 'meta.tsv')
        cases = args.cases
        run_oracle(exe, {'JG_CASES': str(cases), 'JG_INOUT': ','.join(sorted(inout))}, txt1, meta_path)
        meta = read_meta(meta_path)
        fam_bytes = collections.Counter()
        for line in open(txt1):
            fam_bytes[line.split(' ', 1)[0]] += len(line)
        for p in plans:
            if p.name in excluded:
                continue
            n = meta.get(p.name, {}).get('lines', 0)
            sd = small_domain(p, ctx)
            if not p.words or single_line(p, ctx):
                ok = n == 1
            elif sd is not None:
                ok = n >= 1
            else:
                ok = n >= 16
            if not ok:
                excluded[p.name] = ('fewer than 16 distinct stable cases (%d; the others raised no stable value, were not '
                                    'reproducible under random stack/heap fill, or were not finite)' % n) if p.words else 'no stable case'
        runnable = [p for p in plans if p.name not in excluded]
        LIMIT = int(args.max_bytes * 0.93)
        total = sum(fam_bytes[p.name] for p in runnable)

        def scaled(p, c):   # estimated bytes of a family with c cases
            n = max(1, meta.get(p.name, {}).get('lines', 1))
            return fam_bytes[p.name] * min(c, n) / n
        parts = [runnable]
        if total > LIMIT:
            total16 = sum(scaled(p, 16) for p in runnable)
            if total16 <= LIMIT:
                cases = max(16, min(cases, int(cases * LIMIT / total)))
            else:
                cases = 16
                order = sorted(runnable, key=lambda p: ((p.cls.name if p.cls else ''), p.name))
                parts, cur, size = [], [], 0
                for p in order:
                    sz = scaled(p, 16)
                    if cur and size + sz > LIMIT:
                        parts.append(cur)
                        cur, size = [], 0
                    cur.append(p)
                    size += sz
                if cur:
                    parts.append(cur)
        print('   %d families, %d excluded, %d bytes at %d cases -> %d part(s), %d cases' % (
            len(meta), len(excluded), total, args.cases, len(parts), cases), flush=True)

        for k, part in enumerate(parts, 1):
            pmod = mod if len(parts) == 1 else '%s_%d' % (mod, k)
            print('[4/6] part %s: final oracle (baked cases and in-out list), two runs' % pmod, flush=True)
            keep_names = {p.name for p in part}
            drop = {p.name for p in plans if p.name not in keep_names}
            text, ranges, obj_ranges = build_cpp_full(m, plans, drop, cases, set(), hdrs, enum_hdrs, inout)
            pcpp = os.path.join(out, 'oracle', pmod + '.cpp')
            open(pcpp, 'w').write(text)
            exe = compile_run_cpp(pcpp, '-O2')
            by_plan_all = {p_.name: p_ for p_ in plans}
            t1, t2 = os.path.join(tmp, 'run1.txt'), os.path.join(tmp, 'run2.txt')
            mp = os.path.join(tmp, 'meta_%d.tsv' % k)
            run_oracle(exe, {}, t1, os.path.join(tmp, 'meta1.tsv'))
            run_oracle(exe, {}, t2, mp)
            badk = set()
            if open(t1, 'rb').read() == open(t2, 'rb').read():
                # a third run with address space randomization: a line whose outputs differ depends on an address
                t3 = os.path.join(tmp, 'run3.txt')
                run_oracle(exe, {'JG_ASLR': '1'}, t3, os.path.join(tmp, 'meta3.tsv'))
                ref = {}
                for l_ in open(t2):
                    ref[l_.partition(' |')[0]] = l_
                badk = set()
                for l_ in open(t3):
                    k_ = l_.partition(' |')[0]
                    if k_ in ref and ref[k_] != l_:
                        badk.add(k_)
                if badk:
                    fams_ = collections.Counter(k_.split(' ', 1)[0] for k_ in badk)
                    print('   WARNING: %d lines print an address-dependent value (they differ when address space randomization '
                          'is on) and are dropped: %s' % (len(badk), ', '.join('%s (%d)' % kv for kv in fams_.items())), flush=True)
                    keep_ = [l_ for l_ in open(t2) if l_.partition(' |')[0] not in badk]
                    open(t2, 'w').write(''.join(keep_))
                    cnt_ = collections.Counter(l_.split(' ', 1)[0] for l_ in keep_)
                    meta_ = read_meta(mp)
                    for n_ in fams_:
                        pl_ = by_plan_all.get(n_)
                        need_ = 1 if (pl_ is None or not pl_.words or single_line(pl_, ctx) or small_domain(pl_, ctx) is not None) else 16
                        if n_ in meta_:
                            meta_[n_]['lines'] = cnt_.get(n_, 0) if cnt_.get(n_, 0) >= need_ else 0
                            if meta_[n_]['lines'] == 0:
                                keep_ = [l_ for l_ in keep_ if l_.split(' ', 1)[0] != n_]
                                excluded[n_] = 'output depends on an address (differs with address space randomization)'
                    open(t2, 'w').write(''.join(keep_))
                    with open(mp, 'w') as f_:
                        for n_ in [p_.name for p_ in part]:
                            if n_ in meta_:
                                mm_ = meta_[n_]
                                f_.write('%s\t%d\t%d\t%d\t%d\n' % (n_, mm_['lines'], 1 if mm_['libm'] else 0, mm_['throws'], mm_['dropped']))
            if not badk and open(t1, 'rb').read() != open(t2, 'rb').read():
                l1, l2 = open(t1).read().splitlines(), open(t2).read().splitlines()
                bad = collections.OrderedDict()
                for i_ in range(max(len(l1), len(l2))):
                    a_ = l1[i_] if i_ < len(l1) else ''
                    b_ = l2[i_] if i_ < len(l2) else ''
                    if a_ != b_:
                        bad.setdefault((a_ or b_).split(' ', 1)[0], (i_ + 1, a_, b_))
                msg = ['the oracle output is not deterministic (two runs differ); families that differ: %s' % ', '.join(bad)]
                for n_, (i_, a_, b_) in list(bad.items())[:3]:
                    msg.append('  %s line %d:\n    run 1: %s\n    run 2: %s' % (n_, i_, a_[:300], b_[:300]))
                sys.exit('\n'.join(msg))
            final_txt = os.path.join(out, 'tests', pmod + '.txt')
            shutil.copyfile(t2, final_txt)
            nlines = sum(1 for _ in open(final_txt))
            fsize = os.path.getsize(final_txt)
            if fsize > args.max_bytes:
                print('WARNING: %s is %d bytes (> %d); rerun with a smaller --cases or a bigger --max-bytes' % (
                    final_txt, fsize, args.max_bytes))
            ct = collections.defaultdict(set)
            by_plan = {p.name: p for p in part}
            for line in open(final_txt):
                head, _, tail = line.partition(' |')
                w = head.split()
                if len(w) >= 2 and w[0] in by_plan and by_plan[w[0]].kind == 'ctor_obj' and tail.strip() == 'throws':
                    ct[w[0]].add(int(w[1]))
            parts_spec[pmod] = dict(names=[p.name for p in part], meta=read_meta(mp), nlines=nlines, fsize=fsize,
                                    ctor_throws={k: sorted(v) for k, v in ct.items()})
        json.dump(dict(excluded=excluded, bad=sorted(bad_classes), inout=sorted(inout), cases=cases, parts=parts_spec),
                  open(state_path, 'w'))

    print('[5/6] writing the tests and the API docs', flush=True)
    rc = RsCtx(m, rustmod, names)
    allfns = []
    for c in ctx.classes.values():
        allfns += c.ctors + c.fns
    allfns += m.free
    nops = sum(1 for c in ctx.classes.values() for t, why in c.skipped if why.startswith('operator'))
    summaries = []
    all_kept = []
    ctor_throws = {}
    by_name = {p.name: p for p in plans}
    for pmod, spec in parts_spec.items():
        pmeta = spec['meta']
        gone = {n for n in spec['names'] if n not in by_name}
        if gone and args.rust_only:
            # families that names.tsv now skips: drop their lines (rerun without --rust-only to regenerate the oracle)
            txt_path = os.path.join(out, 'tests', pmod + '.txt')
            lines_ = [l for l in open(txt_path) if l.split(' ', 1)[0] not in gone]
            open(txt_path, 'w').write(''.join(lines_))
            spec['nlines'] = len(lines_)
            spec['names'] = [n for n in spec['names'] if n not in gone]
            print('   %s: dropped %d skipped families from the .txt (oracle.cpp still prints them)' % (pmod, len(gone)))
        kept = [by_name[n] for n in spec['names'] if pmeta.get(n, {}).get('lines', 0) > 0]
        for p in kept:
            mt = pmeta[p.name]
            p.lines, p.libm, p.throws = mt['lines'], mt['libm'], mt['throws']
            p.can_throw = mt['throws'] > 0 or bool(p.fn and p.fn.raises)
        all_kept += kept
        rd_text, emit_text = rs_readers(m, rc, kept)
        arms = '\n'.join(rs_arm(p, rc) for p in kept)
        input_less = [p.name for p in kept if not p.words or single_line(p, ctx)]
        small = [(p.name, p.lines) for p in kept if p.words and not single_line(p, ctx) and small_domain(p, ctx) is not None]
        exact = [p.name for p in kept if not p.libm]
        consts = '\n'.join([
            '/// Families that really take no input: they are printed exactly once.',
            'const INPUT_LESS: &[&str] = &[%s];' % ', '.join('"%s"' % n for n in input_less),
            '',
            '/// Families whose whole input domain is printed (so they are exempt from MIN_DISTINCT): name and number of lines.',
            'const SMALL_DOMAIN: &[(&str, usize)] = &[%s];' % ', '.join('("%s", %d)' % x for x in small),
            '',
            '/// Families compared bit for bit: no case of them calls a C library transcendental function. The others use MAX_ULPS.',
            'const EXACT: &[&str] = &[\n%s\n];' % ''.join('    "%s",\n' % n for n in exact).rstrip('\n'),
            '',
            '/// Every family the oracle must print (a family that disappears from the .txt fails).',
            'const FAMILIES: &[&str] = &[\n%s\n];' % ''.join('    "%s",\n' % p.name for p in sorted(kept, key=lambda p: p.name)).rstrip('\n'),
            '',
            '/// The number of lines of tests/%s.txt.' % pmod,
            'const GOLDEN_LINES: usize = %d;' % spec['nlines'],
        ])
        rs = RS_HEAD.replace('@@USES@@', RS_GP_USES).replace('@@CONSTS@@', consts)
        rs += RS_BODY.replace('@@EMITS@@', RS_GP_EMITS + '\n' + emit_text).replace('@@READERS@@', RS_GP_READERS + '\n' + rd_text) \
            .replace('@@ARMS@@', arms)
        rs = rs.replace('@@PKG@@', pkg).replace('@@MOD@@', pmod)
        rs_path = os.path.join(out, 'tests', pmod + '.rs')
        open(rs_path, 'w').write(rs)
        rustfmt_file(rs_path, args.rustfmt_config, out)
        ctor_throws.update({k: set(v) for k, v in spec.get('ctor_throws', {}).items()})
        if len(parts_spec) > 1:
            write_doc(os.path.join(out, 'docs', 'api-%s.md' % pmod), m, kept, [], rc, args, covered_count(kept), None,
                      pmod=pmod, with_gaps=False, ctor_throws=ctor_throws)
        summaries.append((pmod, len(kept), spec['nlines'], spec['fsize']))
    notgen = list(notgen)
    kept_names = {p.name for p in all_kept}
    for p in plans:
        if p.name in kept_names:
            continue
        why = excluded.get(p.name, 'no stable case')
        d = (p.cls.name + '::' + p.fn.cname + '(' + ', '.join(q.qt for q in p.fn.params) + ')') if p.fn else (p.cls.name + ' constructors')
        notgen.append((d, why, p.fn))
    total_cov = covered_count(all_kept)
    total_cov['total'] = len(allfns) + nops
    total_cov['enums'] = (len(ctx.used_enums & set(m.enums)), len(m.enums))
    write_doc(os.path.join(out, 'docs', 'api-%s.md' % mod), m, all_kept if len(parts_spec) == 1 else [], notgen, rc, args,
              total_cov, None, pmod=mod, with_gaps=True, parts=list(parts_spec) if len(parts_spec) > 1 else None,
              ctor_throws=ctor_throws)
    with open(os.path.join(out, 'docs', 'api-%s.names.tsv' % mod), 'w') as f:
        f.write('# effective Rust names (edit and pass back with --names to rename); key = Class::Method(sig)\n')
        for c in ctx.classes.values():
            for fn in c.ctors:
                f.write('%s::%s(%s)\t%s\n' % (c.name, c.name, fn.sig(), fn.rust))
            for fn in c.fns:
                f.write('%s::%s(%s)\t%s\n' % (c.name, fn.cname, fn.sig(), fn.rust))
        for fn in m.free:
            f.write('%s::%s(%s)\t%s\n' % (pkg, fn.cname, fn.sig(), fn.rust))
        for e, cs in m.enums.items():
            if ctx.enums.get(e):
                for c in cs:
                    f.write('%s::%s\t%s\n' % (e, c, enum_variant(m, e, c, names)))
    if len(parts_spec) > 1 and os.path.exists(cpp_path):
        os.remove(cpp_path)         # the compile-check source of the whole package; the parts have their own oracle
    if args.stub:
        os.makedirs(os.path.join(out, 'src'), exist_ok=True)
        write_stub(os.path.join(out, 'src', rustmod + '.rs'), m, all_kept, rc, ctor_throws)
    for s_ in summaries:
        print('   %s: %d families, %d lines, %d bytes' % s_)
    if m.aliases and not allfns:
        print('   NOTE: package %s only holds %d deprecated aliases (%s ...): judge the package they alias instead' % (
            pkg, len(m.aliases), ', '.join('%s=%s' % kv for kv in list(m.aliases.items())[:3])), flush=True)
    print('[6/6] done: callables generated %d of %d (%.1f %%); enums used: %d of %d; not generated: %d' % (
        total_cov['gen'], total_cov['total'], 100.0 * total_cov['gen'] / max(1, total_cov['total']),
        total_cov['enums'][0], total_cov['enums'][1], len(notgen)), flush=True)
    if not args.keep_build and not args.rust_only:
        for fn_ in os.listdir(tmp):
            try:
                os.remove(os.path.join(tmp, fn_))
            except OSError:
                pass


def covered_count(kept):
    cov = set()
    fns = []
    for p in kept:
        if p.kind == 'ctor_obj':
            for f in p.cls.drv_ctors:
                if f is not None:
                    cov.add(id(f))
                    fns.append(f)
        elif p.fn is not None:
            cov.add(id(p.fn))
            fns.append(p.fn)
    return dict(gen=len(cov))


build_cpp_full = build_cpp


if __name__ == '__main__':
    main()

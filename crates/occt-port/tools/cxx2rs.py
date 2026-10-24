#!/usr/bin/env python3
"""cxx2rs: mechanical C++ -> Rust draft from clang's JSON AST (ADR 0025 pipeline).

Usage:
  clang++ -std=c++17 -fsyntax-only -I$OCCT/include/opencascade -Xclang -ast-dump=json X.cxx > X.json
  cxx2rs.py X.json X.cxx [names.tsv] > x_draft.rs

Translates every function and method *defined in source.cxx* statement by statement,
keeping the exact order of floating-point operations (what bit-identity depends on).
Types map by a table (Standard_Real -> f64, gp_Pnt -> Pnt, ...); out-parameters
(non-const references) are kept as `&mut`. Anything it does not understand is emitted
as `todo!("<C++ kind>")` with the C++ text in a comment, so a reviewer (or an LLM pass)
only fixes the marked spots. names.tsv (optional): `Class::Method/arity<TAB>rust_name`.
"""
import json, re, sys

TYPES = {
    'Standard_Real': 'f64', 'double': 'f64', 'float': 'f32', 'Standard_ShortReal': 'f32',
    'Standard_Integer': 'i32', 'int': 'i32', 'unsigned int': 'u32', 'size_t': 'usize',
    'Standard_Boolean': 'bool', 'bool': 'bool', 'void': '()', 'char': 'u8',
    'Standard_Character': 'u8',
}

def rust_type(qt):
    t = qt.strip()
    ref = mut = ptr = False
    if t.endswith('&'):
        ref = True
        t = t[:-1].strip()
    elif t.endswith('*'):
        ptr = True
        t = t[:-1].strip()
    const = t.startswith('const ')
    if const:
        t = t[6:].strip()
    if t.endswith(' const'):
        const = True
        t = t[:-6].strip()
    base = TYPES.get(t)
    m = re.match(r'^(?:NCollection_(?:Sequence|Array1|Vector)|std::vector)<(.*)>$', t)
    if base is None and m:
        base = f'Vec<{rust_type(m.group(1))}>'
    if base is None:
        base = re.sub(r'^gp_', '', t)
        base = base.replace('math_', '').replace('::', '_')
    if ref or ptr:
        return ('&' if const else '&mut ') + base
    return base

def snake(name):
    s = re.sub(r'(?<=[a-z0-9])([A-Z])', r'_\1', name)
    s = re.sub(r'(?<=[A-Z])([A-Z][a-z])', r'_\1', s)
    return s.lower()

def fmt_float(v):
    s = repr(float(v))
    return s if ('.' in s or 'e' in s or 'inf' in s or 'nan' in s) else s + '.0'


class Tx:
    def __init__(self, names):
        self.names = names
        self.todo = 0
        self.statics = set()
        self.self_name = 'self'

    def todo_expr(self, n, why=''):
        self.todo += 1
        return f'todo!("{n.get("kind")}{": " + why if why else ""}")'

    # ------------------------------------------------------------- expressions
    def e(self, n):
        k = n.get('kind')
        inner = n.get('inner', [])
        f = getattr(self, 'x_' + k, None)
        if f:
            return f(n, inner)
        return self.todo_expr(n)

    def x_ImplicitCastExpr(self, n, inner):
        v = self.e(inner[0])
        ck = n.get('castKind')
        to = rust_type(n['type']['qualType'])
        if ck == 'IntegralToFloating':
            return f'f64::from({v})' if to == 'f64' else f'({v} as {to})'
        if ck == 'FloatingToIntegral':
            return f'({v} as {to})'
        if ck == 'IntegralCast' and to not in ('i32',):
            return f'({v} as {to})'
        return v
    x_ExprWithCleanups = x_MaterializeTemporaryExpr = x_CXXBindTemporaryExpr = x_ConstantExpr = \
        lambda self, n, inner: self.e(inner[0])

    def x_ArraySubscriptExpr(self, n, inner):
        return f'{self.e(inner[0])}[({self.e(inner[1])}) as usize]'

    def x_CXXNullPtrLiteralExpr(self, n, inner):
        return 'None'

    def x_CXXThrowExpr(self, n, inner):
        return f'return Err({self.e(inner[0]) if inner else "MathError"})'

    def x_ParenExpr(self, n, inner):
        return f'({self.e(inner[0])})'

    def x_IntegerLiteral(self, n, inner):
        return str(n['value'])

    def x_FloatingLiteral(self, n, inner):
        return fmt_float(n['value'])

    def x_CXXBoolLiteralExpr(self, n, inner):
        return 'true' if n['value'] else 'false'

    def x_DeclRefExpr(self, n, inner):
        d = n['referencedDecl']
        name = d.get('name', '?')
        if d.get('kind') in ('FunctionDecl', 'CXXMethodDecl'):
            return self.fn_name(d, None)
        if d.get('kind') == 'EnumConstantDecl':
            return name
        t = d.get('type', {}).get('qualType', '')
        # A scalar out-parameter (non-const reference) is written through.
        if d.get('kind') == 'ParmVarDecl' and t.endswith('&') and not t.startswith('const') \
                and rust_type(t).lstrip('&mut ') in ('f64', 'i32', 'bool', 'f32', 'usize'):
            return f'*{snake(name)}'
        return snake(name)

    def x_MemberExpr(self, n, inner):
        base = self.e(inner[0]) if inner else self.self_name
        if base in ('this', '(*this)', 'self', '*self'):
            base = self.self_name
        name = n.get('name', '?')
        return f'{base}.{snake(name)}'

    def x_CXXThisExpr(self, n, inner):
        return self.self_name

    def x_UnaryOperator(self, n, inner):
        op = n['opcode']
        v = self.e(inner[0])
        if op in ('-', '!'):
            return f'{op}{v}'
        if op == '+':
            return v
        if op in ('++', '--'):
            return f'{{ {v} {op[0]}= 1; }}'
        if op == '*':
            return f'*{v}'
        if op == '&':
            return f'&{v}'
        return self.todo_expr(n, op)

    def x_BinaryOperator(self, n, inner):
        op = n['opcode']
        a, b = self.e(inner[0]), self.e(inner[1])
        if op == ',':
            return f'{{ {a}; {b} }}'
        return f'{a} {op} {b}'
    x_CompoundAssignOperator = x_BinaryOperator

    def x_ConditionalOperator(self, n, inner):
        c, a, b = (self.e(x) for x in inner)
        return f'if {c} {{ {a} }} else {{ {b} }}'

    def x_CallExpr(self, n, inner):
        callee = inner[0]
        args = [self.e(a) for a in inner[1:] if a.get('kind') != 'CXXDefaultArgExpr']
        decl = self.callee_decl(callee)
        if decl is not None:
            name = self.fn_name(decl, len(args))
            name = {'Abs': 'f64::abs', 'Sqrt': 'libm::sqrt', 'Cos': 'libm::cos', 'Sin': 'libm::sin',
                    'abs': 'f64::abs', 'sqrt': 'libm::sqrt', 'cos': 'libm::cos', 'sin': 'libm::sin',
                    'atan2': 'libm::atan2', 'ATan2': 'libm::atan2', 'ACos': 'libm::acos', 'ASin': 'libm::asin',
                    'Log': 'libm::log', 'Exp': 'libm::exp', 'Pow': 'libm::pow', 'Tan': 'libm::tan',
                    'Max': 'f64::max', 'Min': 'f64::min', 'max': 'f64::max', 'min': 'f64::min',
                    'Sign': 'sign', 'Square': 'square'}.get(decl.get('name'), name)
            return f'{name}({", ".join(args)})'
        return f'{self.e(callee)}({", ".join(args)})'

    def callee_decl(self, n):
        while n.get('kind') in ('ImplicitCastExpr', 'ParenExpr'):
            n = n['inner'][0]
        if n.get('kind') == 'DeclRefExpr':
            return n['referencedDecl']
        return None

    def fn_name(self, decl, arity):
        name = decl.get('name', '?')
        key = f'{name}/{arity}'
        return self.names.get(key, self.names.get(name, snake(name)))

    def x_CXXMemberCallExpr(self, n, inner):
        me = inner[0]
        args = [self.e(a) for a in inner[1:] if a.get('kind') != 'CXXDefaultArgExpr']
        base = self.e(me['inner'][0]) if me.get('inner') else self.self_name
        name = me.get('name', '?')
        key = f'{name}/{len(args)}'
        rname = self.names.get(key, self.names.get(name, snake(name)))
        return f'{base}.{rname}({", ".join(args)})'

    def x_CXXOperatorCallExpr(self, n, inner):
        op = None
        callee = inner[0]
        d = self.callee_decl(callee)
        if d is not None:
            op = d.get('name', '').replace('operator', '')
        args = [self.e(a) for a in inner[1:]]
        if op in ('*', '+', '-', '/', '^') and len(args) == 2:
            if op == '^':
                return f'{args[0]}.crossed(&{args[1]})'
            return f'{args[0]} {op} {args[1]}'
        if op == '-' and len(args) == 1:
            return f'-{args[0]}'
        if op in ('=', '+=', '-=', '*=', '/=') and len(args) == 2:
            return f'{args[0]} {op} {args[1]}'
        if op == '()' or op == '[]':
            return f'{args[0]}[{", ".join(args[1:])}]'
        return self.todo_expr(n, f'operator{op}')

    def x_CXXConstructExpr(self, n, inner):
        t = rust_type(n['type']['qualType'])
        args = [self.e(a) for a in inner if a.get('kind') != 'CXXDefaultArgExpr']
        ctor = n.get('ctorType', {}).get('qualType', '')
        if len(args) == 1 and ('&&' in ctor or 'const ' + n['type']['qualType'] + ' &' in ctor):
            return args[0]  # copy / move
        if not args:
            return f'<{t}>::default()' if '<' in t else f'{t}::default()'
        return f'{t}::new({", ".join(args)})'
    x_CXXTemporaryObjectExpr = x_CXXConstructExpr

    def x_CXXFunctionalCastExpr(self, n, inner):
        return self.e(inner[0])
    x_CStyleCastExpr = x_CXXStaticCastExpr = lambda self, n, inner: \
        f'({self.e(inner[0])} as {rust_type(n["type"]["qualType"])})'

    # --------------------------------------------------------------- statements
    def s(self, n, ind):
        k = n.get('kind')
        inner = n.get('inner', [])
        p = '    ' * ind
        if k == 'CompoundStmt':
            return ''.join(self.s(x, ind) for x in inner)
        if k == 'DeclStmt':
            out = ''
            for v in inner:
                if v.get('kind') != 'VarDecl':
                    out += f'{p}{self.todo_expr(v)};\n'
                    continue
                t = rust_type(v['type']['qualType'])
                init = [x for x in v.get('inner', []) if x.get('kind') not in ('FullComment',)]
                val = self.e(init[0]) if init else None
                if val is None or val.endswith('::default()'):
                    dflt = f'<{t}>::default()' if '<' in t else f'{t}::default()'
                    val = {'f64': '0.0', 'i32': '0', 'bool': 'false'}.get(t, dflt)
                out += f'{p}let mut {snake(v["name"])}: {t} = {val};\n'
            return out
        if k == 'ReturnStmt':
            return f'{p}return {self.e(inner[0])};\n' if inner else f'{p}return;\n'
        if k == 'IfStmt':
            c = self.e(inner[0])
            out = f'{p}if {c} {{\n{self.s(inner[1], ind + 1)}{p}}}'
            if n.get('hasElse') and len(inner) > 2:
                els = inner[2]
                if els.get('kind') == 'IfStmt':
                    out += ' else ' + self.s(els, ind).lstrip()
                    return out
                out += f' else {{\n{self.s(els, ind + 1)}{p}}}'
            return out + '\n'
        if k == 'ForStmt':
            init, _, cond, inc, body = (inner + [None] * 5)[:5]
            out = f'{p}{{\n'
            if init and init.get('kind'):
                out += self.s(init, ind + 1)
            c = self.e(cond) if cond and cond.get('kind') else 'true'
            out += f'{p}    while {c} {{\n{self.s(body, ind + 2)}'
            if inc and inc.get('kind'):
                out += f'{p}        {self.e(inc)};\n'
            return out + f'{p}    }}\n{p}}}\n'
        if k == 'WhileStmt':
            return f'{p}while {self.e(inner[0])} {{\n{self.s(inner[1], ind + 1)}{p}}}\n'
        if k == 'DoStmt':
            return f'{p}loop {{\n{self.s(inner[0], ind + 1)}{p}    if !({self.e(inner[1])}) {{ break; }}\n{p}}}\n'
        if k == 'BreakStmt':
            return f'{p}break;\n'
        if k == 'ContinueStmt':
            return f'{p}continue;\n'
        if k == 'NullStmt':
            return ''
        if k == 'SwitchStmt':
            self.todo += 1
            return f'{p}todo!("switch on {self.e(inner[0])}");\n'
        return f'{p}{self.e(n)};\n'

    # ---------------------------------------------------------------- functions
    def function(self, n, cls):
        params = [x for x in n.get('inner', []) if x.get('kind') == 'ParmVarDecl']
        body = [x for x in n.get('inner', []) if x.get('kind') == 'CompoundStmt']
        if not body:
            return None
        ret = rust_type(n['type']['qualType'].split('(')[0])
        ps = []
        if n.get('kind') == 'CXXMethodDecl' and n.get('storageClass') != 'static' \
                and n.get('previousDecl') not in self.statics:
            ps.append('&self' if n['type']['qualType'].endswith('const') else '&mut self')
        for x in params:
            ps.append(f'{snake(x.get("name", "_"))}: {rust_type(x["type"]["qualType"])}')
        if n.get('kind') == 'CXXConstructorDecl':
            ps = [p for p in ps if p not in ('&self', '&mut self')]
            self.self_name = 'this'
            init = ''
            for x in n.get('inner', []):
                if x.get('kind') == 'CXXCtorInitializer' and x.get('anyInit') and x.get('inner'):
                    v = self.e(x['inner'][0])
                    init += f'    this.{snake(x["anyInit"]["name"])} = {v};\n'
            body_rs = self.s(body[0], 1)
            self.self_name = 'self'
            key = f'{n["name"]}/{len(params)}'
            name = self.names.get(key, 'new' if len(params) == 0 else f'new_{len(params)}')
            sig = f'pub fn {name}({", ".join(ps)}) -> Self'
            return (f'// {cls + "::" if cls else ""}{n["name"]}\n{sig} {{\n    let mut this = Self::default();\n'
                    f'{init}{body_rs}    this\n}}\n')
        name = self.fn_name(n, len(params))
        sig = f'pub fn {name}({", ".join(ps)})' + ('' if ret == '()' else f' -> {ret}')
        return f'// {cls + "::" if cls else ""}{n["name"]}\n{sig} {{\n{self.s(body[0], 1)}}}\n'


def decls(path, src=None):
    """Top-level declarations of a clang JSON dump. A full dump (one TranslationUnitDecl,
    hundreds of MB with the headers) is streamed line by line and only declarations
    located in `src` are parsed; clang prints a location's file only when it changes, so
    the current file is tracked through the whole stream."""
    with open(path) as f:
        first = f.readline()
        f.seek(0)
        if '"kind": "TranslationUnitDecl"' not in f.read(400):
            f.seek(0)
            text = f.read()
            dec = json.JSONDecoder()
            i = 0
            while True:
                i = text.find('{', i)
                if i < 0:
                    return
                obj, i = dec.raw_decode(text, i)
                yield obj
            return
        f.seek(0)
        cur = None
        buf = None
        start_file = None
        in_loc = False
        file_re = re.compile(r'"file": "([^"]*)"')
        for line in f:
            if buf is None and line == '    {\n':
                buf, start_file, in_loc = [line], cur, False
                continue
            if buf is not None:
                buf.append(line)
                if line.startswith('      "loc"'):
                    in_loc = True
                elif line.startswith('      "range"'):
                    in_loc = False
            m = file_re.search(line)
            if m:
                cur = m.group(1)
                if buf is not None and in_loc:
                    start_file = cur
            if buf is not None and line in ('    },\n', '    }\n'):
                if start_file and src in start_file:
                    yield json.loads(''.join(buf).rstrip().rstrip(','))
                buf = None


def class_of(d):
    """The class of a method, from its Itanium mangled name (`_ZN<len><Class>...`)."""
    m = re.match(r'^_ZNK?(\d+)', d.get('mangledName', ''))
    if not m or d.get('kind') not in ('CXXMethodDecl', 'CXXConstructorDecl'):
        return None
    start = m.end()
    return d['mangledName'][start:start + int(m.group(1))]


def record(d):
    """A class's data members as a Rust struct; bases and nested types become comments."""
    bases = ', '.join(b.get('type', {}).get('qualType', '?') for b in d.get('bases', []))
    lines = [f'// {d.get("tagUsed", "class")} {d["name"]}' + (f' : {bases}' if bases else '')]
    fields = [c for c in d.get('inner', []) if c.get('kind') == 'FieldDecl']
    lines.append('#[derive(Debug, Clone, Default)]')
    lines.append(f'pub struct {rust_type(d["name"])} {{')
    for f in fields:
        lines.append(f'    pub {snake(f.get("name", "_"))}: {rust_type(f["type"]["qualType"])},')
    lines.append('}\n')
    return '\n'.join(lines)


def enum(d):
    names = [c['name'] for c in d.get('inner', []) if c.get('kind') == 'EnumConstantDecl']
    body = ''.join(f'    {n},\n' for n in names)
    return f'#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum {rust_type(d["name"])} {{\n{body}}}\n'


def main():
    ast, src = sys.argv[1], sys.argv[2]
    names = {}
    if len(sys.argv) > 3:
        for line in open(sys.argv[3]):
            if '\t' in line:
                k, v = line.rstrip('\n').split('\t')
                names[k] = v
    tx = Tx(names)
    out, seen = [], set()
    all_decls = list(decls(ast, src))

    def walk(n):
        if n.get('kind') == 'CXXMethodDecl' and n.get('storageClass') == 'static':
            tx.statics.add(n['id'])
        for c in n.get('inner', []):
            walk(c)
    for d in all_decls:
        walk(d)
    types = []
    for d in all_decls:
        if d.get('kind') == 'CXXRecordDecl' and d.get('completeDefinition') and d.get('name'):
            cls = d['name']
            types.append(record(d))
            # Methods defined inside the class body (the inline code of headers).
            for c in d.get('inner', []):
                if c.get('kind') in ('CXXMethodDecl', 'CXXConstructorDecl') and not c.get('isImplicit') \
                        and not c.get('name', '').startswith(('operator new', 'operator delete')):
                    r = tx.function(c, cls)
                    if r and c['id'] not in seen:
                        seen.add(c['id'])
                        out.append((cls, r))
            continue
        if d.get('kind') == 'EnumDecl' and d.get('name'):
            types.append(enum(d))
            continue
        if d.get('kind') not in ('FunctionDecl', 'CXXMethodDecl', 'CXXConstructorDecl'):
            continue
        loc = d.get('loc', {})
        if src not in (loc.get('file') or '') and loc.get('file') is not None:
            continue
        cls = class_of(d)
        r = tx.function(d, cls)
        if r and d['id'] not in seen:
            seen.add(d['id'])
            out.append((cls, r))
    # Free functions first, then one impl block per class, in source order.
    blocks = types + [r for c, r in out if c is None]
    classes = []
    for c, _ in out:
        if c is not None and c not in classes:
            classes.append(c)
    for c in classes:
        body = '\n'.join('    ' + l if l else l for r in (r for cc, r in out if cc == c) for l in r.splitlines())
        blocks.append(f'impl {rust_type(c)} {{\n{body}\n}}\n')
    sys.stdout.write('\n'.join(blocks))
    sys.stderr.write(f'{len(out)} functions, {tx.todo} todo spots\n')


if __name__ == '__main__':
    main()

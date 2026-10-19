use super::*;

fn leaves(c: &Csg, out: &mut Vec<(Prim, M4)>) {
    match c {
        Csg::Leaf { prim, at, .. } => out.push((prim.clone(), *at)),
        Csg::Op(_, v) => v.iter().for_each(|c| leaves(c, out)),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn primitives_transforms_and_booleans() {
    let m = read(
        "// a plate with a hole
         w = 40; h = 20;
         difference() {
           cube([w, h, 5]);
           translate([w/2, h/2, -1]) cylinder(h = 7, r = 3, $fn = 32);
         }
         translate([0, 0, 10]) rotate([0, 0, 90]) sphere(d = 4);",
    )
    .unwrap();
    assert_eq!(m.objects.len(), 2, "{m:?}");
    let Csg::Op(Op::Difference, kids) = &m.objects[0] else { panic!("{m:?}") };
    assert_eq!(kids.len(), 2);
    let mut l = Vec::new();
    leaves(&m.objects[0], &mut l);
    assert_eq!(l[0].0, Prim::Cube { size: [40.0, 20.0, 5.0], center: false });
    assert_eq!(l[1].0, Prim::Cylinder { h: 7.0, r1: 3.0, r2: 3.0, center: false });
    assert!(close(l[1].1[0][3], 20.0) && close(l[1].1[1][3], 10.0) && close(l[1].1[2][3], -1.0));
    let mut l = Vec::new();
    leaves(&m.objects[1], &mut l);
    assert_eq!(l[0].0, Prim::Sphere { radius: 2.0 });
    // rotate 90 about Z: X goes to Y.
    assert!(close(l[0].1[1][0], 1.0) && close(l[0].1[0][1], -1.0) && close(l[0].1[2][3], 10.0));
}

#[test]
fn modules_functions_loops_and_comprehensions() {
    let m = read(
        "function sq(x) = x * x;
         module post(i) { translate([i * 10, 0, 0]) cube([2, 2, sq(i) + 1]); }
         module pair() { children(0); translate([0, 50, 0]) children(1); }
         for (i = [0 : 2]) post(i);
         pair() { cube(1); sphere(1); }
         pts = [for (a = [0 : 90 : 270]) [cos(a), sin(a)] * 5];
         linear_extrude(height = 3) polygon(pts);
         n = len([for (i = [1:10]) if (i % 2 == 0) i]);
         cube(n);",
    )
    .unwrap();
    // The for loop is one body (a union of three posts), then the pair, the prism, the cube.
    assert_eq!(m.objects.len(), 4, "{:?}", m.objects);
    let mut l = Vec::new();
    leaves(&m.objects[0], &mut l);
    let heights: Vec<f64> =
        l.iter().map(|(p, _)| if let Prim::Cube { size, .. } = p { size[2] } else { 0.0 }).collect();
    assert_eq!(heights, [1.0, 2.0, 5.0]);
    let mut l = Vec::new();
    leaves(&m.objects[1], &mut l);
    assert!(matches!(l[1].0, Prim::Sphere { .. }) && close(l[1].1[1][3], 50.0), "{l:?}");
    let mut l = Vec::new();
    leaves(&m.objects[2], &mut l);
    let Prim::Prism { shape: Shape2::Polygon(paths), h, .. } = &l[0].0 else { panic!("{l:?}") };
    assert!(close(*h, 3.0) && paths[0].len() == 4 && close(paths[0][1][1], 5.0), "{paths:?}");
    assert_eq!(m.objects[3], Csg::Leaf { prim: Prim::Cube { size: [5.0; 3], center: false }, at: IDENTITY, line: 9 });
}

#[test]
fn rotate_extrude_bakes_2d_transforms() {
    let m = read("rotate_extrude(angle = 180) translate([10, 0]) circle(r = 2);").unwrap();
    let Csg::Leaf { prim: Prim::Revolve { shape, angle }, .. } = &m.objects[0] else { panic!("{m:?}") };
    assert_eq!(*shape, Shape2::Circle { center: [10.0, 0.0], radius: 2.0 });
    assert!(close(*angle, 180.0));
    assert!(read("rotate_extrude() scale([2, 1]) circle(1);").is_err());
}

#[test]
fn unsupported_and_hostile_input_is_reported() {
    let m = read("hull() { cube(1); } import(\"x.stl\"); cube(2);").unwrap();
    assert_eq!(m.objects.len(), 1);
    assert_eq!(m.warnings.len(), 2, "{:?}", m.warnings);
    // Infinite recursion and runaway loops stop with an error.
    assert!(read("module m() m(); m();").is_err());
    assert!(read("function f(x) = f(x + 1); a = f(0); cube(1);").is_err());
    let e = read("for (i = [0 : 1e12]) cube(1);").unwrap_err();
    assert!(e.message.contains("too"), "{e}");
    let deep = format!("a = {}1{};", "(".repeat(1000), ")".repeat(1000));
    assert!(read(&deep).is_err());
    assert!(read("cube(-1);").is_err());
    assert!(read("cube([1, 2, 3]").is_err());
    let e = read("x = 1;\n\ny = ;").unwrap_err();
    assert_eq!(e.line, 3, "{e}");
    // Disabled objects and comments vanish.
    assert_eq!(read("*cube(1); /* sphere(); */ %sphere(2);").unwrap().objects.len(), 1);
}

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::expect_used)]
use crate::inventor_dc::{Dc, Role};
#[test]
fn zz_tree() {
    let Ok(f) = std::env::var("ZZ_TREE") else { return };
    let bytes = std::fs::read(&f).unwrap();
    let t = crate::inventor_features::feature_tree(&bytes, &f, &crate::Limits::default()).unwrap();
    for x in t["features"].as_array().unwrap() {
        if x["type"] == "sketch" && std::env::var("ZZ_SK").is_err() {
            println!("TREE sketch {}", x["name"]);
            continue;
        }
        println!("TREE {x}");
    }
}
#[test]
fn zz_slots() {
    let Ok(list) = std::env::var("ZZ_LIST") else { return };
    let want = std::env::var("ZZ_T").unwrap_or_default();
    for f in std::fs::read_to_string(list).unwrap().lines() {
        let Ok(bytes) = std::fs::read(f) else { continue };
        let Ok(dc) = Dc::open(&bytes, &crate::Limits::default()) else { continue };
        println!("FILE {f}");
        for ft in dc.features() {
            let lab = ft.typ.map(|t| t.label()).unwrap_or("?".into());
            if !lab.contains(&want) {
                continue;
            }
            println!(" FEAT rec {} {:?} {lab} class {:08x}", ft.rec, ft.name, dc.class(ft.rec));
            let Some(s) = dc.slots(ft.rec) else {
                println!("  no slots");
                continue;
            };
            for (j, x) in s.iter().enumerate() {
                let Some(x) = *x else {
                    println!("  {j}: -");
                    continue;
                };
                let r = dc.r(x);
                let t = r.len().checked_sub(2).and_then(|k| crate::inventor_dc::u16_at(r, k));
                println!(
                    "  {j}: rec {x} class {:08x} {:?} len {} param {:?} tail {:?}",
                    dc.class(x),
                    dc.role(x),
                    r.len(),
                    dc.param(x).filter(|_| dc.role(x) == Role::Param),
                    t
                );
            }
        }
    }
}

#[test]
fn zz_all() {
    let Ok(list) = std::env::var("ZZ_ALL") else { return };
    for f in std::fs::read_to_string(list).unwrap().lines() {
        let Ok(bytes) = std::fs::read(f) else { continue };
        let Ok(dc) = Dc::open(&bytes, &crate::Limits::default()) else { continue };
        println!("FILE {f}");
        let mut cls = std::collections::BTreeMap::new();
        for i in 0..dc.n() {
            *cls.entry(dc.class(i)).or_insert(0usize) += 1;
            if !matches!(dc.role(i), Role::PartFeature | Role::PatternFeature) {
                continue;
            }
            let v = crate::inventor_dc::u32_at(dc.r(i), 6).and_then(|v| crate::inventor_dc::reference(v, dc.n()));
            let nm = v.map(|v| (v, dc.class(v), dc.namestr(v)));
            let slots: Vec<String> = dc
                .slots(i)
                .unwrap_or_default()
                .iter()
                .map(|s| s.map(|x| format!("{:08x}", dc.class(x))).unwrap_or("-".into()))
                .collect();
            println!(" REC {i} seq {:?} name {:?} slots {}", dc.seq(i), nm, slots.join(","));
        }
        if std::env::var("ZZ_CLS").is_ok() {
            for (c, n) in cls {
                println!("  CLS {c:08x} {n}");
            }
        }
    }
}

#[test]
fn zz_hist() {
    let Ok(list) = std::env::var("ZZ_HIST") else { return };
    for f in std::fs::read_to_string(list).unwrap().lines() {
        let Ok(bytes) = std::fs::read(f) else { continue };
        let Ok(dc) = Dc::open(&bytes, &crate::Limits::default()) else { continue };
        println!("FILE {f}");
        let res = crate::inventor::extract_sab(&bytes, &crate::Limits::default()).map(|e| e.sab).unwrap_or_default();
        let mut blobs: Vec<(usize, Vec<u8>)> =
            (0..dc.n()).filter(|&i| dc.role(i) == Role::HistoryBlob).map(|i| (i, dc.r(i).to_vec())).collect();
        if std::env::var("ZZ_RES").is_ok() {
            blobs.push((999999, res));
        }
        for (hb, r) in blobs {
            let r = &r[..];
            let Some(k) = r.windows(14).position(|w| w == b"ASM BinaryFile") else { continue };
            let Ok(a) = crate::acis::Acis::parse(&r[k..]) else {
                println!(" HB {hb} unparsed");
                continue;
            };
            println!(" HB {hb} len {} recs {}", r.len(), a.recs.len());
            for (bi, b) in a.recs.iter().enumerate() {
                if b.ty != "body" {
                    continue;
                }
                let mut seen = vec![false; a.recs.len()];
                let mut st = vec![bi];
                seen[bi] = true;
                let mut cnt = std::collections::BTreeMap::new();
                let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
                while let Some(x) = st.pop() {
                    let rx = &a.recs[x];
                    *cnt.entry(rx.ty.clone()).or_insert(0) += 1;
                    if rx.ty == "point" {
                        if let Some(p) = rx.pos.first() {
                            for j in 0..3 {
                                lo[j] = lo[j].min(p[j]);
                                hi[j] = hi[j].max(p[j]);
                            }
                        }
                    }
                    for &p in &rx.refs {
                        let Ok(pu) = usize::try_from(p) else { continue };
                        if pu < a.recs.len()
                            && !seen[pu]
                            && a.recs[pu].ty != "body"
                            && !a.recs[pu].ty.contains("attrib")
                        {
                            seen[pu] = true;
                            st.push(pu);
                        }
                    }
                }
                let c = |n: &str| cnt.get(n).copied().unwrap_or(0);
                println!(
                    "  BODY rec {bi} tag {:?} lumps {} shells {} faces {} wires {} edges {} box {:?} {:?}",
                    b.ints.first(),
                    c("lump"),
                    c("shell"),
                    c("face"),
                    c("wire"),
                    c("edge"),
                    lo,
                    hi
                );
            }
        }
    }
}

#[test]
fn zz_blobs() {
    let Ok(list) = std::env::var("ZZ_BLOBS") else { return };
    let limits = crate::Limits::default();
    for f in std::fs::read_to_string(list).unwrap().lines() {
        let Ok(bytes) = std::fs::read(f) else { continue };
        let mut w = Vec::new();
        let Ok(o) = crate::open(&bytes, &limits, &mut w) else { continue };
        println!("FILE {f}");
        for i in 0..o.streams.len() {
            let name = o.streams[i].name.clone();
            if !name.starts_with('B') || name.len() <= 8 {
                continue;
            }
            let Ok(raw) = o.read(i, &limits) else { continue };
            let Ok(Some(data)) = crate::inventor::b_data(&raw, &limits) else { continue };
            let mut k = 0;
            while let Some(p) = data[k..].windows(14).position(|x| x == b"ASM BinaryFile" || x == b"ACIS BinaryFi") {
                let at = k + p;
                let tag = if at >= 8 { crate::inventor_dc::u32_at(&data, at - 8) } else { None };
                let mut ends = 0;
                println!("  {name} len {} blob at {at} tag {:x?}", data.len(), tag);
                let _ = &mut ends;
                k = at + 14;
            }
        }
    }
}

#[test]
fn zz_rec() {
    let Ok(f) = std::env::var("ZZ_FILE") else { return };
    let bytes = std::fs::read(&f).unwrap();
    let dc = Dc::open(&bytes, &crate::Limits::default()).unwrap();
    let want: Vec<String> = std::env::var("ZZ_CLASSES").unwrap_or_default().split(',').map(str::to_string).collect();
    let mut back: Vec<Vec<usize>> = vec![Vec::new(); dc.n()];
    for i in 0..dc.n() {
        for (_, x) in dc.refs(i) {
            back[x].push(i);
        }
    }
    for i in 0..dc.n() {
        let c = format!("{:08x}", dc.class(i));
        if !want.contains(&c) && !want.contains(&i.to_string()) {
            continue;
        }
        let r = dc.r(i);
        println!("REC {i} {c} len {} seq {:?} name {:?}", r.len(), dc.seq(i), dc.name(i));
        println!(
            "  refs {:?}",
            dc.refs(i).iter().map(|&(o, x)| format!("@{o}->{x}:{:08x}", dc.class(x))).collect::<Vec<_>>()
        );
        println!("  back {:?}", back[i].iter().map(|&x| format!("{x}:{:08x}", dc.class(x))).collect::<Vec<_>>());
        let hex: String = r
            .iter()
            .take(if std::env::var("ZZ_FULL").is_ok() { 100000 } else { 160 })
            .map(|b| format!("{b:02x}"))
            .collect();
        println!("  hex {hex}");
    }
}

#[test]
fn zz_pat() {
    let Ok(list) = std::env::var("ZZ_PAT") else { return };
    for f in std::fs::read_to_string(list).unwrap().lines() {
        let Ok(bytes) = std::fs::read(f) else { continue };
        let Ok(dc) = Dc::open(&bytes, &crate::Limits::default()) else { continue };
        for ft in dc.features() {
            let want = if std::env::var("ZZ_MIR").is_ok() {
                crate::inventor_dc::FeatType::Mirror
            } else if std::env::var("ZZ_CIRC").is_ok() {
                crate::inventor_dc::FeatType::CircPattern
            } else {
                crate::inventor_dc::FeatType::RectPattern
            };
            if ft.typ != Some(want) {
                continue;
            }
            let mut s = String::new();
            for (o, x) in dc.refs(ft.rec) {
                if o <= 14 {
                    continue;
                }
                let r = dc.r(x);
                let c = dc.class(x);
                let d = match c {
                    0xbb1d_d5df => format!(
                        "CNT({:?}={:?})",
                        dc.namestr(x).or_else(|| Some(String::new())),
                        crate::inventor_dc::u32_at(r, r.len() - 4)
                    ),
                    0x9087_4d26 => format!("PAR{:?}", dc.param(x)),
                    0x9087_4d28 => format!("F{}", r.last().unwrap()),
                    0xce52_df40 => format!(
                        "DIR[{:.3},{:.3},{:.3}]",
                        crate::inventor_dc::f64_at(r, r.len() - 24).unwrap(),
                        crate::inventor_dc::f64_at(r, r.len() - 16).unwrap(),
                        crate::inventor_dc::f64_at(r, r.len() - 8).unwrap()
                    ),
                    0x9087_4d91 | 0x2067_3244 => format!("FEAT({:?})", dc.name(x)),
                    _ => format!("{c:08x}"),
                };
                s.push_str(&format!("@{o}:{d} "));
            }
            println!("{} | {:?} | {s}", f.rsplit('/').next().unwrap(), ft.name);
        }
    }
}

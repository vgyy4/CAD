//! FreeCAD `.FCStd` files, tier 2 (ADR 0033): which objects a FreeCAD user sees, and the
//! shape file of each. The ZIP and the XML are untrusted; both readers are capped.
//! The shapes themselves (OCCT BRep text) are parsed by the sandboxed import worker.

use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::{BTreeMap, BTreeSet};

/// Most objects read from one document.
const MAX_OBJECTS: usize = 20_000;
/// Most shapes imported from one file.
pub const MAX_SHAPES: usize = 256;
/// Largest XML file read.
const MAX_XML: usize = 64 << 20;

/// One object of `Document.xml`.
#[derive(Debug, Clone, Default, PartialEq)]
struct Object {
    kind: String,
    label: Option<String>,
    shape_file: Option<String>,
    visible: Option<bool>,
    /// Objects this one links to (Link / LinkList / LinkSub values).
    links: Vec<String>,
}

/// A shape to import: the object's label (untrusted text) and its shape file in the ZIP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub name: String,
    pub label: String,
    pub file: String,
}

/// Types that never are the final part: sketches, datums, groups, spreadsheets, 2D.
fn helper(kind: &str) -> bool {
    kind.starts_with("Sketcher::")
        || kind.starts_with("App::")
        || kind.starts_with("Spreadsheet::")
        || kind.starts_with("Spreadsheet")
        || kind.starts_with("Draft::")
        || kind.starts_with("TechDraw::")
        || matches!(
            kind,
            "PartDesign::Plane" | "PartDesign::Line" | "PartDesign::Point" | "PartDesign::CoordinateSystem"
        )
        || matches!(kind, "PartDesign::ShapeBinder" | "PartDesign::SubShapeBinder" | "Part::Part2DObjectPython")
        || kind.contains("Part2DObject")
        || matches!(
            kind,
            "Part::Plane" | "Part::Circle" | "Part::Ellipse" | "Part::Line" | "Part::Polygon" | "Part::Vertex"
        )
}

fn attr(e: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| {
            let k: &str = a.key.as_ref();
            k == key
        })
        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok())
        .map(|v| v.chars().take(1000).collect())
}

/// Parses `Document.xml`: object types from `<Objects>`, properties from `<ObjectData>`.
fn document(xml: &[u8]) -> Result<BTreeMap<String, Object>, String> {
    if xml.len() > MAX_XML {
        return Err("Document.xml is too large".into());
    }
    let mut r = Reader::from_reader(xml);
    r.config_mut().check_end_names = false;
    let mut buf = Vec::new();
    let mut objects: BTreeMap<String, Object> = BTreeMap::new();
    let mut in_data = false;
    let mut current: Option<String> = None;
    let mut property: Option<String> = None;
    let mut depth = 0u32;
    loop {
        let ev = r.read_event_into(&mut buf).map_err(|e| format!("Document.xml: {e}"))?;
        match &ev {
            Event::Start(e) | Event::Empty(e) => {
                if matches!(ev, Event::Start(_)) {
                    depth += 1;
                    if depth > 64 {
                        return Err("Document.xml is nested too deeply".into());
                    }
                }
                match e.name().as_ref() {
                    "ObjectData" => in_data = true,
                    "Object" => {
                        let Some(name) = attr(e, "name") else { continue };
                        if in_data {
                            current = Some(name);
                        } else {
                            if objects.len() >= MAX_OBJECTS {
                                return Err("the document has too many objects".into());
                            }
                            let kind = attr(e, "type").unwrap_or_default();
                            objects.entry(name).or_default().kind = kind;
                        }
                    }
                    "Property" => property = attr(e, "name"),
                    tag => {
                        let (Some(obj), Some(prop)) = (current.as_ref(), property.as_deref()) else { continue };
                        let Some(o) = objects.get_mut(obj) else { continue };
                        match (prop, tag) {
                            ("Label", "String") => o.label = attr(e, "value"),
                            ("Shape", "Part") => o.shape_file = attr(e, "file").filter(|f| !f.is_empty()),
                            ("Visibility", "Bool") => o.visible = attr(e, "value").map(|v| v == "true"),
                            (_, "Link" | "LinkSub") => {
                                if let Some(v) = attr(e, "value").filter(|v| !v.is_empty())
                                    && o.links.len() < 10_000
                                {
                                    o.links.push(v);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                match e.name().as_ref() {
                    "ObjectData" => in_data = false,
                    "Object" => current = None,
                    "Property" => property = None,
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(objects)
}

/// Visibility per object from `GuiDocument.xml`.
fn gui_visibility(xml: &[u8]) -> Result<BTreeMap<String, bool>, String> {
    if xml.len() > MAX_XML {
        return Err("GuiDocument.xml is too large".into());
    }
    let mut r = Reader::from_reader(xml);
    r.config_mut().check_end_names = false;
    let mut buf = Vec::new();
    let mut out = BTreeMap::new();
    let (mut current, mut property): (Option<String>, Option<String>) = (None, None);
    loop {
        match r.read_event_into(&mut buf).map_err(|e| format!("GuiDocument.xml: {e}"))? {
            Event::Start(e) | Event::Empty(e) => match e.name().as_ref() {
                "ViewProvider" => current = attr(&e, "name"),
                "Property" => property = attr(&e, "name"),
                "Bool" if property.as_deref() == Some("Visibility") => {
                    if let Some(c) = &current
                        && out.len() < MAX_OBJECTS
                    {
                        out.insert(c.clone(), attr(&e, "value").as_deref() == Some("true"));
                    }
                }
                _ => {}
            },
            Event::End(e) => match e.name().as_ref() {
                "ViewProvider" => current = None,
                "Property" => property = None,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// The shapes to import from a document: the visible solid objects, or, when nothing
/// says what is visible, the objects no other object uses.
///
/// # Errors
/// Malformed or oversized XML.
pub fn picks(document_xml: &[u8], gui_xml: Option<&[u8]>) -> Result<Vec<Pick>, String> {
    let mut objects = document(document_xml)?;
    if let Some(g) = gui_xml {
        for (name, v) in gui_visibility(g)? {
            if let Some(o) = objects.get_mut(&name) {
                o.visible = Some(v);
            }
        }
    }
    let used: BTreeSet<String> = objects
        .values()
        .flat_map(|o| o.links.iter())
        .map(|l| l.split(['.', ':']).next().unwrap_or(l).to_string())
        .collect();
    let any_visibility = objects.values().any(|o| o.visible.is_some());
    let mut out: Vec<Pick> = objects
        .iter()
        .filter(|(_, o)| o.shape_file.is_some() && !helper(&o.kind))
        .filter(|(name, o)| if any_visibility { o.visible == Some(true) } else { !used.contains(*name) })
        .map(|(name, o)| Pick {
            name: name.clone(),
            label: o.label.clone().unwrap_or_else(|| name.clone()),
            file: o.shape_file.clone().unwrap_or_default(),
        })
        .collect();
    // A body and its tip feature may both be visible; keep the body.
    let bodies: BTreeSet<&str> =
        objects.iter().filter(|(_, o)| o.kind == "PartDesign::Body").map(|(n, _)| n.as_str()).collect();
    let in_body: BTreeSet<String> = objects
        .iter()
        .filter(|(n, _)| bodies.contains(n.as_str()))
        .flat_map(|(_, o)| o.links.iter().cloned())
        .collect();
    out.retain(|p| bodies.contains(p.name.as_str()) || !in_body.contains(&p.name));
    out.truncate(MAX_SHAPES);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<?xml version='1.0' encoding='utf-8'?>
<Document SchemaVersion="4" ProgramVersion="1.0" FileVersion="1">
  <Objects Count="5">
    <Object type="PartDesign::Body" name="Body" id="1"/>
    <Object type="Sketcher::SketchObject" name="Sketch" id="2"/>
    <Object type="PartDesign::Pad" name="Pad" id="3"/>
    <Object type="Part::Box" name="Box" id="4"/>
    <Object type="Part::Cylinder" name="Cylinder" id="5"/>
  </Objects>
  <ObjectData Count="5">
    <Object name="Body"><Properties Count="3">
      <Property name="Label" type="App::PropertyString"><String value="Bracket &amp; co"/></Property>
      <Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="Sketch"/><Link value="Pad"/></LinkList></Property>
      <Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape.brp"/></Property>
    </Properties></Object>
    <Object name="Sketch"><Properties Count="1">
      <Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape1.brp"/></Property>
    </Properties></Object>
    <Object name="Pad"><Properties Count="2">
      <Property name="Profile" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="0"/></Property>
      <Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape2.brp"/></Property>
    </Properties></Object>
    <Object name="Box"><Properties Count="1">
      <Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape3.brp"/></Property>
    </Properties></Object>
    <Object name="Cylinder"><Properties Count="1">
      <Property name="Shape" type="Part::PropertyPartShape"><Part file="PartShape4.brp"/></Property>
    </Properties></Object>
  </ObjectData>
</Document>"#;

    const GUI: &str = r#"<?xml version='1.0' encoding='utf-8'?>
<Document SchemaVersion="1"><ViewProviderData Count="3">
  <ViewProvider name="Body"><Properties Count="1"><Property name="Visibility" type="App::PropertyBool"><Bool value="true"/></Property></Properties></ViewProvider>
  <ViewProvider name="Pad"><Properties Count="1"><Property name="Visibility" type="App::PropertyBool"><Bool value="true"/></Property></Properties></ViewProvider>
  <ViewProvider name="Box"><Properties Count="1"><Property name="Visibility" type="App::PropertyBool"><Bool value="false"/></Property></Properties></ViewProvider>
  <ViewProvider name="Cylinder"><Properties Count="1"><Property name="Visibility" type="App::PropertyBool"><Bool value="true"/></Property></Properties></ViewProvider>
</ViewProviderData></Document>"#;

    #[test]
    fn visible_solids_are_picked_and_body_features_are_not() {
        let p = picks(DOC.as_bytes(), Some(GUI.as_bytes())).unwrap();
        let names: Vec<&str> = p.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Body", "Cylinder"], "{p:?}");
        assert_eq!(p[0].label, "Bracket & co");
        assert_eq!(p[0].file, "PartShape.brp");
    }

    #[test]
    fn without_visibility_the_unused_objects_are_picked() {
        let p = picks(DOC.as_bytes(), None).unwrap();
        let names: Vec<&str> = p.iter().map(|p| p.name.as_str()).collect();
        // The sketch is a helper; the pad is used by the body.
        assert_eq!(names, ["Body", "Box", "Cylinder"], "{p:?}");
    }

    #[test]
    fn hostile_xml_is_refused_or_ignored() {
        assert!(picks(b"<Document><Objects><Object", None).is_ok_and(|p| p.is_empty()) || picks(b"<a", None).is_err());
        let deep = "<a>".repeat(100);
        assert!(picks(deep.as_bytes(), None).is_err());
        assert!(picks(b"\xff\xfe garbage", None).map(|p| p.is_empty()).unwrap_or(true));
    }
}

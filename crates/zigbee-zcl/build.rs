//! Generates the cluster tables and constants from `data/clusters.toml`.
//!
//! The data is validated here: unique ids, known data types, report
//! defaults that fit, and well-formed field lists. A mistake in the data
//! file fails the build rather than reaching a device.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    cluster: Vec<Cluster>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cluster {
    id: u16,
    name: String,
    #[serde(default)]
    attributes: Vec<Attribute>,
    #[serde(default)]
    commands: Vec<Command>,
    #[serde(default)]
    responses: Vec<Command>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Attribute {
    id: u16,
    name: String,
    #[serde(rename = "type")]
    data_type: String,
    #[serde(default)]
    write: bool,
    #[serde(default)]
    report: Option<Vec<u64>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    id: u8,
    name: String,
    #[serde(default)]
    fields: Vec<String>,
}

/// ZCL data type names and their `DataType` variants.
const TYPES: &[(&str, &str, bool)] = &[
    // (name, variant, analog)
    ("nodata", "NoData", false),
    ("data8", "Data8", false),
    ("data16", "Data16", false),
    ("data24", "Data24", false),
    ("data32", "Data32", false),
    ("data40", "Data40", false),
    ("data48", "Data48", false),
    ("data56", "Data56", false),
    ("data64", "Data64", false),
    ("bool", "Bool", false),
    ("map8", "Map8", false),
    ("map16", "Map16", false),
    ("map24", "Map24", false),
    ("map32", "Map32", false),
    ("map40", "Map40", false),
    ("map48", "Map48", false),
    ("map56", "Map56", false),
    ("map64", "Map64", false),
    ("uint8", "Uint8", true),
    ("uint16", "Uint16", true),
    ("uint24", "Uint24", true),
    ("uint32", "Uint32", true),
    ("uint40", "Uint40", true),
    ("uint48", "Uint48", true),
    ("uint56", "Uint56", true),
    ("uint64", "Uint64", true),
    ("int8", "Int8", true),
    ("int16", "Int16", true),
    ("int24", "Int24", true),
    ("int32", "Int32", true),
    ("int40", "Int40", true),
    ("int48", "Int48", true),
    ("int56", "Int56", true),
    ("int64", "Int64", true),
    ("enum8", "Enum8", false),
    ("enum16", "Enum16", false),
    ("semi", "Semi", true),
    ("single", "Single", true),
    ("double", "Double", true),
    ("octstr", "OctetString", false),
    ("string", "CharString", false),
    ("octstr16", "LongOctetString", false),
    ("string16", "LongCharString", false),
    ("array", "Array", false),
    ("struct", "Struct", false),
    ("set", "Set", false),
    ("bag", "Bag", false),
    ("tod", "TimeOfDay", true),
    ("date", "Date", true),
    ("utc", "UtcTime", true),
    ("clusterid", "ClusterId", false),
    ("attrid", "AttributeId", false),
    ("bacoid", "BacnetOid", false),
    ("ieee", "Ieee", false),
    ("key128", "SecurityKey", false),
];

fn data_type(name: &str, context: &str) -> (&'static str, bool) {
    TYPES
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, v, analog)| (*v, *analog))
        .unwrap_or_else(|| panic!("{context}: unknown data type {name:?}"))
}

fn ident_ok(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
}

fn upper(name: &str) -> String {
    name.to_ascii_uppercase()
}

fn field(spec: &str, context: &str) -> String {
    let (name, ty) = spec
        .split_once(':')
        .unwrap_or_else(|| panic!("{context}: field {spec:?} is not name:type"));
    assert!(ident_ok(name), "{context}: bad field name {name:?}");
    let (ty, optional) = match ty.strip_suffix('?') {
        Some(t) => (t, true),
        None => (ty, false),
    };
    let kind = if ty == "rest" {
        "FieldKind::Rest".to_string()
    } else if let Some(inner) = ty.strip_prefix("list8<").and_then(|t| t.strip_suffix('>')) {
        format!(
            "FieldKind::List8(DataType::{})",
            data_type(inner, context).0
        )
    } else {
        format!("FieldKind::Value(DataType::{})", data_type(ty, context).0)
    };
    format!("FieldDef {{ name: {name:?}, kind: {kind}, optional: {optional} }}")
}

fn commands(list: &[Command], context: &str) -> String {
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut out = String::from("&[");
    let mut sorted: Vec<&Command> = list.iter().collect();
    sorted.sort_by_key(|c| c.id);
    for c in sorted {
        let ctx = format!("{context} command {:?}", c.name);
        assert!(ids.insert(c.id), "{ctx}: duplicate id {:#04x}", c.id);
        assert!(names.insert(c.name.clone()), "{ctx}: duplicate name");
        assert!(ident_ok(&c.name), "{ctx}: bad name");
        let mut seen_optional = false;
        let fields: Vec<String> = c
            .fields
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let optional = f.ends_with('?');
                assert!(
                    !seen_optional || optional,
                    "{ctx}: required field after an optional one"
                );
                seen_optional |= optional;
                assert!(
                    !f.ends_with(":rest") || i + 1 == c.fields.len(),
                    "{ctx}: rest must be last"
                );
                field(f, &ctx)
            })
            .collect();
        let _ = write!(
            out,
            "CommandDef {{ id: {:#04x}, name: {:?}, fields: &[{}] }},",
            c.id,
            c.name,
            fields.join(", ")
        );
    }
    out.push(']');
    out
}

fn main() {
    let path = Path::new("data/clusters.toml");
    println!("cargo:rerun-if-changed={}", path.display());
    let text = std::fs::read_to_string(path).expect("read data/clusters.toml");
    let file: File = toml::from_str(&text).expect("parse data/clusters.toml");

    let mut clusters: Vec<&Cluster> = file.cluster.iter().collect();
    clusters.sort_by_key(|c| c.id);
    let mut cluster_ids = BTreeSet::new();
    let mut cluster_names = BTreeSet::new();

    let mut table = String::from("/// Every cluster this crate knows, sorted by id.\npub static CLUSTERS: &[ClusterDef] = &[\n");
    let mut ids = String::from("/// Cluster ids.\npub mod cluster_id {\n");
    let mut attrs = String::from("/// Attribute ids, one module per cluster.\npub mod attr {\n");
    let mut cmds = String::from("/// Command ids received by each cluster's server, one module per cluster.\npub mod command {\n");
    let mut rsps = String::from("/// Command ids generated by each cluster's server, one module per cluster.\npub mod response {\n");

    for c in &clusters {
        let ctx = format!("cluster {:?}", c.name);
        assert!(
            cluster_ids.insert(c.id),
            "{ctx}: duplicate id {:#06x}",
            c.id
        );
        assert!(
            cluster_names.insert(c.name.clone()),
            "{ctx}: duplicate name"
        );
        assert!(ident_ok(&c.name), "{ctx}: bad name");

        let mut attributes: Vec<&Attribute> = c.attributes.iter().collect();
        attributes.sort_by_key(|a| a.id);
        let mut attr_ids = BTreeSet::new();
        let mut attr_names = BTreeSet::new();
        let mut attr_table = String::from("&[");
        let _ = writeln!(
            attrs,
            "    /// `{}` attributes.\n    pub mod {} {{",
            c.name, c.name
        );
        for a in &attributes {
            let actx = format!("{ctx} attribute {:?}", a.name);
            assert!(attr_ids.insert(a.id), "{actx}: duplicate id {:#06x}", a.id);
            assert!(attr_names.insert(a.name.clone()), "{actx}: duplicate name");
            assert!(ident_ok(&a.name), "{actx}: bad name");
            let (variant, analog) = data_type(&a.data_type, &actx);
            let report = match &a.report {
                None => "None".to_string(),
                Some(r) => {
                    let (min, max, change) = match (analog, r.as_slice()) {
                        (true, [min, max, change]) => (*min, *max, format!("Some({change})")),
                        (false, [min, max]) => (*min, *max, "None".to_string()),
                        (true, _) => panic!("{actx}: analog report needs [min, max, change]"),
                        (false, _) => panic!("{actx}: discrete report needs [min, max]"),
                    };
                    assert!(
                        min <= max && max <= 0xFFFF,
                        "{actx}: report intervals out of range"
                    );
                    format!("Some(ReportDefault {{ min_interval: {min}, max_interval: {max}, change: {change} }})")
                }
            };
            let _ = write!(
                attr_table,
                "AttributeDef {{ id: {:#06x}, name: {:?}, data_type: DataType::{variant}, writable: {}, report: {report} }},",
                a.id, a.name, a.write
            );
            let _ = writeln!(
                attrs,
                "        /// `{}` ({}).\n        pub const {}: u16 = {:#06x};",
                a.name,
                a.data_type,
                upper(&a.name),
                a.id
            );
        }
        attr_table.push(']');
        attrs.push_str("    }\n");

        let command_table = commands(&c.commands, &ctx);
        let response_table = commands(&c.responses, &format!("{ctx} responses"));
        for (module, list) in [(&mut cmds, &c.commands), (&mut rsps, &c.responses)] {
            if list.is_empty() {
                continue;
            }
            let _ = writeln!(
                module,
                "    /// `{}` commands.\n    pub mod {} {{",
                c.name, c.name
            );
            for cmd in list {
                let _ = writeln!(
                    module,
                    "        /// `{}`.\n        pub const {}: u8 = {:#04x};",
                    cmd.name,
                    upper(&cmd.name),
                    cmd.id
                );
            }
            module.push_str("    }\n");
        }

        let _ = writeln!(
            ids,
            "    /// `{}`.\n    pub const {}: u16 = {:#06x};",
            c.name,
            upper(&c.name),
            c.id
        );
        let _ = writeln!(
            table,
            "    ClusterDef {{ id: {:#06x}, name: {:?}, attributes: {attr_table}, commands: {command_table}, responses: {response_table} }},",
            c.id, c.name
        );
    }
    table.push_str("];\n");
    ids.push_str("}\n");
    attrs.push_str("}\n");
    cmds.push_str("}\n");
    rsps.push_str("}\n");

    let out =
        std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("clusters.rs");
    std::fs::write(out, format!("{table}\n{ids}\n{attrs}\n{cmds}\n{rsps}"))
        .expect("write clusters.rs");
}

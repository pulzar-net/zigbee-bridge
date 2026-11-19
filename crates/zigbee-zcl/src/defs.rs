//! Cluster definitions and the table-driven codec for cluster commands.
//!
//! The tables ([`CLUSTERS`]) and constant modules ([`cluster_id`],
//! [`attr`], [`command`], [`response`]) are generated at build time from
//! `data/clusters.toml`.

use zigbee_types::wire::{Reader, Writer};

use crate::frame::Direction;
use crate::types::DataType;
use crate::value::{Value, ValueError};

/// A cluster.
#[derive(Debug)]
pub struct ClusterDef {
    /// Cluster id.
    pub id: u16,
    /// snake_case name.
    pub name: &'static str,
    /// Attributes, sorted by id.
    pub attributes: &'static [AttributeDef],
    /// Commands the server receives, sorted by id.
    pub commands: &'static [CommandDef],
    /// Commands the server generates, sorted by id.
    pub responses: &'static [CommandDef],
}

/// An attribute.
#[derive(Debug)]
pub struct AttributeDef {
    /// Attribute id.
    pub id: u16,
    /// snake_case name.
    pub name: &'static str,
    /// Its type.
    pub data_type: DataType,
    /// Writable.
    pub writable: bool,
    /// The reporting configuration a host writes by default, if the
    /// attribute is worth reporting.
    pub report: Option<ReportDefault>,
}

/// A default reporting configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportDefault {
    /// Seconds between reports at least.
    pub min_interval: u16,
    /// Seconds between reports at most.
    pub max_interval: u16,
    /// Reportable change, for analog types.
    pub change: Option<u64>,
}

/// A cluster-specific command.
#[derive(Debug)]
pub struct CommandDef {
    /// Command id.
    pub id: u8,
    /// snake_case name.
    pub name: &'static str,
    /// Its fields, in order.
    pub fields: &'static [FieldDef],
}

/// A command field.
#[derive(Debug)]
pub struct FieldDef {
    /// snake_case name.
    pub name: &'static str,
    /// How it is encoded.
    pub kind: FieldKind,
    /// May be absent at the end of the frame.
    pub optional: bool,
}

/// How a command field is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// One value of a type, without a type id.
    Value(DataType),
    /// A one-byte count followed by values of a type.
    List8(DataType),
    /// The rest of the payload, as octets.
    Rest,
}

include!(concat!(env!("OUT_DIR"), "/clusters.rs"));

/// The cluster with this id.
pub fn cluster(id: u16) -> Option<&'static ClusterDef> {
    CLUSTERS
        .binary_search_by_key(&id, |c| c.id)
        .ok()
        .and_then(|i| CLUSTERS.get(i))
}

/// The cluster with this name.
pub fn cluster_by_name(name: &str) -> Option<&'static ClusterDef> {
    CLUSTERS.iter().find(|c| c.name == name)
}

impl ClusterDef {
    /// The attribute with this id.
    pub fn attribute(&self, id: u16) -> Option<&'static AttributeDef> {
        self.attributes
            .binary_search_by_key(&id, |a| a.id)
            .ok()
            .and_then(|i| self.attributes.get(i))
    }

    /// The attribute with this name.
    pub fn attribute_by_name(&self, name: &str) -> Option<&'static AttributeDef> {
        self.attributes.iter().find(|a| a.name == name)
    }

    /// The command with this id, sent in `direction`.
    pub fn command(&self, direction: Direction, id: u8) -> Option<&'static CommandDef> {
        let list = match direction {
            Direction::ClientToServer => self.commands,
            Direction::ServerToClient => self.responses,
        };
        list.binary_search_by_key(&id, |c| c.id)
            .ok()
            .and_then(|i| list.get(i))
    }

    /// The command with this name, sent in `direction`.
    pub fn command_by_name(&self, direction: Direction, name: &str) -> Option<&'static CommandDef> {
        let list = match direction {
            Direction::ClientToServer => self.commands,
            Direction::ServerToClient => self.responses,
        };
        list.iter().find(|c| c.name == name)
    }
}

/// A command's fields did not encode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EncodeError {
    /// A required field had no value.
    #[error("missing field {0}")]
    Missing(&'static str),
    /// More values than fields.
    #[error("{0} values for {1} fields")]
    TooMany(usize, usize),
}

impl CommandDef {
    /// Decode a payload into named fields. Absent optional fields are left
    /// out.
    pub fn decode(&self, payload: &[u8]) -> Result<Vec<(&'static str, Value)>, ValueError> {
        let mut r = Reader::new(payload);
        let mut out = Vec::with_capacity(self.fields.len());
        for field in self.fields {
            if field.optional && r.is_empty() {
                break;
            }
            let value = match field.kind {
                FieldKind::Value(t) => Value::decode(t, &mut r)?,
                FieldKind::List8(t) => {
                    let count = usize::from(r.u8()?);
                    let mut items = Vec::with_capacity(count.min(r.remaining()));
                    for _ in 0..count {
                        items.push(Value::decode(t, &mut r)?);
                    }
                    Value::List(DataType::Array, t, Some(items))
                }
                FieldKind::Rest => Value::Octets(DataType::OctetString, Some(r.rest().to_vec())),
            };
            out.push((field.name, value));
        }
        Ok(out)
    }

    /// Encode values given in field order. Trailing optional fields may be
    /// left out.
    pub fn encode(&self, values: &[Value]) -> Result<Vec<u8>, EncodeError> {
        if values.len() > self.fields.len() {
            return Err(EncodeError::TooMany(values.len(), self.fields.len()));
        }
        let mut w = Vec::new();
        for (i, field) in self.fields.iter().enumerate() {
            let Some(value) = values.get(i) else {
                if field.optional {
                    break;
                }
                return Err(EncodeError::Missing(field.name));
            };
            match (field.kind, value) {
                (FieldKind::List8(_), Value::List(_, _, items)) => {
                    let items = items.as_deref().unwrap_or(&[]);
                    w.put_u8(items.len().min(255) as u8);
                    items.iter().take(255).for_each(|v| v.encode(&mut w));
                }
                (FieldKind::Rest, Value::Octets(_, bytes)) => {
                    w.put_bytes(bytes.as_deref().unwrap_or(&[]))
                }
                _ => value.encode(&mut w),
            }
        }
        Ok(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted_and_findable() {
        assert!(CLUSTERS.windows(2).all(|w| w[0].id < w[1].id));
        for c in CLUSTERS {
            assert!(
                c.attributes.windows(2).all(|w| w[0].id < w[1].id),
                "{}",
                c.name
            );
            assert_eq!(cluster(c.id).map(|x| x.name), Some(c.name));
        }
        let t = cluster(cluster_id::TEMPERATURE_MEASUREMENT).unwrap();
        let mv = t
            .attribute(attr::temperature_measurement::MEASURED_VALUE)
            .unwrap();
        assert_eq!(mv.data_type, DataType::Int16);
        assert_eq!(
            mv.report,
            Some(ReportDefault {
                min_interval: 10,
                max_interval: 3600,
                change: Some(10)
            })
        );
        assert_eq!(cluster_by_name("on_off").unwrap().id, 0x0006);
    }

    #[test]
    fn commands_encode_and_decode_from_the_table() {
        let level = cluster(cluster_id::LEVEL_CONTROL).unwrap();
        let mtl = level
            .command(
                Direction::ClientToServer,
                command::level_control::MOVE_TO_LEVEL,
            )
            .unwrap();
        let bytes = mtl.encode(&[Value::u8(128), Value::u16(10)]).unwrap();
        assert_eq!(bytes, [128, 10, 0]);
        let fields = mtl.decode(&bytes).unwrap();
        assert_eq!(
            fields,
            vec![
                ("level", Value::u8(128)),
                ("transition_time", Value::u16(10))
            ]
        );
        assert_eq!(
            mtl.encode(&[Value::u8(1)]),
            Err(EncodeError::Missing("transition_time"))
        );

        let ias = cluster(cluster_id::IAS_ZONE).unwrap();
        let notify = ias
            .command(
                Direction::ServerToClient,
                response::ias_zone::ZONE_STATUS_CHANGE_NOTIFICATION,
            )
            .unwrap();
        let fields = notify.decode(&[0x01, 0x00, 0x00]).unwrap();
        assert_eq!(
            fields,
            vec![
                ("zone_status", Value::map16(1)),
                ("extended_status", Value::map8(0))
            ]
        );

        let groups = cluster(cluster_id::GROUPS).unwrap();
        let get = groups
            .command(
                Direction::ClientToServer,
                command::groups::GET_GROUP_MEMBERSHIP,
            )
            .unwrap();
        let list = Value::List(
            DataType::Array,
            DataType::Uint16,
            Some(vec![Value::u16(1), Value::u16(2)]),
        );
        let bytes = get.encode(std::slice::from_ref(&list)).unwrap();
        assert_eq!(bytes, [2, 1, 0, 2, 0]);
        assert_eq!(get.decode(&bytes).unwrap(), vec![("group_list", list)]);
    }
}

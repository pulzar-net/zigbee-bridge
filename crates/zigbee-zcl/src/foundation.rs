//! Global (foundation) commands, common to every cluster (ZCL chapter 2.5).

use zigbee_primitives::wire::{Reader, Writer};

use crate::types::DataType;
use crate::value::{Value, ValueError};

/// Global command ids.
pub mod id {
    #![allow(missing_docs)]
    pub const READ_ATTRIBUTES: u8 = 0x00;
    pub const READ_ATTRIBUTES_RESPONSE: u8 = 0x01;
    pub const WRITE_ATTRIBUTES: u8 = 0x02;
    pub const WRITE_ATTRIBUTES_UNDIVIDED: u8 = 0x03;
    pub const WRITE_ATTRIBUTES_RESPONSE: u8 = 0x04;
    pub const WRITE_ATTRIBUTES_NO_RESPONSE: u8 = 0x05;
    pub const CONFIGURE_REPORTING: u8 = 0x06;
    pub const CONFIGURE_REPORTING_RESPONSE: u8 = 0x07;
    pub const READ_REPORTING_CONFIGURATION: u8 = 0x08;
    pub const READ_REPORTING_CONFIGURATION_RESPONSE: u8 = 0x09;
    pub const REPORT_ATTRIBUTES: u8 = 0x0A;
    pub const DEFAULT_RESPONSE: u8 = 0x0B;
    pub const DISCOVER_ATTRIBUTES: u8 = 0x0C;
    pub const DISCOVER_ATTRIBUTES_RESPONSE: u8 = 0x0D;
}

/// One record of a read attributes response.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadRecord {
    /// Attribute id.
    pub attribute: u16,
    /// [`crate::status`] code.
    pub status: u8,
    /// The value, when `status` is success.
    pub value: Option<Value>,
}

/// One attribute's reporting configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportingConfig {
    /// Attribute id.
    pub attribute: u16,
    /// The attribute's type.
    pub data_type: DataType,
    /// Seconds between reports at least.
    pub min_interval: u16,
    /// Seconds between reports at most (0xFFFF: no periodic reports).
    pub max_interval: u16,
    /// Change that triggers a report, for analog types only.
    pub reportable_change: Option<Value>,
}

/// One record of a configure reporting response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportingStatus {
    /// [`crate::status`] code.
    pub status: u8,
    /// The attribute, absent in the single all-success record.
    pub attribute: Option<u16>,
}

/// A global command.
#[derive(Debug, Clone, PartialEq)]
pub enum Global {
    /// Read attributes.
    ReadAttributes(Vec<u16>),
    /// The answer.
    ReadAttributesResponse(Vec<ReadRecord>),
    /// Write attributes.
    WriteAttributes(Vec<(u16, Value)>),
    /// Write attributes, all or none.
    WriteAttributesUndivided(Vec<(u16, Value)>),
    /// The answer: one success record, or a record per failure.
    WriteAttributesResponse(Vec<(u8, Option<u16>)>),
    /// Write attributes without an answer.
    WriteAttributesNoResponse(Vec<(u16, Value)>),
    /// Configure reporting (attributes the receiver reports).
    ConfigureReporting(Vec<ReportingConfig>),
    /// The answer.
    ConfigureReportingResponse(Vec<ReportingStatus>),
    /// An attribute report.
    ReportAttributes(Vec<(u16, Value)>),
    /// The default response.
    DefaultResponse {
        /// The command answered.
        command: u8,
        /// [`crate::status`] code.
        status: u8,
    },
    /// Discover attributes.
    DiscoverAttributes {
        /// First attribute id.
        start: u16,
        /// Most attributes to return.
        max: u8,
    },
    /// The answer.
    DiscoverAttributesResponse {
        /// No more attributes.
        complete: bool,
        /// Attributes and their types.
        attributes: Vec<(u16, DataType)>,
    },
    /// A global command this crate does not decode.
    Other {
        /// Command id.
        command: u8,
        /// Raw payload.
        payload: Vec<u8>,
    },
}

/// A global command's payload did not decode.
pub type DecodeError = ValueError;

fn read_type(r: &mut Reader<'_>) -> Result<DataType, ValueError> {
    let id = r.u8()?;
    DataType::from_u8(id).ok_or(ValueError::UnknownType(id))
}

fn read_values(r: &mut Reader<'_>) -> Result<Vec<(u16, Value)>, ValueError> {
    let mut out = Vec::new();
    while !r.is_empty() {
        let attribute = r.u16()?;
        out.push((attribute, Value::decode_typed(r)?));
    }
    Ok(out)
}

impl Global {
    /// The command id.
    pub fn command_id(&self) -> u8 {
        match self {
            Global::ReadAttributes(_) => id::READ_ATTRIBUTES,
            Global::ReadAttributesResponse(_) => id::READ_ATTRIBUTES_RESPONSE,
            Global::WriteAttributes(_) => id::WRITE_ATTRIBUTES,
            Global::WriteAttributesUndivided(_) => id::WRITE_ATTRIBUTES_UNDIVIDED,
            Global::WriteAttributesResponse(_) => id::WRITE_ATTRIBUTES_RESPONSE,
            Global::WriteAttributesNoResponse(_) => id::WRITE_ATTRIBUTES_NO_RESPONSE,
            Global::ConfigureReporting(_) => id::CONFIGURE_REPORTING,
            Global::ConfigureReportingResponse(_) => id::CONFIGURE_REPORTING_RESPONSE,
            Global::ReportAttributes(_) => id::REPORT_ATTRIBUTES,
            Global::DefaultResponse { .. } => id::DEFAULT_RESPONSE,
            Global::DiscoverAttributes { .. } => id::DISCOVER_ATTRIBUTES,
            Global::DiscoverAttributesResponse { .. } => id::DISCOVER_ATTRIBUTES_RESPONSE,
            Global::Other { command, .. } => *command,
        }
    }

    /// Decode a global command's payload.
    pub fn decode(command: u8, payload: &[u8]) -> Result<Global, DecodeError> {
        let mut r = Reader::new(payload);
        Ok(match command {
            id::READ_ATTRIBUTES => {
                let count = r.remaining() / 2;
                Global::ReadAttributes(r.u16_list(count)?)
            }
            id::READ_ATTRIBUTES_RESPONSE => {
                let mut records = Vec::new();
                while !r.is_empty() {
                    let attribute = r.u16()?;
                    let status = r.u8()?;
                    let value = if status == crate::status::SUCCESS {
                        Some(Value::decode_typed(&mut r)?)
                    } else {
                        None
                    };
                    records.push(ReadRecord {
                        attribute,
                        status,
                        value,
                    });
                }
                Global::ReadAttributesResponse(records)
            }
            id::WRITE_ATTRIBUTES => Global::WriteAttributes(read_values(&mut r)?),
            id::WRITE_ATTRIBUTES_UNDIVIDED => {
                Global::WriteAttributesUndivided(read_values(&mut r)?)
            }
            id::WRITE_ATTRIBUTES_NO_RESPONSE => {
                Global::WriteAttributesNoResponse(read_values(&mut r)?)
            }
            id::REPORT_ATTRIBUTES => Global::ReportAttributes(read_values(&mut r)?),
            id::WRITE_ATTRIBUTES_RESPONSE => {
                let mut records = Vec::new();
                while !r.is_empty() {
                    let status = r.u8()?;
                    let attribute = if r.remaining() >= 2 {
                        Some(r.u16()?)
                    } else {
                        None
                    };
                    records.push((status, attribute));
                }
                Global::WriteAttributesResponse(records)
            }
            id::CONFIGURE_REPORTING => {
                let mut configs = Vec::new();
                while !r.is_empty() {
                    let direction = r.u8()?;
                    let attribute = r.u16()?;
                    if direction != 0 {
                        // "Received" configurations carry only a timeout;
                        // keep the record so the attribute is not lost.
                        let timeout = r.u16()?;
                        configs.push(ReportingConfig {
                            attribute,
                            data_type: DataType::NoData,
                            min_interval: 0,
                            max_interval: timeout,
                            reportable_change: None,
                        });
                        continue;
                    }
                    let data_type = read_type(&mut r)?;
                    let min_interval = r.u16()?;
                    let max_interval = r.u16()?;
                    let reportable_change = if data_type.is_analog() {
                        Some(Value::decode(data_type, &mut r)?)
                    } else {
                        None
                    };
                    configs.push(ReportingConfig {
                        attribute,
                        data_type,
                        min_interval,
                        max_interval,
                        reportable_change,
                    });
                }
                Global::ConfigureReporting(configs)
            }
            id::CONFIGURE_REPORTING_RESPONSE => {
                let mut records = Vec::new();
                while !r.is_empty() {
                    let status = r.u8()?;
                    if r.is_empty() {
                        records.push(ReportingStatus {
                            status,
                            attribute: None,
                        });
                        break;
                    }
                    let _direction = r.u8()?;
                    records.push(ReportingStatus {
                        status,
                        attribute: Some(r.u16()?),
                    });
                }
                Global::ConfigureReportingResponse(records)
            }
            id::DEFAULT_RESPONSE => Global::DefaultResponse {
                command: r.u8()?,
                status: r.u8()?,
            },
            id::DISCOVER_ATTRIBUTES => Global::DiscoverAttributes {
                start: r.u16()?,
                max: r.u8()?,
            },
            id::DISCOVER_ATTRIBUTES_RESPONSE => {
                let complete = r.bool()?;
                let mut attributes = Vec::new();
                while !r.is_empty() {
                    let attribute = r.u16()?;
                    attributes.push((attribute, read_type(&mut r)?));
                }
                Global::DiscoverAttributesResponse {
                    complete,
                    attributes,
                }
            }
            other => Global::Other {
                command: other,
                payload: payload.to_vec(),
            },
        })
    }

    /// Encode the payload.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Vec::with_capacity(16);
        match self {
            Global::ReadAttributes(ids) => ids.iter().for_each(|a| w.put_u16(*a)),
            Global::ReadAttributesResponse(records) => {
                for rec in records {
                    w.put_u16(rec.attribute);
                    w.put_u8(rec.status);
                    if let (crate::status::SUCCESS, Some(v)) = (rec.status, &rec.value) {
                        v.encode_typed(&mut w);
                    }
                }
            }
            Global::WriteAttributes(values)
            | Global::WriteAttributesUndivided(values)
            | Global::WriteAttributesNoResponse(values)
            | Global::ReportAttributes(values) => {
                for (attribute, value) in values {
                    w.put_u16(*attribute);
                    value.encode_typed(&mut w);
                }
            }
            Global::WriteAttributesResponse(records) => {
                for (status, attribute) in records {
                    w.put_u8(*status);
                    if let Some(a) = attribute {
                        w.put_u16(*a);
                    }
                }
            }
            Global::ConfigureReporting(configs) => {
                for c in configs {
                    w.put_u8(0);
                    w.put_u16(c.attribute);
                    w.put_u8(c.data_type.id());
                    w.put_u16(c.min_interval);
                    w.put_u16(c.max_interval);
                    if c.data_type.is_analog() {
                        match &c.reportable_change {
                            Some(change) => change.encode(&mut w),
                            None => Value::Uint(c.data_type, 0).encode(&mut w),
                        }
                    }
                }
            }
            Global::ConfigureReportingResponse(records) => {
                for rec in records {
                    w.put_u8(rec.status);
                    if let Some(a) = rec.attribute {
                        w.put_u8(0);
                        w.put_u16(a);
                    }
                }
            }
            Global::DefaultResponse { command, status } => {
                w.put_u8(*command);
                w.put_u8(*status);
            }
            Global::DiscoverAttributes { start, max } => {
                w.put_u16(*start);
                w.put_u8(*max);
            }
            Global::DiscoverAttributesResponse {
                complete,
                attributes,
            } => {
                w.put_u8(u8::from(*complete));
                for (a, t) in attributes {
                    w.put_u16(*a);
                    w.put_u8(t.id());
                }
            }
            Global::Other { payload, .. } => w.put_bytes(payload),
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_commands_round_trip() {
        let commands = vec![
            Global::ReadAttributes(vec![0x0004, 0x0005]),
            Global::ReadAttributesResponse(vec![
                ReadRecord {
                    attribute: 4,
                    status: 0,
                    value: Some(Value::string("LUMI")),
                },
                ReadRecord {
                    attribute: 0x4000,
                    status: crate::status::UNSUPPORTED_ATTRIBUTE,
                    value: None,
                },
            ]),
            Global::WriteAttributes(vec![(0x0010, Value::Ieee(zigbee_primitives::Ieee(5)))]),
            Global::WriteAttributesResponse(vec![(0, None)]),
            Global::ConfigureReporting(vec![
                ReportingConfig {
                    attribute: 0,
                    data_type: DataType::Int16,
                    min_interval: 10,
                    max_interval: 3600,
                    reportable_change: Some(Value::i16(10)),
                },
                ReportingConfig {
                    attribute: 0,
                    data_type: DataType::Bool,
                    min_interval: 0,
                    max_interval: 3600,
                    reportable_change: None,
                },
            ]),
            Global::ConfigureReportingResponse(vec![ReportingStatus {
                status: 0,
                attribute: None,
            }]),
            Global::ConfigureReportingResponse(vec![
                ReportingStatus {
                    status: 0x8C,
                    attribute: Some(1),
                },
                ReportingStatus {
                    status: 0x86,
                    attribute: Some(2),
                },
            ]),
            Global::ReportAttributes(vec![(0, Value::i16(2150))]),
            Global::DefaultResponse {
                command: 0x02,
                status: 0,
            },
            Global::DiscoverAttributes { start: 0, max: 16 },
            Global::DiscoverAttributesResponse {
                complete: true,
                attributes: vec![(0, DataType::Uint8)],
            },
        ];
        for c in commands {
            assert_eq!(
                Global::decode(c.command_id(), &c.encode()).unwrap(),
                c,
                "{c:?}"
            );
        }
    }

    #[test]
    fn decodes_a_real_temperature_report() {
        // measured_value (0x0000), int16, 21.50 degrees
        let g = Global::decode(id::REPORT_ATTRIBUTES, &[0x00, 0x00, 0x29, 0x66, 0x08]).unwrap();
        assert_eq!(g, Global::ReportAttributes(vec![(0, Value::i16(2150))]));
    }
}

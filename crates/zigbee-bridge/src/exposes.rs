//! What a device's standard clusters mean as JSON fields, and the Home
//! Assistant discovery that describes them.
//!
//! Everything here is from the ZCL specification, so it holds for every
//! device that implements the cluster: no per-device code. Device quirks
//! belong in device definitions, not here.

use serde_json::{json, Value as Json};
use zigbee_host::Device;
use zigbee_primitives::Ieee;
use zigbee_zcl::{attr, cluster_id, Value};

use crate::api::{
    DiscoveryAvailability, DiscoveryConfig, DiscoveryDevice, DiscoveryOrigin, DiscoveryPayload,
};

/// How a raw attribute becomes a JSON value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Convert {
    /// Multiply, rounding to `decimals`.
    Scale(f64, u32),
    /// Illuminance: lux = 10 ^ ((value - 1) / 10000).
    Lux,
    /// "ON" / "OFF".
    OnOff,
    /// One bit of a bitmap as a boolean.
    Bit(u8),
    /// The value as is.
    Raw,
}

/// The Home Assistant entity type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    /// A sensor.
    Sensor,
    /// A binary sensor.
    BinarySensor,
    /// A switch (on/off without level).
    Switch,
}

impl Component {
    fn name(self) -> &'static str {
        match self {
            Component::Sensor => "sensor",
            Component::BinarySensor => "binary_sensor",
            Component::Switch => "switch",
        }
    }
}

/// One attribute as a field.
#[derive(Debug)]
pub struct Expose {
    /// Cluster.
    pub cluster: u16,
    /// Attribute.
    pub attribute: u16,
    /// JSON field name.
    pub field: &'static str,
    /// Conversion.
    pub convert: Convert,
    /// Unit.
    pub unit: Option<&'static str>,
    /// Home Assistant device class.
    pub device_class: Option<&'static str>,
    /// Home Assistant state class.
    pub state_class: Option<&'static str>,
    /// Home Assistant entity type.
    pub component: Component,
    /// A diagnostic entity.
    pub diagnostic: bool,
}

macro_rules! expose {
    ($cluster:expr, $attr:expr, $field:literal, $convert:expr, $unit:expr, $class:expr, $state:expr, $component:expr, $diag:expr) => {
        Expose {
            cluster: $cluster,
            attribute: $attr,
            field: $field,
            convert: $convert,
            unit: $unit,
            device_class: $class,
            state_class: $state,
            component: $component,
            diagnostic: $diag,
        }
    };
}

const M: Option<&str> = Some("measurement");

/// Every standard attribute the bridge turns into a field.
pub static EXPOSES: &[Expose] = &[
    expose!(
        cluster_id::TEMPERATURE_MEASUREMENT,
        attr::temperature_measurement::MEASURED_VALUE,
        "temperature",
        Convert::Scale(0.01, 2),
        Some("°C"),
        Some("temperature"),
        M,
        Component::Sensor,
        false
    ),
    expose!(
        cluster_id::RELATIVE_HUMIDITY_MEASUREMENT,
        attr::relative_humidity_measurement::MEASURED_VALUE,
        "humidity",
        Convert::Scale(0.01, 2),
        Some("%"),
        Some("humidity"),
        M,
        Component::Sensor,
        false
    ),
    expose!(
        cluster_id::PRESSURE_MEASUREMENT,
        attr::pressure_measurement::MEASURED_VALUE,
        "pressure",
        Convert::Scale(1.0, 1),
        Some("hPa"),
        Some("pressure"),
        M,
        Component::Sensor,
        false
    ),
    expose!(
        cluster_id::ILLUMINANCE_MEASUREMENT,
        attr::illuminance_measurement::MEASURED_VALUE,
        "illuminance",
        Convert::Lux,
        Some("lx"),
        Some("illuminance"),
        M,
        Component::Sensor,
        false
    ),
    expose!(
        cluster_id::FLOW_MEASUREMENT,
        attr::flow_measurement::MEASURED_VALUE,
        "flow",
        Convert::Scale(0.1, 1),
        Some("m³/h"),
        None,
        M,
        Component::Sensor,
        false
    ),
    expose!(
        cluster_id::SOIL_MOISTURE_MEASUREMENT,
        attr::soil_moisture_measurement::MEASURED_VALUE,
        "soil_moisture",
        Convert::Scale(0.01, 2),
        Some("%"),
        Some("moisture"),
        M,
        Component::Sensor,
        false
    ),
    expose!(
        cluster_id::CARBON_DIOXIDE_MEASUREMENT,
        attr::carbon_dioxide_measurement::MEASURED_VALUE,
        "co2",
        Convert::Scale(1e6, 0),
        Some("ppm"),
        Some("carbon_dioxide"),
        M,
        Component::Sensor,
        false
    ),
    expose!(
        cluster_id::PM25_MEASUREMENT,
        attr::pm25_measurement::MEASURED_VALUE,
        "pm25",
        Convert::Scale(1.0, 1),
        Some("µg/m³"),
        Some("pm25"),
        M,
        Component::Sensor,
        false
    ),
    expose!(
        cluster_id::OCCUPANCY_SENSING,
        attr::occupancy_sensing::OCCUPANCY,
        "occupancy",
        Convert::Bit(0),
        None,
        Some("occupancy"),
        None,
        Component::BinarySensor,
        false
    ),
    expose!(
        cluster_id::ON_OFF,
        attr::on_off::ON_OFF,
        "state",
        Convert::OnOff,
        None,
        None,
        None,
        Component::Switch,
        false
    ),
    expose!(
        cluster_id::POWER_CONFIGURATION,
        attr::power_configuration::BATTERY_PERCENTAGE_REMAINING,
        "battery",
        Convert::Scale(0.5, 0),
        Some("%"),
        Some("battery"),
        M,
        Component::Sensor,
        true
    ),
    expose!(
        cluster_id::POWER_CONFIGURATION,
        attr::power_configuration::BATTERY_VOLTAGE,
        "battery_voltage",
        Convert::Scale(100.0, 0),
        Some("mV"),
        Some("voltage"),
        M,
        Component::Sensor,
        true
    ),
    expose!(
        cluster_id::ELECTRICAL_MEASUREMENT,
        attr::electrical_measurement::ACTIVE_POWER,
        "power",
        Convert::Scale(1.0, 1),
        Some("W"),
        Some("power"),
        M,
        Component::Sensor,
        false
    ),
    expose!(
        cluster_id::ELECTRICAL_MEASUREMENT,
        attr::electrical_measurement::RMS_VOLTAGE,
        "voltage",
        Convert::Scale(1.0, 1),
        Some("V"),
        Some("voltage"),
        M,
        Component::Sensor,
        false
    ),
    expose!(
        cluster_id::ELECTRICAL_MEASUREMENT,
        attr::electrical_measurement::RMS_CURRENT,
        "current",
        Convert::Scale(0.001, 3),
        Some("A"),
        Some("current"),
        M,
        Component::Sensor,
        false
    ),
    expose!(
        cluster_id::METERING,
        attr::metering::CURRENT_SUMMATION_DELIVERED,
        "energy",
        Convert::Scale(0.001, 3),
        Some("kWh"),
        Some("energy"),
        Some("total_increasing"),
        Component::Sensor,
        false
    ),
    expose!(
        cluster_id::THERMOSTAT,
        attr::thermostat::LOCAL_TEMPERATURE,
        "local_temperature",
        Convert::Scale(0.01, 2),
        Some("°C"),
        Some("temperature"),
        M,
        Component::Sensor,
        false
    ),
    expose!(
        cluster_id::DEVICE_TEMPERATURE_CONFIGURATION,
        attr::device_temperature_configuration::CURRENT_TEMPERATURE,
        "device_temperature",
        Convert::Raw,
        Some("°C"),
        Some("temperature"),
        M,
        Component::Sensor,
        true
    ),
];

/// The expose for an attribute.
pub fn find(cluster: u16, attribute: u16) -> Option<&'static Expose> {
    EXPOSES
        .iter()
        .find(|e| e.cluster == cluster && e.attribute == attribute)
}

fn round(v: f64, decimals: u32) -> f64 {
    let f = 10f64.powi(decimals as i32);
    (v * f).round() / f
}

/// Convert a value; `None` for invalid ("no reading") values.
pub fn convert(expose: &Expose, value: &Value) -> Option<Json> {
    match expose.convert {
        Convert::OnOff => value
            .as_bool()
            .map(|on| json!(if on { "ON" } else { "OFF" })),
        Convert::Bit(bit) => value.as_u64().map(|v| json!(v & (1 << bit) != 0)),
        Convert::Lux => value.as_f64().map(|v| {
            json!(round(
                if v <= 0.0 {
                    0.0
                } else {
                    10f64.powf((v - 1.0) / 10000.0)
                },
                0
            ))
        }),
        Convert::Scale(factor, decimals) => {
            value.as_f64().map(|v| json!(round(v * factor, decimals)))
        }
        Convert::Raw => serde_json::to_value(value).ok().filter(|j| !j.is_null()),
    }
}

/// The field name for an expose on an endpoint: plain on the first
/// endpoint that has the cluster, `<field>_<endpoint>` on later ones.
pub fn field_name(device: &Device, endpoint: u8, expose: &Expose) -> String {
    match device
        .endpoints
        .iter()
        .find(|e| e.input_clusters.contains(&expose.cluster))
    {
        Some(first) if first.endpoint != endpoint => format!("{}_{endpoint}", expose.field),
        _ => expose.field.to_string(),
    }
}

/// IAS zone fields for a zone status, by zone type.
pub fn ias_fields(zone_type: Option<u16>, status: u16) -> Vec<(&'static str, Json)> {
    let alarm = status & 0x0001 != 0;
    let main = match zone_type {
        Some(0x0015) => ("contact", json!(!alarm)),
        Some(0x000D) => ("occupancy", json!(alarm)),
        Some(0x002A) => ("water_leak", json!(alarm)),
        Some(0x0028) => ("smoke", json!(alarm)),
        Some(0x002B) => ("carbon_monoxide", json!(alarm)),
        Some(0x002D) => ("vibration", json!(alarm)),
        _ => ("alarm", json!(alarm)),
    };
    vec![
        main,
        ("tamper", json!(status & 0x0004 != 0)),
        ("battery_low", json!(status & 0x0008 != 0)),
    ]
}

fn ias_class(field: &str) -> Option<&'static str> {
    Some(match field {
        "contact" => "door",
        "occupancy" => "motion",
        "water_leak" => "moisture",
        "smoke" => "smoke",
        "carbon_monoxide" => "carbon_monoxide",
        "vibration" => "vibration",
        "tamper" => "tamper",
        "battery_low" => "battery",
        _ => "safety",
    })
}

/// Everything a discovery message needs to know about the bridge.
pub struct DiscoveryContext<'a> {
    /// Home Assistant's discovery prefix.
    pub prefix: &'a str,
    /// The bridge's base topic.
    pub base: &'a str,
    /// The bridge's version.
    pub version: &'a str,
}

fn title(field: &str) -> String {
    let mut out = String::new();
    for (i, word) in field.split('_').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(c) = chars.next() {
            out.extend(c.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out.replace("Co2", "CO2").replace("Pm25", "PM2.5")
}

fn id(ieee: Ieee) -> String {
    ieee.to_prefixed_hex()
}

/// Discovery messages (topic, payload) for a device's standard clusters.
pub fn discovery(
    ctx: &DiscoveryContext<'_>,
    device: &Device,
    friendly: &str,
) -> Vec<(String, DiscoveryConfig)> {
    let ieee = id(device.ieee);
    let state_topic = format!("{}/{friendly}", ctx.base);
    let device_info = DiscoveryDevice {
        identifiers: vec![format!("zigbee-bridge_{ieee}")],
        name: friendly.to_string(),
        manufacturer: device.manufacturer.clone(),
        model: device.model.clone(),
        sw_version: device.sw_build_id.clone(),
    };
    let base_payload = |field: &str, name: String| DiscoveryConfig {
        name,
        unique_id: format!("{ieee}_{field}_zigbee-bridge"),
        object_id: format!("{}_{field}", friendly.replace(['/', ' '], "_")),
        state_topic: state_topic.clone(),
        value_template: Some(format!("{{{{ value_json.{field} }}}}")),
        availability: vec![DiscoveryAvailability {
            topic: format!("{}/bridge/state", ctx.base),
            value_template: "{{ value_json.state }}".into(),
        }],
        device: device_info.clone(),
        origin: DiscoveryOrigin {
            name: "zigbee-bridge".into(),
            sw_version: ctx.version.into(),
        },
        ..DiscoveryConfig::default()
    };
    let config_topic = |component: &str, field: &str| {
        format!(
            "{}/{component}/{}/{field}/config",
            ctx.prefix,
            ieee.trim_start_matches("0x")
        )
    };
    let text = |s: &str| Some(s.to_string());
    let mut out = Vec::new();
    for ep in &device.endpoints {
        let has = |c: u16| ep.input_clusters.contains(&c);
        let light = has(cluster_id::ON_OFF) && has(cluster_id::LEVEL_CONTROL);
        if light {
            let field = match device
                .endpoints
                .iter()
                .find(|e| e.input_clusters.contains(&cluster_id::ON_OFF))
            {
                Some(first) if first.endpoint != ep.endpoint => format!("light_{}", ep.endpoint),
                _ => "light".to_string(),
            };
            let p = DiscoveryConfig {
                value_template: None,
                schema: text("json"),
                command_topic: Some(format!("{state_topic}/set")),
                brightness: Some(true),
                brightness_scale: Some(254),
                supported_color_modes: has(cluster_id::COLOR_CONTROL)
                    .then(|| vec!["color_temp".to_string()]),
                ..base_payload(&field, "Light".into())
            };
            out.push((config_topic("light", &field), p));
        }
        for e in EXPOSES.iter().filter(|e| has(e.cluster)) {
            if light && e.cluster == cluster_id::ON_OFF {
                continue;
            }
            // A battery cluster on a mains device has no battery.
            if e.cluster == cluster_id::POWER_CONFIGURATION
                && device
                    .power_source
                    .is_some_and(|p| p & 0x7F == 0x01 || p & 0x7F == 0x04)
            {
                continue;
            }
            let field = field_name(device, ep.endpoint, e);
            let mut p = DiscoveryConfig {
                unit_of_measurement: e.unit.map(str::to_string),
                device_class: e.device_class.map(str::to_string),
                state_class: e.state_class.map(str::to_string),
                entity_category: e.diagnostic.then(|| "diagnostic".to_string()),
                ..base_payload(&field, title(&field))
            };
            match e.component {
                Component::BinarySensor => {
                    p.payload_on = Some(DiscoveryPayload::Bool(true));
                    p.payload_off = Some(DiscoveryPayload::Bool(false));
                }
                Component::Switch => {
                    p.command_topic = Some(format!("{state_topic}/set"));
                    p.payload_on = Some(DiscoveryPayload::Text(r#"{"state":"ON"}"#.into()));
                    p.payload_off = Some(DiscoveryPayload::Text(r#"{"state":"OFF"}"#.into()));
                    p.state_on = text("ON");
                    p.state_off = text("OFF");
                }
                Component::Sensor => {}
            }
            out.push((config_topic(e.component.name(), &field), p));
        }
        if has(cluster_id::IAS_ZONE) {
            for (field, _) in ias_fields(device.ias_zone_type, 0) {
                // HA's door class is "on" when open, and `contact` is true
                // when closed.
                let p = DiscoveryConfig {
                    payload_on: Some(DiscoveryPayload::Bool(field != "contact")),
                    payload_off: Some(DiscoveryPayload::Bool(field == "contact")),
                    device_class: ias_class(field).map(str::to_string),
                    entity_category: matches!(field, "tamper" | "battery_low")
                        .then(|| "diagnostic".to_string()),
                    ..base_payload(field, title(field))
                };
                out.push((config_topic("binary_sensor", field), p));
            }
        }
    }
    let lq = DiscoveryConfig {
        unit_of_measurement: text("lqi"),
        state_class: text("measurement"),
        entity_category: text("diagnostic"),
        icon: text("mdi:signal"),
        ..base_payload("linkquality", "Link quality".into())
    };
    out.push((config_topic("sensor", "linkquality"), lq));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use zigbee_zdo::SimpleDescriptor;

    fn device() -> Device {
        let mut d = Device::new(Ieee(0x00124B0000000001), 0x1234, 0);
        d.model = Some("SIM-TH1".into());
        d.power_source = Some(3);
        d.endpoints = vec![SimpleDescriptor {
            endpoint: 1,
            profile_id: 0x0104,
            device_id: 0x0302,
            device_version: 1,
            input_clusters: vec![0, 1, 0x0402, 0x0405],
            output_clusters: vec![],
        }];
        d
    }

    #[test]
    fn converts_standard_readings() {
        let t = find(cluster_id::TEMPERATURE_MEASUREMENT, 0).unwrap();
        assert_eq!(convert(t, &Value::i16(2150)), Some(json!(21.5)));
        assert_eq!(
            convert(t, &Value::Int(zigbee_zcl::DataType::Int16, -32768)),
            None
        );
        let b = find(
            cluster_id::POWER_CONFIGURATION,
            attr::power_configuration::BATTERY_PERCENTAGE_REMAINING,
        )
        .unwrap();
        assert_eq!(convert(b, &Value::u8(200)), Some(json!(100.0)));
        let l = find(cluster_id::ILLUMINANCE_MEASUREMENT, 0).unwrap();
        assert_eq!(convert(l, &Value::u16(10001)), Some(json!(10.0)));
        let o = find(cluster_id::ON_OFF, 0).unwrap();
        assert_eq!(convert(o, &Value::bool(true)), Some(json!("ON")));
        assert_eq!(
            ias_fields(Some(0x0015), 0x0001)[0],
            ("contact", json!(false))
        );
    }

    #[test]
    fn discovery_describes_each_reading() {
        let d = device();
        let ctx = DiscoveryContext {
            prefix: "homeassistant",
            base: "zigbee",
            version: "0.1.0",
        };
        let messages = discovery(&ctx, &d, "0x00124b0000000001");
        let topics: Vec<&str> = messages.iter().map(|(t, _)| t.as_str()).collect();
        assert!(topics.contains(&"homeassistant/sensor/00124b0000000001/temperature/config"));
        assert!(topics.contains(&"homeassistant/sensor/00124b0000000001/battery/config"));
        assert!(topics.contains(&"homeassistant/sensor/00124b0000000001/linkquality/config"));
        let (_, t) = messages
            .iter()
            .find(|(t, _)| t.ends_with("/temperature/config"))
            .unwrap();
        assert_eq!(t.state_topic, "zigbee/0x00124b0000000001");
        assert_eq!(
            t.value_template.as_deref(),
            Some("{{ value_json.temperature }}")
        );
        assert_eq!(t.device_class.as_deref(), Some("temperature"));
        assert_eq!(t.unique_id, "0x00124b0000000001_temperature_zigbee-bridge");
    }
}

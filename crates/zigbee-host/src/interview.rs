//! The interview: learn what a new device is, then configure it.
//!
//! Each step's result is stored before the next runs, so an interview a
//! sleepy device interrupts resumes at the step that failed the next time
//! the device talks, instead of starting over.

use zigbee_types::Ieee;
use zigbee_zcl::foundation::ReportingConfig;
use zigbee_zcl::{attr, cluster_id, DataType, Value};
use zigbee_zdo::{Body, SimpleDescriptor};

use crate::adapter::Adapter;
use crate::device::{ConfiguredReport, InterviewStage};
use crate::error::HostError;
use crate::host::{Host, HostEvent};

/// Basic cluster attributes read at interview, in two small batches (some
/// devices reject long reads).
const BASIC_FIRST: &[u16] = &[
    attr::basic::MANUFACTURER_NAME,
    attr::basic::MODEL_IDENTIFIER,
    attr::basic::POWER_SOURCE,
];
const BASIC_SECOND: &[u16] = &[
    attr::basic::APPLICATION_VERSION,
    attr::basic::DATE_CODE,
    attr::basic::SW_BUILD_ID,
];
const BASIC_THIRD: &[u16] = &[
    attr::basic::ZCL_VERSION,
    attr::basic::STACK_VERSION,
    attr::basic::HW_VERSION,
];

/// Run or resume the interview of `ieee`.
pub(crate) async fn run<A: Adapter>(host: &Host<A>, ieee: Ieee) {
    host.emit_event(HostEvent::InterviewStarted { ieee });
    loop {
        let Some(device) = host.device(ieee) else {
            return;
        };
        let stage = device.interview.stage;
        if stage == InterviewStage::Done {
            break;
        }
        match step(host, ieee, stage).await {
            Ok(()) => {
                host.update_device(ieee, |d| {
                    d.interview.stage = stage.next();
                    d.interview.error = None;
                });
                host.emit_event(HostEvent::InterviewStep { ieee, stage });
            }
            Err(e) => {
                tracing::warn!(%ieee, ?stage, error = %e, "interview step failed");
                host.update_device(ieee, |d| {
                    d.interview.error = Some(e.to_string());
                    d.interview.attempts += 1;
                });
                host.emit_event(HostEvent::InterviewFailed {
                    ieee,
                    stage,
                    error: e.to_string(),
                });
                return;
            }
        }
    }
    if let Some(device) = host.device(ieee) {
        tracing::info!(%ieee, manufacturer = ?device.manufacturer, model = ?device.model, "interview done");
        host.mark_backup_wanted();
        host.emit_event(HostEvent::InterviewDone {
            device: Box::new(device),
        });
    }
}

async fn step<A: Adapter>(
    host: &Host<A>,
    ieee: Ieee,
    stage: InterviewStage,
) -> Result<(), HostError> {
    let device = host.device(ieee).ok_or(HostError::UnknownDevice(ieee))?;
    let nwk = device.nwk;
    match stage {
        InterviewStage::NodeDescriptor => {
            let m = host
                .zdo_request_retry(nwk, Body::NodeDescReq { nwk })
                .await?;
            match m.body {
                Body::NodeDescRsp {
                    status: 0,
                    descriptor: Some(d),
                    ..
                } => {
                    host.update_device(ieee, |dev| dev.node_descriptor = Some(d));
                    Ok(())
                }
                Body::NodeDescRsp { status, .. } => Err(HostError::Status {
                    what: "Node_Desc_req".into(),
                    status,
                }),
                _ => Err(HostError::BadFrame("unexpected Node_Desc reply".into())),
            }
        }
        InterviewStage::ActiveEndpoints => {
            let m = host
                .zdo_request_retry(nwk, Body::ActiveEpReq { nwk })
                .await?;
            match m.body {
                Body::ActiveEpRsp {
                    status: 0,
                    endpoints,
                    ..
                } => {
                    host.update_device(ieee, |dev| {
                        dev.endpoints = endpoints
                            .iter()
                            .map(|e| SimpleDescriptor {
                                endpoint: *e,
                                ..SimpleDescriptor::default()
                            })
                            .collect();
                    });
                    Ok(())
                }
                Body::ActiveEpRsp { status, .. } => Err(HostError::Status {
                    what: "Active_EP_req".into(),
                    status,
                }),
                _ => Err(HostError::BadFrame("unexpected Active_EP reply".into())),
            }
        }
        InterviewStage::SimpleDescriptors => {
            let pending: Vec<u8> = device
                .endpoints
                .iter()
                .filter(|e| {
                    e.profile_id == 0 && e.input_clusters.is_empty() && e.output_clusters.is_empty()
                })
                .map(|e| e.endpoint)
                .collect();
            for endpoint in pending {
                let m = host
                    .zdo_request_retry(nwk, Body::SimpleDescReq { nwk, endpoint })
                    .await?;
                match m.body {
                    Body::SimpleDescRsp {
                        status: 0,
                        descriptor: Some(d),
                        ..
                    } => {
                        host.update_device(ieee, |dev| {
                            if let Some(slot) =
                                dev.endpoints.iter_mut().find(|e| e.endpoint == endpoint)
                            {
                                *slot = d;
                            }
                        });
                    }
                    Body::SimpleDescRsp { status, .. } => {
                        return Err(HostError::Status {
                            what: format!("Simple_Desc_req({endpoint})"),
                            status,
                        })
                    }
                    _ => return Err(HostError::BadFrame("unexpected Simple_Desc reply".into())),
                }
            }
            Ok(())
        }
        InterviewStage::Basic => {
            let Some(endpoint) = device
                .endpoint_with(cluster_id::BASIC)
                .map(|e| e.endpoint)
                .or(device.endpoints.first().map(|e| e.endpoint))
            else {
                return Ok(());
            };
            for (i, batch) in [BASIC_FIRST, BASIC_SECOND, BASIC_THIRD]
                .into_iter()
                .enumerate()
            {
                let records = match host
                    .read_attributes(ieee, endpoint, cluster_id::BASIC, batch)
                    .await
                {
                    Ok(r) => r,
                    // Only the first batch is required.
                    Err(e) if i > 0 => {
                        tracing::debug!(%ieee, error = %e, "second Basic read");
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                host.update_device(ieee, |dev| {
                    for r in records {
                        let Some(value) = r.value else { continue };
                        match r.attribute {
                            attr::basic::MANUFACTURER_NAME => {
                                dev.manufacturer = value.as_str().map(clean)
                            }
                            attr::basic::MODEL_IDENTIFIER => dev.model = value.as_str().map(clean),
                            attr::basic::POWER_SOURCE => {
                                dev.power_source = value.as_u64().map(|v| v as u8)
                            }
                            attr::basic::APPLICATION_VERSION => {
                                dev.app_version = value.as_u64().map(|v| v as u8)
                            }
                            attr::basic::ZCL_VERSION => {
                                dev.zcl_version = value.as_u64().map(|v| v as u8)
                            }
                            attr::basic::STACK_VERSION => {
                                dev.stack_version = value.as_u64().map(|v| v as u8)
                            }
                            attr::basic::HW_VERSION => {
                                dev.hw_version = value.as_u64().map(|v| v as u8)
                            }
                            attr::basic::DATE_CODE => dev.date_code = value.as_str().map(clean),
                            attr::basic::SW_BUILD_ID => dev.sw_build_id = value.as_str().map(clean),
                            _ => {}
                        }
                    }
                });
            }
            Ok(())
        }
        InterviewStage::Configure => {
            if host.config().configure_reporting {
                configure(host, ieee).await;
            }
            Ok(())
        }
        InterviewStage::Done => Ok(()),
    }
}

/// Model and manufacturer strings arrive padded with NULs or spaces.
fn clean(s: &str) -> String {
    s.trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_string()
}

/// Bind and configure reporting for every server cluster that has default
/// reporting in the cluster library, and enroll IAS zones. Best effort: a
/// device that refuses one cluster still gets the others.
async fn configure<A: Adapter>(host: &Host<A>, ieee: Ieee) {
    let Some(device) = host.device(ieee) else {
        return;
    };
    let mut configured = Vec::new();
    for ep in &device.endpoints {
        for cluster in &ep.input_clusters {
            if *cluster == cluster_id::IAS_ZONE {
                if let Err(e) = enroll(host, ieee, ep.endpoint).await {
                    tracing::warn!(%ieee, endpoint = ep.endpoint, error = %e, "IAS zone enrollment");
                }
            }
            let Some(def) = zigbee_zcl::cluster(*cluster) else {
                continue;
            };
            let configs: Vec<ReportingConfig> = def
                .attributes
                .iter()
                .filter_map(|a| {
                    a.report.map(|r| ReportingConfig {
                        attribute: a.id,
                        data_type: a.data_type,
                        min_interval: r.min_interval,
                        max_interval: r.max_interval,
                        reportable_change: r.change.map(|c| change_value(a.data_type, c)),
                    })
                })
                .collect();
            if configs.is_empty() {
                continue;
            }
            if let Err(e) = host.bind_to_host(ieee, ep.endpoint, *cluster).await {
                tracing::warn!(%ieee, endpoint = ep.endpoint, cluster, error = %e, "bind");
                continue;
            }
            let attributes: Vec<u16> = configs.iter().map(|c| c.attribute).collect();
            match host
                .configure_reporting(ieee, ep.endpoint, *cluster, configs)
                .await
            {
                Ok(records) => {
                    for attribute in attributes {
                        // One record without an attribute means all succeeded.
                        let status = records
                            .iter()
                            .find(|r| r.attribute == Some(attribute))
                            .or_else(|| records.iter().find(|r| r.attribute.is_none()))
                            .map(|r| r.status)
                            .unwrap_or(zigbee_zcl::status::SUCCESS);
                        configured.push(ConfiguredReport {
                            endpoint: ep.endpoint,
                            cluster: *cluster,
                            attribute,
                            status,
                        });
                    }
                }
                Err(e) => {
                    tracing::warn!(%ieee, endpoint = ep.endpoint, cluster, error = %e, "configure reporting")
                }
            }
        }
    }
    host.update_device(ieee, |d| d.reporting = configured);
}

fn change_value(data_type: DataType, change: u64) -> Value {
    match data_type.class() {
        zigbee_zcl::types::Class::Signed => Value::Int(data_type, change as i64),
        zigbee_zcl::types::Class::Float => Value::Float(data_type, change as f64),
        _ => Value::Uint(data_type, change),
    }
}

/// Write the coordinator as the zone's CIE, then answer with an enroll
/// response (auto-enroll-response, which every IAS device accepts).
async fn enroll<A: Adapter>(host: &Host<A>, ieee: Ieee, endpoint: u8) -> Result<(), HostError> {
    if let Ok(records) = host
        .read_attributes(
            ieee,
            endpoint,
            cluster_id::IAS_ZONE,
            &[attr::ias_zone::ZONE_TYPE],
        )
        .await
    {
        if let Some(zone_type) = records
            .first()
            .and_then(|r| r.value.as_ref())
            .and_then(Value::as_u64)
        {
            host.update_device(ieee, |d| d.ias_zone_type = Some(zone_type as u16));
        }
    }
    let cie = Value::Ieee(host.network().ieee);
    host.write_attributes(
        ieee,
        endpoint,
        cluster_id::IAS_ZONE,
        vec![(attr::ias_zone::IAS_CIE_ADDRESS, cie)],
    )
    .await?;
    enroll_response(host, ieee, endpoint).await
}

/// Send an IAS zone enroll response with the device's zone id (assigning
/// the lowest free id the first time).
pub(crate) async fn enroll_response<A: Adapter>(
    host: &Host<A>,
    ieee: Ieee,
    endpoint: u8,
) -> Result<(), HostError> {
    let zone_id = match host.device(ieee).and_then(|d| d.ias_zone_id) {
        Some(id) => id,
        None => {
            let used = host.used_zone_ids();
            let id = (0u8..=0xFE).find(|i| !used.contains(i)).unwrap_or(0);
            host.update_device(ieee, |d| d.ias_zone_id = Some(id));
            id
        }
    };
    host.command(
        ieee,
        endpoint,
        cluster_id::IAS_ZONE,
        "zone_enroll_response",
        &[Value::enum8(0), Value::u8(zone_id)],
    )
    .await
    .map(|_| ())
}

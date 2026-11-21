//! The MQTT session: one connection, re-established by rumqttc's event
//! loop, with the bridge's availability as the last will.

use std::time::Duration;

use rumqttc::{AsyncClient, Event, LastWill, MqttOptions, Packet, QoS};
use tokio::sync::mpsc;

use crate::api::{Availability, BridgeState};
use crate::config::MqttConfig;

/// Something from the broker.
#[derive(Debug)]
pub enum Incoming {
    /// The session (re)connected: subscribe and republish retained state.
    Connected,
    /// A message on a subscribed topic.
    Message {
        /// Topic.
        topic: String,
        /// Payload.
        payload: Vec<u8>,
    },
}

/// A cloneable publisher.
#[derive(Clone, Debug)]
pub struct Mqtt {
    client: AsyncClient,
}

/// The availability topic.
pub fn state_topic(base: &str) -> String {
    format!("{base}/bridge/state")
}

/// Connect (in the background) and return the publisher and incoming
/// messages.
pub fn connect(config: &MqttConfig) -> Result<(Mqtt, mpsc::Receiver<Incoming>), String> {
    let mut options = MqttOptions::new(&config.client_id, &config.host, config.port);
    options.set_keep_alive(Duration::from_secs(config.keepalive.max(5)));
    options.set_clean_session(true);
    options.set_max_packet_size(256 * 1024, 256 * 1024);
    options.set_last_will(LastWill::new(
        state_topic(&config.base_topic),
        BridgeState::payload(Availability::Offline),
        QoS::AtLeastOnce,
        true,
    ));
    if let Some(user) = &config.username {
        let password = match &config.password_file {
            Some(path) => std::fs::read_to_string(path)
                .map_err(|e| format!("mqtt.password_file {}: {e}", path.display()))?
                .trim_end()
                .to_string(),
            None => String::new(),
        };
        options.set_credentials(user, password);
    }
    let (client, mut event_loop) = AsyncClient::new(options, 256);
    let (tx, rx) = mpsc::channel(256);
    tokio::spawn(async move {
        loop {
            match event_loop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    tracing::info!("MQTT connected");
                    if tx.send(Incoming::Connected).await.is_err() {
                        return;
                    }
                }
                Ok(Event::Incoming(Packet::Publish(p))) => {
                    let message = Incoming::Message {
                        topic: p.topic.clone(),
                        payload: p.payload.to_vec(),
                    };
                    if tx.send(message).await.is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "MQTT connection; retrying");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });
    Ok((Mqtt { client }, rx))
}

impl Mqtt {
    /// Publish without blocking the caller on the broker.
    pub async fn publish(
        &self,
        topic: impl Into<String>,
        payload: impl Into<Vec<u8>>,
        retain: bool,
    ) {
        let topic = topic.into();
        if let Err(e) = self
            .client
            .publish(topic.clone(), QoS::AtLeastOnce, retain, payload.into())
            .await
        {
            tracing::warn!(%topic, error = %e, "MQTT publish");
        }
    }

    /// Publish a value as JSON.
    pub async fn publish_json(
        &self,
        topic: impl Into<String>,
        value: &impl serde::Serialize,
        retain: bool,
    ) {
        self.publish(topic, serde_json::to_vec(value).unwrap_or_default(), retain)
            .await;
    }

    /// Subscribe.
    pub async fn subscribe(&self, filter: &str) {
        if let Err(e) = self.client.subscribe(filter, QoS::AtLeastOnce).await {
            tracing::warn!(%filter, error = %e, "MQTT subscribe");
        }
    }

    /// Disconnect cleanly (the will is not sent).
    pub async fn disconnect(&self) {
        let _ = self.client.disconnect().await;
    }
}

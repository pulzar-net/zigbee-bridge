//! The mesh as the coordinator sees it: links between devices, from the
//! last hop of every message, from route records, and from neighbor-table
//! scans. Passive sources cost nothing on the air; scans are explicit.

use std::collections::HashMap;

use serde::Serialize;
use zigbee_types::NodeId;

/// Where a link was learned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkSource {
    /// The last hop of a message the coordinator received.
    LastHop,
    /// A hop in a route record.
    RouteRecord,
    /// A router's neighbor table (`Mgmt_Lqi_rsp`).
    NeighborTable,
}

/// A link between two devices, from `from` to `to` (towards the
/// coordinator for passive links).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link {
    /// Sending side.
    pub from: NodeId,
    /// Receiving side.
    pub to: NodeId,
    /// Link quality, when known.
    pub lqi: Option<u8>,
    /// Signal strength in dBm, when known.
    pub rssi: Option<i8>,
    /// How it was learned.
    pub source: LinkSource,
    /// When, seconds since the Unix epoch.
    pub seen_unix: u64,
}

/// Links older than this are dropped.
pub const MAX_AGE_S: u64 = 24 * 3600;
/// The most links kept, so a large or hostile network cannot grow memory.
pub const MAX_LINKS: usize = 8192;

/// The link table.
#[derive(Debug, Default, Clone)]
pub struct Topology {
    links: HashMap<(NodeId, NodeId), Link>,
    /// The last route record from each source: relays nearest the source first.
    routes: HashMap<NodeId, Vec<NodeId>>,
}

impl Topology {
    fn put(&mut self, link: Link) {
        if self.links.len() >= MAX_LINKS && !self.links.contains_key(&(link.from, link.to)) {
            self.expire(link.seen_unix);
            if self.links.len() >= MAX_LINKS {
                if let Some(oldest) = self
                    .links
                    .values()
                    .min_by_key(|l| l.seen_unix)
                    .map(|l| (l.from, l.to))
                {
                    self.links.remove(&oldest);
                }
            }
        }
        let key = (link.from, link.to);
        match self.links.get_mut(&key) {
            // A passive or route-record hop without quality keeps the last
            // known quality.
            Some(old) if link.lqi.is_none() => {
                old.seen_unix = link.seen_unix;
            }
            _ => {
                self.links.insert(key, link);
            }
        }
    }

    /// A message from `source` arrived with this last-hop quality. The
    /// last hop is the last relay of `source`'s recorded route, or `source`
    /// itself when it has none.
    pub fn last_hop(
        &mut self,
        source: NodeId,
        coordinator: NodeId,
        lqi: u8,
        rssi: Option<i8>,
        now: u64,
    ) {
        let from = self
            .routes
            .get(&source)
            .and_then(|r| r.last().copied())
            .unwrap_or(source);
        self.put(Link {
            from,
            to: coordinator,
            lqi: Some(lqi),
            rssi,
            source: LinkSource::LastHop,
            seen_unix: now,
        });
    }

    /// A route record: `source` reached the coordinator through `relays`.
    pub fn route_record(
        &mut self,
        source: NodeId,
        relays: &[NodeId],
        coordinator: NodeId,
        lqi: Option<u8>,
        rssi: Option<i8>,
        now: u64,
    ) {
        let mut path = Vec::with_capacity(relays.len() + 2);
        path.push(source);
        path.extend_from_slice(relays);
        path.push(coordinator);
        for (i, pair) in path.windows(2).enumerate() {
            if let [from, to] = pair {
                let last = i + 2 == path.len();
                self.put(Link {
                    from: *from,
                    to: *to,
                    lqi: lqi.filter(|_| last),
                    rssi: rssi.filter(|_| last),
                    source: LinkSource::RouteRecord,
                    seen_unix: now,
                });
            }
        }
        self.routes.insert(source, relays.to_vec());
    }

    /// A router reported a neighbor and the quality it hears it at.
    pub fn neighbor(&mut self, router: NodeId, neighbor: NodeId, lqi: u8, now: u64) {
        self.put(Link {
            from: neighbor,
            to: router,
            lqi: Some(lqi),
            rssi: None,
            source: LinkSource::NeighborTable,
            seen_unix: now,
        });
    }

    /// Forget a device that left.
    pub fn remove_node(&mut self, node: NodeId) {
        self.links
            .retain(|(from, to), _| *from != node && *to != node);
        self.routes.remove(&node);
    }

    /// Drop links older than [`MAX_AGE_S`].
    pub fn expire(&mut self, now: u64) {
        self.links
            .retain(|_, l| now.saturating_sub(l.seen_unix) <= MAX_AGE_S);
    }

    /// Every current link.
    pub fn links(&self) -> Vec<Link> {
        let mut out: Vec<Link> = self.links.values().cloned().collect();
        out.sort_by_key(|l| (l.from, l.to));
        out
    }

    /// The recorded route of a device, relays nearest it first.
    pub fn route(&self, source: NodeId) -> Option<&[NodeId]> {
        self.routes.get(&source).map(Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_hop_follows_the_route_record() {
        let mut t = Topology::default();
        t.last_hop(0x1111, 0, 200, Some(-40), 10);
        t.route_record(0x2222, &[0x3333, 0x1111], 0, Some(150), Some(-60), 11);
        t.last_hop(0x2222, 0, 140, Some(-62), 12);
        let links = t.links();
        let hop = links
            .iter()
            .find(|l| l.from == 0x1111 && l.to == 0)
            .unwrap();
        assert_eq!(hop.lqi, Some(140));
        assert!(links
            .iter()
            .any(|l| l.from == 0x2222 && l.to == 0x3333 && l.source == LinkSource::RouteRecord));
        assert!(links.iter().any(|l| l.from == 0x3333 && l.to == 0x1111));
        t.expire(11 + MAX_AGE_S + 1);
        assert_eq!(t.links().len(), 1);
        t.remove_node(0x1111);
        assert!(t.links().is_empty());
    }

    #[test]
    fn table_is_bounded() {
        let mut t = Topology::default();
        for i in 0..(MAX_LINKS as u32 + 100) {
            t.neighbor((i % 65000) as u16 + 1, 0, 1, u64::from(i));
        }
        assert!(t.links().len() <= MAX_LINKS);
    }
}
